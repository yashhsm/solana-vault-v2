use anchor_spl::{
    associated_token::get_associated_token_address_with_program_id, token, token_2022,
};
use async_vault_v2_client::{
    extensions::instant_settlement::get_state as get_instant_settlement_state,
    lite::SendTransaction, sdk::program_id, InitializeInstantSettlementBuilder,
    InitializeProtocolFeeConfigBuilder, InitializeTranchesBuilder, InitializeVaultBuilder,
    InstantDepositBuilder, InstantRedeemBuilder, UpdateVaultBuilder, UpdateVaultNavBuilder, Vault,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, clock::Clock, instruction::AccountMeta, pubkey::Pubkey,
    signature::Keypair, signer::Signer, transaction::Transaction,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        assert_error_code, create_ata, create_mint, get_mint_supply, get_token_account_amount,
        helper_mint_to, set_share_balance, set_up_async_vault_v2,
    },
    async_vault_v2::constants::{
        EXTENSION_ALREADY_INITIALIZED, INSTANT_DEPOSIT_AMOUNT_ABOVE_MAXIMUM,
        INSTANT_DEPOSIT_AMOUNT_BELOW_MINIMUM, INSTANT_REDEEM_SHARES_ABOVE_MAXIMUM,
        INSTANT_REDEEM_SHARES_BELOW_MINIMUM, INVALID_ASSET_MINT_EXTENSIONS,
        INVALID_INSTANT_SETTLEMENT_THRESHOLD_CONFIG, INVALID_ROLLING_LIMIT_CONFIG, INVALID_VAULT,
        MISSING_REQUIRED_ACCOUNT, ROLLING_LIMIT_EXCEEDED, STALE_NAV, UNAUTHORIZED_SIGNER,
        UNSUPPORTED_PHASE_CONFIG, VAULT_ALREADY_INITIALIZED,
    },
};

const TRANCHE_CONFIG_SEED: &[u8] = b"tranches";
const INSTANT_USER_LIMIT_SEED: &[u8] = b"instant_user";
const PROTOCOL_FEE_CONFIG_SEED: &[u8] = b"protocol_fee_config";
const DEFAULT_INSTANT_NAV_STALENESS_SLOTS: u64 = 64;

fn add_program(svm: &mut LiteSVM) {
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
}

fn tranche_config_address(vault: Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[TRANCHE_CONFIG_SEED, vault.as_ref()], &program_id()).0
}

fn instant_user_address(vault: Pubkey, user: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[INSTANT_USER_LIMIT_SEED, vault.as_ref(), user.as_ref()],
        &program_id(),
    )
    .0
}

fn protocol_fee_config_address() -> Pubkey {
    Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &program_id()).0
}

fn user_asset_account(user: Pubkey, asset_mint: Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(&user, &asset_mint, &token::ID)
}

fn token_account(user: Pubkey, mint: Pubkey, token_program: Pubkey) -> Pubkey {
    get_associated_token_address_with_program_id(&user, &mint, &token_program)
}

fn read_vault(svm: &LiteSVM, vault: Pubkey) -> Vault {
    let account = svm.get_account(&vault).expect("vault should exist");
    Vault::from_bytes(account.data()).unwrap()
}

fn configure_nav_staleness(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault: Pubkey,
    max_nav_staleness_slots: u64,
) {
    let vault_config = read_vault(svm, vault);
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(vault_config.share_mint)
        .vault(vault)
        .max_nav_staleness_slots(max_nav_staleness_slots)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("nav staleness config should succeed");
}

fn enable_transfer_fee(
    svm: &mut LiteSVM,
    mint_authority: &Keypair,
    asset_mint: Pubkey,
    fee_bps: u16,
) {
    let ix = token_2022::spl_token_2022::extension::transfer_fee::instruction::set_transfer_fee(
        &token_2022::ID,
        &asset_mint,
        &mint_authority.pubkey(),
        &[],
        fee_bps,
        u64::MAX,
    )
    .unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&mint_authority.pubkey()),
        &[mint_authority],
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
        .expect("set_transfer_fee should succeed");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.epoch += 2;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
}

fn initialize_instant_settlement(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    vault: Pubkey,
    instant_redemption_fee_bps: u16,
) {
    initialize_instant_settlement_with_user_limits(
        svm,
        payer,
        authority,
        vault,
        instant_redemption_fee_bps,
        0,
        0,
    );
}

fn initialize_instant_settlement_with_user_limits(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    vault: Pubkey,
    instant_redemption_fee_bps: u16,
    max_user_deposit_amount: u64,
    max_user_redeem_shares: u64,
) {
    configure_nav_staleness(svm, authority, vault, DEFAULT_INSTANT_NAV_STALENESS_SLOTS);
    InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .instant_redemption_fee_bps(instant_redemption_fee_bps)
        .min_deposit_amount(0)
        .max_deposit_amount(0)
        .min_redeem_shares(0)
        .max_redeem_shares(0)
        .max_user_deposit_amount(max_user_deposit_amount)
        .max_user_redeem_shares(max_user_redeem_shares)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
        .expect("initialize instant settlement should succeed");
}

fn initialize_vault_and_nav(
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
        .expect("vault initialization should succeed");
    svm.expire_blockhash();
    update_vault_nav(svm, authority, vault);
}

fn update_vault_nav(svm: &mut LiteSVM, authority: &Keypair, vault: Pubkey) {
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("nav update should succeed");
}

fn configure_rolling_window(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: Pubkey,
    vault: Pubkey,
    window_slots: u64,
) {
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint)
        .vault(vault)
        .rolling_limit_window_slots(window_slots)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("rolling window update should succeed");
}

#[allow(clippy::too_many_arguments)]
fn instant_deposit(
    svm: &mut LiteSVM,
    user: &Keypair,
    asset_mint: Pubkey,
    share_mint: Pubkey,
    vault: Pubkey,
    reserve: Pubkey,
    user_asset_account: Pubkey,
    user_share_account: Pubkey,
    amount: u64,
) -> litesvm::types::TransactionResult {
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .vault(vault)
        .instant_user(Some(instant_user_address(vault, user.pubkey())))
        .vault_token_account(reserve)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(amount)
        .instruction()
        .send_transaction(svm, &user.pubkey(), &[user])
}

#[allow(clippy::too_many_arguments)]
fn instant_redeem(
    svm: &mut LiteSVM,
    user: &Keypair,
    asset_mint: Pubkey,
    share_mint: Pubkey,
    vault: Pubkey,
    reserve: Pubkey,
    user_asset_account: Pubkey,
    user_share_account: Pubkey,
    fee_recipient_token_account: Pubkey,
    shares: u64,
) -> litesvm::types::TransactionResult {
    InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .vault(vault)
        .instant_user(Some(instant_user_address(vault, user.pubkey())))
        .vault_token_account(reserve)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account)
        .fee_recipient_token_account(Some(fee_recipient_token_account))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(shares)
        .instruction()
        .send_transaction(svm, &user.pubkey(), &[user])
}

#[test]
fn test_initialize_instant_settlement_rejects_non_curator() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        _asset_mint,
        _share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let err = InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(user.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(0)
        .min_deposit_amount(0)
        .max_deposit_amount(0)
        .min_redeem_shares(0)
        .max_redeem_shares(0)
        .max_user_deposit_amount(0)
        .max_user_redeem_shares(0)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &user])
        .unwrap_err();

    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
    assert_ne!(authority.pubkey(), user.pubkey());
}

#[test_case(true, false, VAULT_ALREADY_INITIALIZED, "VaultAlreadyInitialized" ; "after_vault_init")]
#[test_case(false, true, EXTENSION_ALREADY_INITIALIZED, "ExtensionAlreadyInitialized" ; "duplicate")]
fn test_initialize_instant_settlement_fails(
    init_vault_first: bool,
    init_extension_first: bool,
    expected_error: u32,
    expected_name: &str,
) {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        _asset_mint,
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

    if init_vault_first {
        InitializeVaultBuilder::new()
            .share_mint(share_mint.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("vault initialization should succeed");
    }
    if init_extension_first {
        initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
        svm.expire_blockhash();
    }
    configure_nav_staleness(
        &mut svm,
        &authority,
        vault_pubkey,
        DEFAULT_INSTANT_NAV_STALENESS_SLOTS,
    );

    let err = InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(1)
        .min_deposit_amount(0)
        .max_deposit_amount(0)
        .min_redeem_shares(0)
        .max_redeem_shares(0)
        .max_user_deposit_amount(0)
        .max_user_redeem_shares(0)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .unwrap_err();

    assert_error_code(&err, expected_error, expected_name);
}

#[test]
fn test_initialize_instant_settlement_stores_thresholds() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        _asset_mint,
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

    configure_nav_staleness(
        &mut svm,
        &authority,
        vault_pubkey,
        DEFAULT_INSTANT_NAV_STALENESS_SLOTS,
    );
    InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(125)
        .min_deposit_amount(10)
        .max_deposit_amount(100)
        .min_redeem_shares(5)
        .max_redeem_shares(50)
        .max_user_deposit_amount(300)
        .max_user_redeem_shares(70)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("initialize instant settlement should succeed");

    let vault_account = svm.get_account(&vault_pubkey).expect("vault should exist");
    let extension =
        get_instant_settlement_state(vault_account.data()).expect("extension should exist");
    assert!(extension.enabled);
    assert_eq!(extension.min_deposit_amount, 10);
    assert_eq!(extension.max_deposit_amount, 100);
    assert_eq!(extension.min_redeem_shares, 5);
    assert_eq!(extension.max_redeem_shares, 50);
    assert_eq!(extension.max_user_deposit_amount, 300);
    assert_eq!(extension.max_user_redeem_shares, 70);
}

#[test_case(100, 99, 0, 0 ; "deposit_min_above_max")]
#[test_case(0, 0, 100, 99 ; "redeem_min_above_max")]
fn test_initialize_instant_settlement_rejects_invalid_threshold_config(
    min_deposit_amount: u64,
    max_deposit_amount: u64,
    min_redeem_shares: u64,
    max_redeem_shares: u64,
) {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        _asset_mint,
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

    configure_nav_staleness(
        &mut svm,
        &authority,
        vault_pubkey,
        DEFAULT_INSTANT_NAV_STALENESS_SLOTS,
    );
    let err = InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(1)
        .min_deposit_amount(min_deposit_amount)
        .max_deposit_amount(max_deposit_amount)
        .min_redeem_shares(min_redeem_shares)
        .max_redeem_shares(max_redeem_shares)
        .max_user_deposit_amount(0)
        .max_user_redeem_shares(0)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .unwrap_err();

    assert_error_code(
        &err,
        INVALID_INSTANT_SETTLEMENT_THRESHOLD_CONFIG,
        "InvalidInstantSettlementThresholdConfig",
    );
}

#[test_case(0, true ; "zero_fee")]
#[test_case(1, false ; "zero_staleness")]
fn test_initialize_instant_settlement_requires_fee_and_staleness(
    instant_redemption_fee_bps: u16,
    configure_staleness: bool,
) {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        _asset_mint,
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

    if configure_staleness {
        configure_nav_staleness(
            &mut svm,
            &authority,
            vault_pubkey,
            DEFAULT_INSTANT_NAV_STALENESS_SLOTS,
        );
    }

    let err = InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(instant_redemption_fee_bps)
        .min_deposit_amount(0)
        .max_deposit_amount(0)
        .min_redeem_shares(0)
        .max_redeem_shares(0)
        .max_user_deposit_amount(0)
        .max_user_redeem_shares(0)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .unwrap_err();

    assert_error_code(
        &err,
        INVALID_INSTANT_SETTLEMENT_THRESHOLD_CONFIG,
        "InvalidInstantSettlementThresholdConfig",
    );
}

#[test]
fn test_instant_deposit_respects_thresholds() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);

    configure_nav_staleness(
        &mut svm,
        &authority,
        vault_pubkey,
        DEFAULT_INSTANT_NAV_STALENESS_SLOTS,
    );
    InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(1)
        .min_deposit_amount(20)
        .max_deposit_amount(40)
        .min_redeem_shares(0)
        .max_redeem_shares(0)
        .max_user_deposit_amount(0)
        .max_user_redeem_shares(0)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("initialize instant settlement should succeed");
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());

    svm.expire_blockhash();
    let below_min = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(19)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(
        &below_min,
        INSTANT_DEPOSIT_AMOUNT_BELOW_MINIMUM,
        "InstantDepositAmountBelowMinimum",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );

    svm.expire_blockhash();
    let above_max = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(41)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(
        &above_max,
        INSTANT_DEPOSIT_AMOUNT_ABOVE_MAXIMUM,
        "InstantDepositAmountAboveMaximum",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );

    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(20)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit at threshold should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before + 20
    );
}

#[test]
fn test_instant_redeem_respects_thresholds() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);

    configure_nav_staleness(
        &mut svm,
        &authority,
        vault_pubkey,
        DEFAULT_INSTANT_NAV_STALENESS_SLOTS,
    );
    InitializeInstantSettlementBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instant_redemption_fee_bps(1)
        .min_deposit_amount(0)
        .max_deposit_amount(0)
        .min_redeem_shares(20)
        .max_redeem_shares(40)
        .max_user_deposit_amount(0)
        .max_user_redeem_shares(0)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("initialize instant settlement should succeed");
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(100)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit should fund reserve");

    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());

    svm.expire_blockhash();
    let below_min = InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account)
        .fee_recipient_token_account(Some(fee_recipient_ata))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(19)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(
        &below_min,
        INSTANT_REDEEM_SHARES_BELOW_MINIMUM,
        "InstantRedeemSharesBelowMinimum",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );

    svm.expire_blockhash();
    let above_max = InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(41)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(
        &above_max,
        INSTANT_REDEEM_SHARES_ABOVE_MAXIMUM,
        "InstantRedeemSharesAboveMaximum",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );

    svm.expire_blockhash();
    InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account)
        .fee_recipient_token_account(Some(fee_recipient_ata))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(40)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant redeem at threshold should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before + 39
    );
}

#[test]
fn test_zero_user_limits_do_not_create_instant_user_bucket() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    let instant_user = instant_user_address(vault_pubkey, user.pubkey());
    assert!(svm.get_account(&instant_user).is_none());

    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(20)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("zero-limit instant deposit should not require an instant user account");
    assert!(svm.get_account(&instant_user).is_none());

    svm.expire_blockhash();
    InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account)
        .fee_recipient_token_account(Some(fee_recipient_ata))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("zero-limit instant redeem should not require an instant user account");
    assert!(svm.get_account(&instant_user).is_none());
}

#[test]
fn test_instant_deposit_user_limit_requires_window_config() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement_with_user_limits(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        1,
        50,
        0,
    );
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    let assets_before = get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());

    svm.expire_blockhash();
    let err = instant_deposit(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        10,
    )
    .unwrap_err();

    assert_error_code(
        &err,
        INVALID_ROLLING_LIMIT_CONFIG,
        "InvalidRollingLimitConfig",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
}

#[test]
fn test_instant_deposit_user_limit_requires_bucket_account() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement_with_user_limits(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        1,
        50,
        0,
    );
    configure_rolling_window(&mut svm, &authority, share_mint.pubkey(), vault_pubkey, 10);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    let assets_before = get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());

    svm.expire_blockhash();
    let err = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();

    assert_error_code(&err, MISSING_REQUIRED_ACCOUNT, "MissingRequiredAccount");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
}

#[test]
fn test_instant_deposit_user_limit_is_per_user_and_resets() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000);
    initialize_instant_settlement_with_user_limits(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        1,
        60,
        0,
    );
    configure_rolling_window(&mut svm, &authority, share_mint.pubkey(), vault_pubkey, 10);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    svm.expire_blockhash();
    instant_deposit(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        50,
    )
    .expect("first instant deposit should fit user limit");

    let second_user = Keypair::new();
    svm.airdrop(&second_user.pubkey(), 1_000_000_000).unwrap();
    let second_user_asset_account =
        create_ata(&mut svm, &second_user, &asset_mint.pubkey(), &token::ID);
    let second_user_share_account =
        create_ata(&mut svm, &second_user, &share_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &second_user_asset_account,
        &mint_authority,
        100,
        &token::ID,
    );

    svm.expire_blockhash();
    instant_deposit(
        &mut svm,
        &second_user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        second_user_asset_account,
        second_user_share_account,
        60,
    )
    .expect("second user should have an independent instant deposit bucket");

    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let user_shares_before =
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());

    svm.expire_blockhash();
    let err = instant_deposit(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        11,
    )
    .unwrap_err();
    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        user_shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot += 10;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    instant_deposit(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        11,
    )
    .expect("instant deposit should succeed after user window resets");
}

#[test]
fn test_instant_redeem_user_limit_resets() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000);
    initialize_instant_settlement_with_user_limits(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        1,
        0,
        60,
    );
    configure_rolling_window(&mut svm, &authority, share_mint.pubkey(), vault_pubkey, 10);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = user_asset_account(user.pubkey(), asset_mint.pubkey());
    svm.expire_blockhash();
    instant_deposit(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        100,
    )
    .expect("instant deposit should fund reserve");

    svm.expire_blockhash();
    instant_redeem(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        fee_recipient_ata,
        50,
    )
    .expect("first instant redeem should fit user limit");

    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let user_shares_before =
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());

    svm.expire_blockhash();
    let err = instant_redeem(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        fee_recipient_ata,
        11,
    )
    .unwrap_err();
    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        user_shares_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot += 10;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    instant_redeem(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        user_asset_account,
        user_share_account,
        fee_recipient_ata,
        11,
    )
    .expect("instant redeem should succeed after user window resets");
}

#[test]
fn test_instant_deposit_disabled_without_extension() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        _payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let err = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();

    assert_error_code(&err, 0, "InstantSettlementDisabled");
}

#[test]
fn test_instant_deposit_succeeds_for_primary_asset() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_assets_before = get_token_account_amount(
        &svm.get_account(&user_asset_account(user.pubkey(), asset_mint.pubkey()))
            .unwrap(),
    );
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let supply_before = get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap());

    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(40)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit should succeed");

    assert_eq!(
        get_token_account_amount(
            &svm.get_account(&user_asset_account(user.pubkey(), asset_mint.pubkey()))
                .unwrap()
        ),
        user_assets_before - 40
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before + 40
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before + 40
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        supply_before + 40
    );
    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.total_asset_balance, 40);
}

#[test]
fn test_instant_deposit_rejects_reenabled_token_2022_transfer_fee() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token_2022::ID, Some(0), token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = token_account(user.pubkey(), asset_mint.pubkey(), token_2022::ID);
    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let supply_before = get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap());

    enable_transfer_fee(&mut svm, &mint_authority, asset_mint.pubkey(), 1);

    let err = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token_2022::ID)
        .share_token_program(token::ID)
        .amount(20)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(
        &err,
        INVALID_ASSET_MINT_EXTENSIONS,
        "InvalidAssetMintExtensions",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        supply_before
    );
}

#[test]
fn test_instant_redeem_succeeds_with_instant_fee() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1_000);
    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");
    svm.expire_blockhash();
    update_vault_nav(&mut svm, &authority, vault_pubkey);

    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(100)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit should succeed");

    let user_assets_before = get_token_account_amount(
        &svm.get_account(&user_asset_account(user.pubkey(), asset_mint.pubkey()))
            .unwrap(),
    );
    let fee_assets_before = get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap());
    svm.expire_blockhash();
    InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .fee_recipient_token_account(Some(fee_recipient_ata))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(40)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant redeem should succeed");

    assert_eq!(
        get_token_account_amount(
            &svm.get_account(&user_asset_account(user.pubkey(), asset_mint.pubkey()))
                .unwrap()
        ),
        user_assets_before + 36
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        fee_assets_before + 4
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        60
    );
    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.total_asset_balance, 60);
}

#[test]
fn test_instant_redeem_protocol_fee_splits_total_instant_fees() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1_000);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let protocol_fee_recipient = Keypair::new();
    svm.airdrop(&protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let protocol_fee_recipient_ata = create_ata(
        &mut svm,
        &protocol_fee_recipient,
        &asset_mint.pubkey(),
        &token::ID,
    );
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .protocol_fee_bps(2_500)
        .protocol_fee_recipient(protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("protocol fee config should succeed");

    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(100)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit should succeed");

    svm.expire_blockhash();
    InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .fee_recipient_token_account(Some(fee_recipient_ata))
        .protocol_fee_recipient_token_account(Some(protocol_fee_recipient_ata))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(40)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant redeem should split protocol fee");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        3,
        "fee recipient should receive total fee less protocol skim"
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&protocol_fee_recipient_ata).unwrap()),
        1,
        "protocol recipient should receive rounded-up skim"
    );
}

#[test]
fn test_instant_redeem_protocol_fee_uses_program_config_recipient_when_supplied() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1_000);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

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
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .protocol_fee_bps(2_500)
        .protocol_fee_recipient(vault_protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("protocol fee config should succeed");

    let protocol_fee_config = protocol_fee_config_address();
    InitializeProtocolFeeConfigBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(configured_protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize protocol fee config should succeed");

    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(100)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit should succeed");

    svm.expire_blockhash();
    InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .fee_recipient_token_account(Some(fee_recipient_ata))
        .protocol_fee_recipient_token_account(Some(configured_protocol_fee_recipient_ata))
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(40)
        .add_remaining_account(AccountMeta::new_readonly(protocol_fee_config, false))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant redeem should use program-level protocol fee recipient");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_ata).unwrap()),
        3
    );
    assert_eq!(
        get_token_account_amount(
            &svm.get_account(&configured_protocol_fee_recipient_ata)
                .unwrap()
        ),
        1
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&vault_protocol_fee_recipient_ata).unwrap()),
        0
    );
}

#[test]
fn test_instant_redeem_rejects_reenabled_token_2022_transfer_fee() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token_2022::ID, Some(0), token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let user_asset_account = token_account(user.pubkey(), asset_mint.pubkey(), token_2022::ID);
    svm.expire_blockhash();
    InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account)
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token_2022::ID)
        .share_token_program(token::ID)
        .amount(100)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("instant deposit should succeed while transfer fee is zero");

    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    let shares_before = get_token_account_amount(&svm.get_account(&user_share_account).unwrap());
    let supply_before = get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap());

    enable_transfer_fee(&mut svm, &mint_authority, asset_mint.pubkey(), 1);

    let err = InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token_2022::ID)
        .share_token_program(token::ID)
        .shares(20)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(
        &err,
        INVALID_ASSET_MINT_EXTENSIONS,
        "InvalidAssetMintExtensions",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        reserve_before
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_share_account).unwrap()),
        shares_before
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        supply_before
    );
}

#[test]
fn test_instant_settlement_rejects_non_reserve_token_account() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
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
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
    initialize_vault_and_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    svm.expire_blockhash();
    let deposit_err = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(pending_vault_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(&deposit_err, INVALID_VAULT, "InvalidVault");

    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 10);
    svm.expire_blockhash();
    let redeem_err = InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(pending_vault_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(&redeem_err, INVALID_VAULT, "InvalidVault");
}

#[test]
fn test_instant_settlement_rejects_stale_nav_and_insufficient_liquidity() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        _mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);
    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");
    svm.expire_blockhash();
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .max_nav_staleness_slots(1)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("staleness config should succeed");
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("nav update should succeed");

    let mut clock = svm.get_sysvar::<solana_sdk::clock::Clock>();
    clock.slot = clock.slot.saturating_add(2);
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    let stale_err = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(&stale_err, STALE_NAV, "StaleNav");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .max_nav_staleness_slots(DEFAULT_INSTANT_NAV_STALENESS_SLOTS)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("restore staleness config should succeed");
    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 10);
    svm.expire_blockhash();
    let liquidity_err = InstantRedeemBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_share_account(user_share_account)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .shares(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(&liquidity_err, 0, "InsufficientLiquidity");
}

#[test]
fn test_instant_deposit_rejects_tranche_enabled_vault() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    initialize_instant_settlement(&mut svm, &payer, &authority, vault_pubkey, 1);

    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);
    svm.expire_blockhash();
    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts([0; 4])
        .max_request_amounts([0; 4])
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");
    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    svm.expire_blockhash();
    let err = InstantDepositBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instant_user(Some(instant_user_address(vault_pubkey, user.pubkey())))
        .vault_token_account(reserve_pubkey)
        .user_asset_account(user_asset_account(user.pubkey(), asset_mint.pubkey()))
        .user_share_account(user_share_account)
        .fee_recipient_token_account(None)
        .asset_token_program(token::ID)
        .share_token_program(token::ID)
        .amount(10)
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .unwrap_err();
    assert_error_code(&err, UNSUPPORTED_PHASE_CONFIG, "UnsupportedPhaseConfig");
}
