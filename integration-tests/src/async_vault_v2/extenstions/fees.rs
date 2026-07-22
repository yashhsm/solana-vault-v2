use anchor_spl::{associated_token::get_associated_token_address_with_program_id, token};
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, ApproveRequestBuilder, CreateDepositRequestBuilder,
    CreateRedeemRequestBuilder, FeeType, InitializeDepositFeeBuilder,
    InitializeVaultBuilder as InitializeAsyncVaultBuilder, InitializeWithdrawalFeeBuilder, Request,
    RequestArgs, RequestState, UpdateVaultBuilder as UpdateVaultAsyncBuilder,
    UpdateVaultNavBuilder, Vault,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, instruction::AccountMeta, pubkey::Pubkey, signature::Keypair,
    signer::Signer, transaction::Transaction,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, create_ata, get_token_account_amount,
        helper_mint_to, initialize_and_activate_protocol_fee_config, set_share_balance,
        set_up_async_vault_v2, set_vault_total_asset_balance,
    },
    async_vault_v2::constants::{
        INSUFFICIENT_DEPOSIT_AMOUNT, MISSING_FEE_RECIPIENT, ROLLING_LIMIT_EXCEEDED,
    },
};

// NAV: 200_000_000_000 with 9 decimals → shares = assets/200, assets = shares*200
const NAV: u128 = 200_000_000_000;
#[allow(clippy::too_many_arguments)]
fn setup_with_fees(
    deposit_fee: Option<FeeType>,
    withdrawal_fee: Option<FeeType>,
) -> (
    LiteSVM,
    Keypair, // authority
    Keypair, // mint_authority
    Keypair, // asset_mint
    Keypair, // share_mint
    Keypair, // user
    Pubkey,  // reserve_pubkey
    Pubkey,  // vault_pubkey
    Pubkey,  // pending_vault_pubkey
    Pubkey,  // fee_recipient_ata
    Pubkey,  // user_share_account
) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    if let Some(fee) = deposit_fee {
        InitializeDepositFeeBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .deposit_fee(fee)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("init deposit fee should succeed");
    }

    if let Some(fee) = withdrawal_fee {
        InitializeWithdrawalFeeBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .withdrawal_fee(fee)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("init withdrawal fee should succeed");
    }

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
        .updated_nav(NAV)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update nav should succeed");

    (
        svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    )
}

fn approve_request_with_fee_recipient(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault_pubkey: Pubkey,
    request_pubkey: Pubkey,
    asset_mint: Pubkey,
    share_mint: Pubkey,
    reserve_pubkey: Pubkey,
    pending_vault_pubkey: Pubkey,
    fee_recipient_ata: Option<Pubkey>,
    protocol_fee_config: Option<Pubkey>,
    protocol_fee_recipient_ata: Option<Pubkey>,
) -> Result<litesvm::types::TransactionMetadata, litesvm::types::FailedTransactionMetadata> {
    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(svm, &request_pubkey);
    let mut builder = ApproveRequestBuilder::new();
    builder
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_pubkey)
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID);

    if let Some(ata) = fee_recipient_ata {
        builder.add_remaining_account(AccountMeta::new(ata, false));
    }
    if let Some(config) = protocol_fee_config {
        builder.add_remaining_account(AccountMeta::new_readonly(config, false));
    }
    if let Some(ata) = protocol_fee_recipient_ata {
        builder.add_remaining_account(AccountMeta::new(ata, false));
    }

    builder
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

fn configure_protocol_fee(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: Pubkey,
    vault_pubkey: Pubkey,
    protocol_fee_recipient: Pubkey,
    protocol_fee_bps: u16,
) {
    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint)
        .vault(vault_pubkey)
        .protocol_fee_bps(protocol_fee_bps)
        .protocol_fee_recipient(protocol_fee_recipient)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("set protocol fee should succeed");
}

// deposit_amount=1_000_000, fixed fee=1_000 → net=999_000, shares=4_995
#[test_case(FeeType::FixedAmount { amount: 1_000 }, 1_000_000, 1_000, 999_000, 4_995 ; "fixed_fee")]
// deposit_amount=1_000_000, 1% fee → fee=10_000, net=990_000, shares=4_950
#[test_case(FeeType::Percentage { bps: 100 }, 1_000_000, 10_000, 990_000, 4_950 ; "percentage_fee")]
fn test_approve_deposit_with_fee(
    fee: FeeType,
    deposit_amount: u64,
    expected_fee: u64,
    expected_net: u64,
    expected_shares: u64,
) {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        _user_share_account,
    ) = setup_with_fees(Some(fee), None);

    let user_asset_account = get_associated_token_address_with_program_id(
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
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: deposit_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    let pending_before = get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let fee_recipient_before =
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap());

    approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        None,
        None,
    )
    .expect("approve_request with deposit fee should succeed");

    let vault_after = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(vault_after.pending_async_requests, 0);
    assert_eq!(
        vault_after.total_asset_balance, expected_net,
        "total_asset_balance should equal net deposit (after fee)"
    );

    let request_after = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("request should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(request_after.request_state, RequestState::Claimable);
    assert_eq!(request_after.price, NAV);
    assert_eq!(
        request_after.amount, expected_shares,
        "request.amount should be shares calculated from net deposit"
    );

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        fee_recipient_before + expected_fee,
        "fee_recipient_ata should receive the fee"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before + expected_net,
        "reserve should receive the net deposit"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        pending_before - deposit_amount,
        "pending_vault should be fully drained"
    );
}

#[test]
fn test_approve_deposit_protocol_fee_splits_deposit_fee() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        _user_share_account,
    ) = setup_with_fees(Some(FeeType::Percentage { bps: 100 }), None);

    let protocol_fee_recipient = Keypair::new();
    svm.airdrop(&protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let protocol_fee_recipient_ata = create_ata(
        &mut svm,
        &protocol_fee_recipient,
        &asset_mint.pubkey(),
        &token::ID,
    );
    configure_protocol_fee(
        &mut svm,
        &authority,
        share_mint.pubkey(),
        vault_pubkey,
        protocol_fee_recipient.pubkey(),
        2_500,
    );

    let user_asset_account = get_associated_token_address_with_program_id(
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
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        None,
        Some(protocol_fee_recipient_ata),
    )
    .expect("approve_request with protocol deposit fee should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        7_500
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&protocol_fee_recipient_ata).unwrap()),
        2_500
    );
    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.total_asset_balance, 990_000);
}

#[test]
fn test_approve_deposit_protocol_fee_uses_program_config_recipient_when_supplied() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        _user_share_account,
    ) = setup_with_fees(Some(FeeType::Percentage { bps: 100 }), None);

    let vault_protocol_fee_recipient = Keypair::new();
    let configured_protocol_fee_recipient = Keypair::new();
    svm.airdrop(&vault_protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    svm.airdrop(&configured_protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let vault_protocol_fee_recipient_ata = create_ata(
        &mut svm,
        &vault_protocol_fee_recipient,
        &asset_mint.pubkey(),
        &token::ID,
    );
    let configured_protocol_fee_recipient_ata = create_ata(
        &mut svm,
        &configured_protocol_fee_recipient,
        &asset_mint.pubkey(),
        &token::ID,
    );
    configure_protocol_fee(
        &mut svm,
        &authority,
        share_mint.pubkey(),
        vault_pubkey,
        vault_protocol_fee_recipient.pubkey(),
        2_500,
    );

    let protocol_fee_config = initialize_and_activate_protocol_fee_config(
        &mut svm,
        &authority,
        configured_protocol_fee_recipient.pubkey(),
    );

    let user_asset_account = get_associated_token_address_with_program_id(
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
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        Some(protocol_fee_config),
        Some(configured_protocol_fee_recipient_ata),
    )
    .expect("approve_request should use program-level protocol fee recipient");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        7_500
    );
    assert_eq!(
        get_token_account_amount(
            &svm.get_account(&configured_protocol_fee_recipient_ata)
                .unwrap()
        ),
        2_500
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&vault_protocol_fee_recipient_ata).unwrap()),
        0
    );
}

#[test]
fn test_approve_deposit_protocol_fee_rejects_wrong_protocol_recipient_owner() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        _user_share_account,
    ) = setup_with_fees(Some(FeeType::Percentage { bps: 100 }), None);

    let protocol_fee_recipient = Keypair::new();
    let wrong_protocol_owner = Keypair::new();
    svm.airdrop(&protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    svm.airdrop(&wrong_protocol_owner.pubkey(), 1_000_000_000)
        .unwrap();
    let wrong_protocol_fee_recipient_ata = create_ata(
        &mut svm,
        &wrong_protocol_owner,
        &asset_mint.pubkey(),
        &token::ID,
    );
    configure_protocol_fee(
        &mut svm,
        &authority,
        share_mint.pubkey(),
        vault_pubkey,
        protocol_fee_recipient.pubkey(),
        2_500,
    );

    let user_asset_account = get_associated_token_address_with_program_id(
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
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    let fee_recipient_before =
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap());
    let wrong_protocol_before =
        get_token_account_amount(&svm.get_account(&wrong_protocol_fee_recipient_ata).unwrap());

    let err = approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        None,
        Some(wrong_protocol_fee_recipient_ata),
    )
    .unwrap_err();
    assert!(
        format!("{err:?}").contains("InvalidAccountOwner"),
        "expected low-level token account owner validation failure, got {err:?}"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        fee_recipient_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&wrong_protocol_fee_recipient_ata).unwrap()),
        wrong_protocol_before
    );
}

// redeem_shares=5_000, gross_assets=1_000_000, fixed fee=1_000 → net=999_000
#[test_case(FeeType::FixedAmount { amount: 1_000 }, 5_000, 1_000_000, 1_000, 999_000 ; "fixed_fee")]
// redeem_shares=5_000, gross_assets=1_000_000, 1% fee=10_000 → net=990_000
#[test_case(FeeType::Percentage { bps: 100 }, 5_000, 1_000_000, 10_000, 990_000 ; "percentage_fee")]
fn test_approve_redeem_with_fee(
    fee: FeeType,
    redeem_shares: u64,
    gross_assets: u64,
    expected_fee: u64,
    expected_net_assets: u64,
) {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = setup_with_fees(None, Some(fee));

    // Fund reserve so there are enough assets to cover the redeem
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        gross_assets,
        &token::ID,
    );
    set_vault_total_asset_balance(&mut svm, vault_pubkey, gross_assets);

    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        redeem_shares,
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
            amount: redeem_shares,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create redeem request should succeed");

    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let pending_before = get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap());
    let fee_recipient_before =
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap());

    approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        None,
        None,
    )
    .expect("approve_request with withdrawal fee should succeed");

    let vault_after = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(vault_after.pending_async_requests, 0);
    assert_eq!(
        vault_after.total_asset_balance, 0,
        "total_asset_balance should be decremented by gross assets"
    );

    let request_after = Request::from_bytes(
        svm.get_account(&request_keypair.pubkey())
            .expect("request should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(request_after.request_state, RequestState::Claimable);
    assert_eq!(request_after.price, NAV);
    assert_eq!(
        request_after.amount, expected_net_assets,
        "request.amount should be net assets (after withdrawal fee)"
    );

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        fee_recipient_before + expected_fee,
        "fee_recipient_ata should receive the withdrawal fee"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        pending_before + expected_net_assets,
        "pending_vault should receive net assets"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before - gross_assets,
        "reserve should be reduced by gross assets (fee + net)"
    );
}

#[test]
fn test_approve_redeem_protocol_fee_splits_withdrawal_fee() {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = setup_with_fees(None, Some(FeeType::Percentage { bps: 100 }));

    let protocol_fee_recipient = Keypair::new();
    svm.airdrop(&protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let protocol_fee_recipient_ata = create_ata(
        &mut svm,
        &protocol_fee_recipient,
        &asset_mint.pubkey(),
        &token::ID,
    );
    configure_protocol_fee(
        &mut svm,
        &authority,
        share_mint.pubkey(),
        vault_pubkey,
        protocol_fee_recipient.pubkey(),
        2_500,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000_000,
        &token::ID,
    );
    set_vault_total_asset_balance(&mut svm, vault_pubkey, 1_000_000);
    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 5_000);

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
            amount: 5_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create redeem request should succeed");

    approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        None,
        Some(protocol_fee_recipient_ata),
    )
    .expect("approve_request with protocol withdrawal fee should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        7_500
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&protocol_fee_recipient_ata).unwrap()),
        2_500
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        990_000
    );
    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.total_asset_balance, 0);
}

#[test]
fn test_redeem_rolling_limit_counts_gross_assets_before_withdrawal_fee() {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = setup_with_fees(None, Some(FeeType::FixedAmount { amount: 100_000 }));

    let gross_assets = 1_000_000u64;
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        gross_assets,
        &token::ID,
    );
    set_vault_total_asset_balance(&mut svm, vault_pubkey, gross_assets);
    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 5_000);

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .rolling_limit_window_slots(10)
        .redemption_rolling_limit(950_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set redemption rolling limit should succeed");

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
            amount: 5_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create redeem request should succeed");

    let err = approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        Some(fee_recipient_ata),
        None,
        None,
    )
    .unwrap_err();

    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");
}

#[test]
fn test_approve_deposit_no_fee_no_remaining_account() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = setup_with_fees(None, None);

    let deposit_amount = 1_000_000u64;
    let user_asset_account = get_associated_token_address_with_program_id(
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
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: deposit_amount,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("create deposit request should succeed");

    // No remaining account needed when no fee extension is set
    approve_request_with_fee_recipient(
        &mut svm,
        &authority,
        vault_pubkey,
        request_keypair.pubkey(),
        asset_mint.pubkey(),
        share_mint.pubkey(),
        reserve_pubkey,
        pending_vault_pubkey,
        None,
        None,
        None,
    )
    .expect("approve_request without fee should succeed without remaining account");
}

#[test_case(true ; "deposit")]
#[test_case(false ; "redeem")]
fn test_approve_fee_missing_remaining_account_fails(is_deposit: bool) {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = setup_with_fees(
        Some(FeeType::FixedAmount { amount: 1_000 }),
        Some(FeeType::FixedAmount { amount: 1_000 }),
    );

    let request_keypair = Keypair::new();

    if is_deposit {
        let user_asset_account = get_associated_token_address_with_program_id(
            &user.pubkey(),
            &asset_mint.pubkey(),
            &token::ID,
        );
        CreateDepositRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .request(request_keypair.pubkey())
            .vault(vault_pubkey)
            .user_token_account(user_asset_account)
            .pending_vault(pending_vault_pubkey)
            .asset_token_program(spl_token::ID)
            .args(RequestArgs {
                amount: 1_000_000,
                operator: None,
            })
            .instruction()
            .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
            .expect("create deposit request should succeed");
    } else {
        let gross_assets = 1_000_000u64;
        helper_mint_to(
            &mut svm,
            &asset_mint.pubkey(),
            &reserve_pubkey,
            &mint_authority,
            gross_assets,
            &token::ID,
        );
        set_vault_total_asset_balance(&mut svm, vault_pubkey, gross_assets);
        set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 5_000);

        CreateRedeemRequestBuilder::new()
            .user(user.pubkey())
            .asset_mint(asset_mint.pubkey())
            .share_mint(share_mint.pubkey())
            .request(request_keypair.pubkey())
            .vault(vault_pubkey)
            .user_share_account(user_share_account)
            .share_token_program(spl_token::ID)
            .args(RequestArgs {
                amount: 5_000,
                operator: None,
            })
            .instruction()
            .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
            .expect("create redeem request should succeed");
    }

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    let ix = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction();

    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&authority.pubkey()),
        &[&authority],
        svm.latest_blockhash(),
    );
    let err = svm.send_transaction(tx).unwrap_err();
    assert_error_code(&err, MISSING_FEE_RECIPIENT, "MissingFeeRecipient");
}

#[test]
fn test_create_deposit_below_fixed_fee_rejected() {
    let (
        mut svm,
        _authority,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = setup_with_fees(Some(FeeType::FixedAmount { amount: 1_000 }), None);

    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    let request_keypair = Keypair::new();
    let err = CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(request_keypair.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 500,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .unwrap_err();
    assert_error_code(
        &err,
        INSUFFICIENT_DEPOSIT_AMOUNT,
        "Deposit amount too small.",
    );
}
