use anchor_spl::token;
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, AddVaultAssetBuilder, ApproveRequestBuilder,
    CancelRequestBuilder, ClaimBuilder, CreateDepositRequestBuilder, CreateRedeemRequestBuilder,
    InitializeVaultBuilder, RejectRequestBuilder, RemoveVaultAssetBuilder, RequestArgs,
    UpdateVaultBuilder, UpdateVaultNavBuilder, Vault, VaultAsset,
};
use litesvm::LiteSVM;
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey, signature::Keypair, signer::Signer};

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, create_ata, create_mint, get_token_account_amount,
        helper_mint_to, set_share_balance, set_up_async_vault_v2,
    },
    async_vault_v2::constants::{
        ASSET_ALREADY_APPROVED, ASSET_BALANCE_NON_ZERO, DEPOSIT_CAP_EXCEEDED, INVALID_ASSET_MINT,
        INVALID_PENDING_VAULT, MAX_APPROVED_ASSETS_EXCEEDED, TIMELOCK_REQUIRED,
        UNAUTHORIZED_SIGNER,
    },
};

const ASSET_CONFIG_SEED: &[u8] = b"asset";
const ASSET_RESERVE_SEED: &[u8] = b"asset_reserve";
const ASSET_PENDING_SEED: &[u8] = b"asset_pending";

fn load_program(svm: &mut LiteSVM) {
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
}

fn derive_asset_accounts(vault: Pubkey, asset_mint: Pubkey) -> (Pubkey, Pubkey, Pubkey) {
    let (vault_asset, _) = Pubkey::find_program_address(
        &[ASSET_CONFIG_SEED, vault.as_ref(), asset_mint.as_ref()],
        &program_id(),
    );
    let (reserve, _) = Pubkey::find_program_address(
        &[ASSET_RESERVE_SEED, vault.as_ref(), asset_mint.as_ref()],
        &program_id(),
    );
    let (pending_vault, _) = Pubkey::find_program_address(
        &[ASSET_PENDING_SEED, vault.as_ref(), asset_mint.as_ref()],
        &program_id(),
    );
    (vault_asset, reserve, pending_vault)
}

fn add_vault_asset(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    vault: Pubkey,
    asset_mint: Pubkey,
    deposit_cap: u64,
) -> litesvm::types::TransactionResult {
    let (vault_asset, reserve, pending_vault) = derive_asset_accounts(vault, asset_mint);
    AddVaultAssetBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .asset_mint(asset_mint)
        .vault_asset(vault_asset)
        .reserve(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(token::ID)
        .deposit_cap(deposit_cap)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
}

fn remove_vault_asset(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault: Pubkey,
    asset_mint: Pubkey,
) -> litesvm::types::TransactionResult {
    let (vault_asset, reserve, pending_vault) = derive_asset_accounts(vault, asset_mint);
    RemoveVaultAssetBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .asset_mint(asset_mint)
        .vault_asset(vault_asset)
        .reserve(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

fn read_vault(svm: &LiteSVM, vault: Pubkey) -> Vault {
    let account = svm.get_account(&vault).expect("vault should exist");
    Vault::from_bytes(account.data()).unwrap()
}

fn read_vault_asset(svm: &LiteSVM, vault_asset: Pubkey) -> VaultAsset {
    let account = svm
        .get_account(&vault_asset)
        .expect("vault asset should exist");
    VaultAsset::from_bytes(account.data()).unwrap()
}

fn initialize_and_set_nav(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: Pubkey,
    vault: Pubkey,
) {
    InitializeVaultBuilder::new()
        .share_mint(share_mint)
        .authority(authority.pubkey())
        .vault(vault)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize vault should succeed");

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("update nav should succeed");
}

fn closed_or_empty(svm: &LiteSVM, account: Pubkey) -> bool {
    svm.get_account(&account)
        .map(|account| account.lamports() == 0)
        .unwrap_or(true)
}

#[test]
fn test_add_vault_asset_initializes_accounts() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let (vault_asset_pubkey, reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());

    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        123,
    )
    .expect("add_vault_asset should succeed");

    let vault = read_vault(&svm, vault_pubkey);
    assert_eq!(vault.approved_asset_count, 2);

    let vault_asset = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(vault_asset.vault, vault_pubkey);
    assert_eq!(vault_asset.asset_mint, asset_mint.pubkey());
    assert_eq!(vault_asset.reserve, reserve_pubkey);
    assert_eq!(vault_asset.pending_vault, pending_vault_pubkey);
    assert_eq!(vault_asset.deposit_cap, 123);
    assert_eq!(vault_asset.idle_balance, 0);
    assert_eq!(vault_asset.deployed_balance, 0);
    assert_eq!(vault_asset.pending_deposit_amount, 0);
    assert_eq!(vault_asset.manager_window_start_slot, 0);
    assert_eq!(vault_asset.manager_window_amount, 0);

    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        0
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        0
    );
}

#[test]
fn test_secondary_asset_deposit_approve_and_claim_updates_asset_ledger() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
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

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let user_asset_account = create_ata(&mut svm, &user, &asset_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        1_000_000,
        &token::ID,
    );
    let (vault_asset_pubkey, reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        1_000_000,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 400_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("secondary-asset deposit request should succeed");

    let vault_after_create = read_vault(&svm, vault_pubkey);
    let asset_after_create = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(vault_after_create.pending_deposit_amount, 0);
    assert_eq!(asset_after_create.pending_deposit_amount, 400_000);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        400_000
    );

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve secondary-asset deposit should succeed");

    let asset_after_approve = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(asset_after_approve.pending_deposit_amount, 0);
    assert_eq!(asset_after_approve.idle_balance, 400_000);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        0
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        400_000
    );

    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .request(request_keypair.pubkey())
        .pending_vault(None)
        .user_share_account(Some(user_share_account))
        .user_asset_account(None)
        .asset_token_program(token::ID)
        .share_token_program(Some(token::ID))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("claim secondary-asset deposit shares should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        400_000
    );
}

#[test]
fn test_secondary_asset_deposit_cancel_releases_asset_reservation() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let user_asset_account = create_ata(&mut svm, &user, &asset_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        500_000,
        &token::ID,
    );
    let (vault_asset_pubkey, _reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 250_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("secondary-asset deposit request should succeed");

    CancelRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_token_account(Some(user_asset_account))
        .asset_pending_vault(Some(pending_vault_pubkey))
        .user_share_account(None)
        .share_token_program(None)
        .asset_token_program(Some(token::ID))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("cancel secondary-asset deposit should succeed");

    let asset_after_cancel = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(asset_after_cancel.pending_deposit_amount, 0);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        500_000
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        0
    );
}

#[test]
fn test_secondary_asset_deposit_reject_releases_asset_reservation() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let user_asset_account = create_ata(&mut svm, &user, &asset_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        500_000,
        &token::ID,
    );
    let (vault_asset_pubkey, _reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 250_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("secondary-asset deposit request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    RejectRequestBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user(user.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .user_token_account(Some(user_asset_account))
        .asset_pending_vault(Some(pending_vault_pubkey))
        .user_share_account(None)
        .share_token_program(None)
        .asset_token_program(Some(token::ID))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("reject secondary-asset deposit should succeed");

    let asset_after_reject = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(asset_after_reject.pending_deposit_amount, 0);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        500_000
    );
}

#[test]
fn test_secondary_asset_deposit_requires_vault_asset_account() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let user_asset_account = create_ata(&mut svm, &user, &asset_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        100_000,
        &token::ID,
    );
    let (vault_asset_pubkey, _reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let request_keypair = Keypair::new();
    let result = CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 10_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair]);

    assert_error_code(&result.unwrap_err(), INVALID_ASSET_MINT, "InvalidAssetMint");
    assert_eq!(
        read_vault_asset(&svm, vault_asset_pubkey).pending_deposit_amount,
        0
    );
}

#[test]
fn test_secondary_asset_deposit_cap_is_enforced() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let user_asset_account = create_ata(&mut svm, &user, &asset_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        100_000,
        &token::ID,
    );
    let (vault_asset_pubkey, _reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        50_000,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let request_keypair = Keypair::new();
    let result = CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 50_001,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair]);

    assert_error_code(
        &result.unwrap_err(),
        DEPOSIT_CAP_EXCEEDED,
        "DepositCapExceeded",
    );
    assert_eq!(
        read_vault_asset(&svm, vault_asset_pubkey).pending_deposit_amount,
        0
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        0
    );
}

#[test]
fn test_add_vault_asset_rejects_non_curator() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        _authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();
    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);

    let result = add_vault_asset(
        &mut svm,
        &payer,
        &unauthorized,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    );

    assert_error_code(
        &result.unwrap_err(),
        UNAUTHORIZED_SIGNER,
        "UnauthorizedSigner",
    );
}

#[test]
fn test_add_vault_asset_rejects_primary_asset_mint() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        _mint_authority,
        primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let result = add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        primary_asset_mint.pubkey(),
        0,
    );

    assert_error_code(
        &result.unwrap_err(),
        ASSET_ALREADY_APPROVED,
        "AssetAlreadyApproved",
    );
}

#[test]
fn test_add_vault_asset_rejects_duplicate_secondary_asset() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("initial secondary asset approval should succeed");

    let result = add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    );

    assert!(result.is_err());
    let vault = read_vault(&svm, vault_pubkey);
    assert_eq!(vault.approved_asset_count, 2);
}

#[test]
fn test_add_vault_asset_rejects_max_assets() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    for _ in 0..7 {
        let asset_mint = Keypair::new();
        create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
        add_vault_asset(
            &mut svm,
            &payer,
            &authority,
            vault_pubkey,
            asset_mint.pubkey(),
            0,
        )
        .expect("asset should be approved up to the configured maximum");
    }

    let vault = read_vault(&svm, vault_pubkey);
    assert_eq!(vault.approved_asset_count, 8);

    let extra_asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &extra_asset_mint, &token::ID);
    let result = add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        extra_asset_mint.pubkey(),
        0,
    );

    assert_error_code(
        &result.unwrap_err(),
        MAX_APPROVED_ASSETS_EXCEEDED,
        "MaxApprovedAssetsExceeded",
    );
}

#[test]
fn test_remove_vault_asset_closes_zero_balance_asset() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let (vault_asset_pubkey, reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");

    remove_vault_asset(&mut svm, &authority, vault_pubkey, asset_mint.pubkey())
        .expect("remove_vault_asset should succeed when balances are zero");

    let vault = read_vault(&svm, vault_pubkey);
    assert_eq!(vault.approved_asset_count, 1);
    assert!(closed_or_empty(&svm, vault_asset_pubkey));
    assert!(closed_or_empty(&svm, reserve_pubkey));
    assert!(closed_or_empty(&svm, pending_vault_pubkey));
}

#[test]
fn test_remove_vault_asset_rejects_nonzero_token_balance() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        _share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let (vault_asset_pubkey, reserve_pubkey, _pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1,
        &token::ID,
    );

    let result = remove_vault_asset(&mut svm, &authority, vault_pubkey, asset_mint.pubkey());

    assert_error_code(
        &result.unwrap_err(),
        ASSET_BALANCE_NON_ZERO,
        "AssetBalanceNonZero",
    );
    assert!(svm.get_account(&vault_asset_pubkey).is_some());
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        1
    );
}

#[test]
fn test_add_vault_asset_requires_timelock_queue_when_enabled() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        _user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("curator should enable the timelock before queued-only changes");

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let result = add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    );

    assert_error_code(&result.unwrap_err(), TIMELOCK_REQUIRED, "TimelockRequired");
}

#[test]
fn test_secondary_asset_redeem_approve_and_claim_updates_asset_ledger() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 500_000);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let user_asset_account = create_ata(&mut svm, &user, &asset_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        500_000,
        &token::ID,
    );
    let (vault_asset_pubkey, reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let deposit_request = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(deposit_request.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 400_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &deposit_request])
        .expect("secondary-asset deposit request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &deposit_request.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(deposit_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve secondary-asset deposit should succeed");

    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .request(deposit_request.pubkey())
        .pending_vault(None)
        .user_share_account(Some(user_share_account))
        .user_asset_account(None)
        .asset_token_program(token::ID)
        .share_token_program(Some(token::ID))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("claim secondary-asset deposit shares should succeed");

    let redeem_request = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(redeem_request.pubkey())
        .user_share_account(user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 150_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &redeem_request])
        .expect("secondary-asset redeem request should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        250_000
    );

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &redeem_request.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(redeem_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve secondary-asset redeem should succeed");

    let asset_after_approve = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(asset_after_approve.idle_balance, 250_000);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        250_000
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        150_000
    );

    let wrong_pending_claim = ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(redeem_request.pubkey())
        .pending_vault(Some(reserve_pubkey))
        .user_share_account(None)
        .user_asset_account(Some(user_asset_account))
        .asset_token_program(token::ID)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user]);
    assert_error_code(
        &wrong_pending_claim.unwrap_err(),
        INVALID_PENDING_VAULT,
        "InvalidPendingVault",
    );

    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(redeem_request.pubkey())
        .pending_vault(Some(pending_vault_pubkey))
        .user_share_account(None)
        .user_asset_account(Some(user_asset_account))
        .asset_token_program(token::ID)
        .share_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("claim secondary-asset redeem should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        250_000
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&pending_vault_pubkey).unwrap()),
        0
    );
}

#[test]
fn test_secondary_asset_redeem_cancel_restores_shares() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
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

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let (vault_asset_pubkey, _reserve_pubkey, _pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);
    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 200_000);

    let request_keypair = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_share_account(user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 75_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("secondary-asset redeem request should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        125_000
    );

    CancelRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .request(request_keypair.pubkey())
        .user_token_account(None)
        .asset_pending_vault(None)
        .user_share_account(Some(user_share_account))
        .share_token_program(Some(token::ID))
        .asset_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("cancel secondary-asset redeem should restore shares");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        200_000
    );
}

#[test]
fn test_secondary_asset_redeem_reject_restores_shares() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
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

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    let (vault_asset_pubkey, _reserve_pubkey, _pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);
    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 200_000);

    let request_keypair = Keypair::new();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_share_account(user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 75_000,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair])
        .expect("secondary-asset redeem request should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        125_000
    );

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &request_keypair.pubkey());
    RejectRequestBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .request(request_keypair.pubkey())
        .user(user.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .user_token_account(None)
        .asset_pending_vault(None)
        .user_share_account(Some(user_share_account))
        .share_token_program(Some(token::ID))
        .asset_token_program(None)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("reject secondary-asset redeem should restore shares");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        200_000
    );
}

#[test]
fn test_secondary_asset_redeem_request_requires_vault_asset_account() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
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

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &token::ID);
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        0,
    )
    .expect("add_vault_asset should succeed");
    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);
    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 100);

    let request_keypair = Keypair::new();
    let result = CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .request(request_keypair.pubkey())
        .user_share_account(user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 100,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request_keypair]);

    assert_error_code(&result.unwrap_err(), INVALID_ASSET_MINT, "InvalidAssetMint");
}
