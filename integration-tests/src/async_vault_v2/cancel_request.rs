use anchor_spl::{associated_token::get_associated_token_address_with_program_id, token};
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, CancelRequestBuilder, CreateDepositRequestBuilder,
    CreateRedeemRequestBuilder, InitializeVaultBuilder as InitializeAsyncVaultBuilder, Request,
    RequestArgs, RequestState, UpdateVaultBuilder as UpdateVaultAsyncBuilder,
    UpdateVaultNavBuilder, Vault,
};
use borsh::BorshSerialize;
use litesvm::LiteSVM;
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey, signature::Keypair, signer::Signer};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        assert_error_code, create_ata, get_token_account_amount, set_share_balance,
        set_up_async_vault_v2, set_vault_pending_async_requests,
    },
    async_vault_v2::constants::{
        ARITHMETIC_ERROR, MISSING_REQUIRED_ACCOUNT, PAUSED_VAULT, REQUEST_IS_NOT_PENDING,
        UNAUTHORIZED_SIGNER,
    },
};

#[derive(Clone, Copy)]
enum CancelDepositFailure {
    RequestNotPending,
    MissingRefundAccount,
    PendingRequestsUnderflow,
}

fn set_request_state(svm: &mut LiteSVM, request_pubkey: Pubkey, state: RequestState) {
    let mut account = svm.get_account(&request_pubkey).unwrap();
    let mut request = Request::from_bytes(account.data()).unwrap();
    request.request_state = state;
    let mut buf = Vec::new();
    request.serialize(&mut buf).unwrap();
    let tlv_bytes = account.data()[buf.len()..].to_vec();
    buf.extend_from_slice(&tlv_bytes);
    account.data = buf;
    svm.set_account(request_pubkey, account).unwrap();
}

#[test_case(1_000_000 ; "cancel deposit request refunds user")]
#[test_case(1 ; "cancel minimum deposit succeeds")]
#[test_case(500_000_000 ; "cancel large deposit refunds full amount")]
fn test_cancel_deposit_request(deposit_amount: u64) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let user_amount = 1_000_000_000;
    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, Some(0), token::ID, user_amount);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    let user_token_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    let request_keypair = Keypair::new();

    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_token_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: deposit_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("deposit request should succeed");

    let user_balance_after_deposit =
        get_token_account_amount(&svm.get_account(&user_token_account).unwrap());
    assert_eq!(user_balance_after_deposit, user_amount - deposit_amount);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        deposit_amount
    );

    let vault_before = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    let pending_before = vault_before.pending_async_requests;

    let user_pubkey = user.pubkey();
    CancelRequestBuilder::new()
        .user(user_pubkey)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(Some(user_token_account))
        .asset_pending_vault(Some(pending_vault_pubkey))
        .asset_token_program(Some(token::ID))
        .user_share_account(None)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user_pubkey, &[user])
        .expect("cancel deposit request should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_token_account).unwrap()),
        user_amount,
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        0
    );

    assert!(
        svm.get_account(&request_keypair.pubkey()).is_none(),
        "Request account should be closed"
    );

    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.pending_async_requests, pending_before - 1,);
}

#[test_case(true ; "wrong user cannot cancel request")]
#[test_case(false ; "paused vault rejects cancel")]
fn test_cancel_deposit_request_fails(wrong_user: bool) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let user_amount = 1_000_000_000;
    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, Some(0), token::ID, user_amount);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    let user_token_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_token_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("deposit request should succeed");

    if !wrong_user {
        UpdateVaultAsyncBuilder::new()
            .authority(authority.pubkey())
            .share_mint(share_mint.pubkey())
            .paused(true)
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("pause should succeed");
    }

    let cancel_signer = if wrong_user {
        let attacker = Keypair::new();
        svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
        let _ = create_ata(&mut svm, &attacker, &asset_mint.pubkey(), &token::ID);
        attacker
    } else {
        user
    };

    let cancel_user_ata = get_associated_token_address_with_program_id(
        &cancel_signer.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    let cancel_signer_pubkey = cancel_signer.pubkey();
    let err = CancelRequestBuilder::new()
        .user(cancel_signer_pubkey)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(Some(cancel_user_ata))
        .asset_pending_vault(Some(pending_vault_pubkey))
        .asset_token_program(Some(token::ID))
        .user_share_account(None)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &cancel_signer_pubkey, &[cancel_signer])
        .unwrap_err();

    if wrong_user {
        assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
    } else {
        assert_error_code(&err, PAUSED_VAULT, "PausedVault");
    }
}

#[test_case(CancelDepositFailure::RequestNotPending, REQUEST_IS_NOT_PENDING, "RequestIsNotPending" ; "request not pending")]
#[test_case(CancelDepositFailure::MissingRefundAccount, MISSING_REQUIRED_ACCOUNT, "MissingRequiredAccount" ; "missing refund account")]
#[test_case(CancelDepositFailure::PendingRequestsUnderflow, ARITHMETIC_ERROR, "ArithmeticError" ; "pending requests underflow")]
fn test_cancel_deposit_request_negative(
    failure: CancelDepositFailure,
    expected_error_code: u32,
    expected_error_name: &str,
) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let user_amount = 1_000_000_000;
    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, Some(0), token::ID, user_amount);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    let user_token_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_token_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("deposit request should succeed");

    if matches!(failure, CancelDepositFailure::RequestNotPending) {
        set_request_state(&mut svm, request_keypair.pubkey(), RequestState::Claimable);
    }

    if matches!(failure, CancelDepositFailure::PendingRequestsUnderflow) {
        set_vault_pending_async_requests(&mut svm, vault_pubkey, 0);
    }

    let user_token_account = if matches!(failure, CancelDepositFailure::MissingRefundAccount) {
        None
    } else {
        Some(user_token_account)
    };

    let user_pubkey = user.pubkey();
    let err = CancelRequestBuilder::new()
        .user(user_pubkey)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_token_account)
        .asset_pending_vault(Some(pending_vault_pubkey))
        .asset_token_program(Some(token::ID))
        .user_share_account(None)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user_pubkey, &[&user])
        .unwrap_err();

    assert_error_code(&err, expected_error_code, expected_error_name);
}

#[test_case(1_000_000_000 ; "cancel redeem request mints shares back")]
#[test_case(500_000_000 ; "cancel partial redeem mints correct amount")]
fn test_cancel_redeem_request(share_amount: u64) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        share_amount,
    );

    let request_keypair = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_share_account(user_share_account)
        .share_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: share_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("redeem request should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        0
    );

    let vault_before = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    let pending_before = vault_before.pending_async_requests;

    let user_pubkey = user.pubkey();
    CancelRequestBuilder::new()
        .user(user_pubkey)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(None)
        .asset_pending_vault(None)
        .asset_token_program(None)
        .user_share_account(Some(user_share_account))
        .share_token_program(Some(token::ID))
        .instruction()
        .send_transaction(&mut svm, &user_pubkey, &[user])
        .expect("cancel redeem request should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        share_amount,
    );

    assert!(
        svm.get_account(&request_keypair.pubkey()).is_none(),
        "Request account should be closed"
    );

    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.pending_async_requests, pending_before - 1);
}

#[test]
fn test_cancel_redeem_request_missing_mint_back_account_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000,
    );

    let request_keypair = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_share_account(user_share_account)
        .share_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("redeem request should succeed");

    let user_pubkey = user.pubkey();
    let err = CancelRequestBuilder::new()
        .user(user_pubkey)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(None)
        .asset_pending_vault(None)
        .asset_token_program(None)
        .user_share_account(None)
        .share_token_program(Some(token::ID))
        .instruction()
        .send_transaction(&mut svm, &user_pubkey, &[&user])
        .unwrap_err();

    assert_error_code(&err, MISSING_REQUIRED_ACCOUNT, "MissingRequiredAccount");
}
