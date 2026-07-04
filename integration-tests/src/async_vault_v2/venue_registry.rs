use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, AddVaultAssetBuilder, ApproveRequestBuilder,
    ApproveVaultVenueBuilder, CreateDepositRequestBuilder, CreateVenuePositionBuilder,
    DeployVenuePositionBuilder, InitializeVaultBuilder, Position, PullVenuePositionBuilder,
    RegisterVenueBuilder, RemoveVaultVenueBuilder, RemoveVenuePositionBuilder, RequestArgs,
    SetVenueEntryPausedBuilder, UpdateVaultBuilder, UpdateVaultNavBuilder, Vault, VaultAsset,
    VaultVenue, VenueEntry, VenueType,
};
use litesvm::LiteSVM;
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey, signature::Keypair, signer::Signer};

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, create_ata, create_mint, get_token_account_amount,
        helper_mint_to, set_up_async_vault_v2,
    },
    async_vault_v2::constants::{
        INVALID_ASSET_MINT, INVALID_VENUE_DISCRIMINATOR_COUNT, POSITION_BALANCE_NON_ZERO,
        ROLLING_LIMIT_EXCEEDED, TIMELOCK_REQUIRED, UNAUTHORIZED_SIGNER, VENUE_PAUSED,
    },
};

const VENUE_ENTRY_SEED: &[u8] = b"venue";
const VAULT_VENUE_SEED: &[u8] = b"vault_venue";
const POSITION_SEED: &[u8] = b"position";
const POSITION_TOKEN_SEED: &[u8] = b"position_token";
const ASSET_CONFIG_SEED: &[u8] = b"asset";
const ASSET_RESERVE_SEED: &[u8] = b"asset_reserve";
const ASSET_PENDING_SEED: &[u8] = b"asset_pending";

fn load_program(svm: &mut LiteSVM) {
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
}

fn venue_id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn venue_discriminators() -> [u8; 64] {
    let mut discriminators = [0_u8; 64];
    discriminators[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    discriminators[8..16].copy_from_slice(&[8, 7, 6, 5, 4, 3, 2, 1]);
    discriminators
}

fn derive_venue_entry(registry_authority: Pubkey, venue_id: [u8; 32]) -> Pubkey {
    Pubkey::find_program_address(
        &[
            VENUE_ENTRY_SEED,
            registry_authority.as_ref(),
            venue_id.as_ref(),
        ],
        &program_id(),
    )
    .0
}

fn derive_vault_venue(vault: Pubkey, venue_entry: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[VAULT_VENUE_SEED, vault.as_ref(), venue_entry.as_ref()],
        &program_id(),
    )
    .0
}

fn derive_position(vault: Pubkey, venue_entry: Pubkey, asset_mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            POSITION_SEED,
            vault.as_ref(),
            venue_entry.as_ref(),
            asset_mint.as_ref(),
        ],
        &program_id(),
    )
    .0
}

fn derive_position_token(vault: Pubkey, venue_entry: Pubkey, asset_mint: Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            POSITION_TOKEN_SEED,
            vault.as_ref(),
            venue_entry.as_ref(),
            asset_mint.as_ref(),
        ],
        &program_id(),
    )
    .0
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

fn register_venue(
    svm: &mut LiteSVM,
    payer: &Keypair,
    registry_authority: &Keypair,
    venue_id: [u8; 32],
    discriminator_count: u8,
) -> litesvm::types::TransactionResult {
    register_venue_with_routine_safe(
        svm,
        payer,
        registry_authority,
        venue_id,
        discriminator_count,
        true,
    )
}

fn register_venue_with_routine_safe(
    svm: &mut LiteSVM,
    payer: &Keypair,
    registry_authority: &Keypair,
    venue_id: [u8; 32],
    discriminator_count: u8,
    routine_safe: bool,
) -> litesvm::types::TransactionResult {
    let venue_entry = derive_venue_entry(registry_authority.pubkey(), venue_id);
    RegisterVenueBuilder::new()
        .payer(payer.pubkey())
        .registry_authority(registry_authority.pubkey())
        .venue_entry(venue_entry)
        .venue_id(venue_id)
        .target_program(spl_token::ID)
        .allowed_discriminator_count(discriminator_count)
        .allowed_discriminators(venue_discriminators())
        .risk_class(2)
        .venue_type(VenueType::ExternalProtocol)
        .routine_safe(routine_safe)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, registry_authority])
}

fn approve_vault_venue(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    vault: Pubkey,
    venue_entry: Pubkey,
) -> litesvm::types::TransactionResult {
    let vault_venue = derive_vault_venue(vault, venue_entry);
    ApproveVaultVenueBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
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
        .asset_token_program(spl_token::ID)
        .deposit_cap(deposit_cap)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
}

fn create_venue_position(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    vault: Pubkey,
    asset_mint: Pubkey,
    vault_asset: Option<Pubkey>,
    venue_entry: Pubkey,
) -> litesvm::types::TransactionResult {
    let vault_venue = derive_vault_venue(vault, venue_entry);
    let position = derive_position(vault, venue_entry, asset_mint);
    let position_token_account = derive_position_token(vault, venue_entry, asset_mint);
    CreateVenuePositionBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .asset_mint(asset_mint)
        .vault_asset(vault_asset)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .position(position)
        .position_token_account(position_token_account)
        .asset_token_program(spl_token::ID)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
}

struct PositionAccounts {
    vault: Pubkey,
    asset_mint: Pubkey,
    vault_asset: Option<Pubkey>,
    venue_entry: Pubkey,
    vault_venue: Pubkey,
    position: Pubkey,
    vault_token_account: Pubkey,
    position_token_account: Pubkey,
}

struct SecondaryAssetAccounts {
    asset_mint: Pubkey,
    vault_asset: Pubkey,
    reserve: Pubkey,
}

fn position_accounts(
    vault: Pubkey,
    asset_mint: Pubkey,
    vault_asset: Option<Pubkey>,
    venue_entry: Pubkey,
    vault_token_account: Pubkey,
) -> PositionAccounts {
    PositionAccounts {
        vault,
        asset_mint,
        vault_asset,
        venue_entry,
        vault_venue: derive_vault_venue(vault, venue_entry),
        position: derive_position(vault, venue_entry, asset_mint),
        vault_token_account,
        position_token_account: derive_position_token(vault, venue_entry, asset_mint),
    }
}

fn deploy_position(
    svm: &mut LiteSVM,
    authority: &Keypair,
    accounts: &PositionAccounts,
    amount: u64,
) -> litesvm::types::TransactionResult {
    DeployVenuePositionBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(accounts.asset_mint)
        .vault(accounts.vault)
        .vault_asset(accounts.vault_asset)
        .venue_entry(accounts.venue_entry)
        .vault_venue(accounts.vault_venue)
        .position(accounts.position)
        .vault_token_account(accounts.vault_token_account)
        .position_token_account(accounts.position_token_account)
        .asset_token_program(spl_token::ID)
        .amount(amount)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

fn pull_position(
    svm: &mut LiteSVM,
    authority: &Keypair,
    accounts: &PositionAccounts,
    amount: u64,
) -> litesvm::types::TransactionResult {
    PullVenuePositionBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(accounts.asset_mint)
        .vault(accounts.vault)
        .vault_asset(accounts.vault_asset)
        .venue_entry(accounts.venue_entry)
        .vault_venue(accounts.vault_venue)
        .position(accounts.position)
        .vault_token_account(accounts.vault_token_account)
        .position_token_account(accounts.position_token_account)
        .asset_token_program(spl_token::ID)
        .amount(amount)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

fn read_venue_entry(svm: &LiteSVM, venue_entry: Pubkey) -> VenueEntry {
    let account = svm.get_account(&venue_entry).expect("venue should exist");
    VenueEntry::from_bytes(account.data()).unwrap()
}

fn read_vault_venue(svm: &LiteSVM, vault_venue: Pubkey) -> VaultVenue {
    let account = svm
        .get_account(&vault_venue)
        .expect("vault venue should exist");
    VaultVenue::from_bytes(account.data()).unwrap()
}

fn read_position(svm: &LiteSVM, position: Pubkey) -> Position {
    let account = svm.get_account(&position).expect("position should exist");
    Position::from_bytes(account.data()).unwrap()
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

fn add_and_fund_secondary_asset(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    mint_authority: &Keypair,
    depositor: &Keypair,
    share_mint: Pubkey,
    vault: Pubkey,
    amount: u64,
    deposit_cap: u64,
) -> SecondaryAssetAccounts {
    let asset_mint = Keypair::new();
    create_mint(svm, mint_authority, &asset_mint, &spl_token::ID);
    let user_asset_account = create_ata(svm, depositor, &asset_mint.pubkey(), &spl_token::ID);
    helper_mint_to(
        svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        mint_authority,
        amount,
        &spl_token::ID,
    );

    let (vault_asset, reserve, pending_vault) = derive_asset_accounts(vault, asset_mint.pubkey());
    add_vault_asset(
        svm,
        payer,
        authority,
        vault,
        asset_mint.pubkey(),
        deposit_cap,
    )
    .expect("add secondary asset should succeed");

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(depositor.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint)
        .vault(vault)
        .vault_asset(Some(vault_asset))
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount,
            operator: None,
        })
        .instruction()
        .send_transaction(svm, &depositor.pubkey(), &[depositor, &request_keypair])
        .expect("secondary deposit request should succeed");

    let (owner, request_type, request_amount, created_at, nav_update_version) =
        approve_request_args(svm, &request_keypair.pubkey());
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint)
        .vault(vault)
        .vault_asset(Some(vault_asset))
        .request(request_keypair.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(request_amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .vault_token_account(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(spl_token::ID)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("approve secondary deposit should succeed");

    SecondaryAssetAccounts {
        asset_mint: asset_mint.pubkey(),
        vault_asset,
        reserve,
    }
}

fn closed_or_empty(svm: &LiteSVM, account: Pubkey) -> bool {
    svm.get_account(&account)
        .map(|account| account.lamports() == 0)
        .unwrap_or(true)
}

#[test]
fn test_register_venue_initializes_entry() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let payer = Keypair::new();
    let registry_authority = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&registry_authority.pubkey(), 1_000_000_000)
        .unwrap();

    let venue_id = venue_id(7);
    let venue_entry_pubkey = derive_venue_entry(registry_authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &registry_authority, venue_id, 2)
        .expect("register venue should succeed");

    let venue_entry = read_venue_entry(&svm, venue_entry_pubkey);
    assert_eq!(venue_entry.registry_authority, registry_authority.pubkey());
    assert_eq!(venue_entry.venue_id, venue_id);
    assert_eq!(venue_entry.target_program, spl_token::ID);
    assert_eq!(venue_entry.allowed_discriminator_count, 2);
    assert_eq!(
        &venue_entry.allowed_discriminators[..16],
        &venue_discriminators()[..16]
    );
    assert!(venue_entry.allowed_discriminators[16..]
        .iter()
        .all(|b| *b == 0));
    assert_eq!(venue_entry.risk_class, 2);
    assert_eq!(venue_entry.venue_type, VenueType::ExternalProtocol);
    assert!(venue_entry.routine_safe);
    assert!(!venue_entry.paused);
}

#[test]
fn test_register_venue_rejects_invalid_discriminator_count() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let payer = Keypair::new();
    let registry_authority = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&registry_authority.pubkey(), 1_000_000_000)
        .unwrap();

    let err = register_venue(&mut svm, &payer, &registry_authority, venue_id(9), 0).unwrap_err();
    assert_error_code(
        &err,
        INVALID_VENUE_DISCRIMINATOR_COUNT,
        "InvalidVenueDiscriminatorCount",
    );
}

#[test]
fn test_set_venue_entry_paused_requires_registry_authority() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let payer = Keypair::new();
    let registry_authority = Keypair::new();
    let wrong_authority = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&registry_authority.pubkey(), 1_000_000_000)
        .unwrap();
    svm.airdrop(&wrong_authority.pubkey(), 1_000_000_000)
        .unwrap();

    let venue_id = venue_id(10);
    let venue_entry = derive_venue_entry(registry_authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &registry_authority, venue_id, 1)
        .expect("register venue should succeed");

    let err = SetVenueEntryPausedBuilder::new()
        .registry_authority(wrong_authority.pubkey())
        .venue_entry(venue_entry)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &wrong_authority.pubkey(), &[&wrong_authority])
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
}

#[test]
fn test_curator_can_approve_and_remove_vault_venue() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
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
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    let venue_id = venue_id(11);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");

    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");

    let vault_venue_pubkey = derive_vault_venue(vault_pubkey, venue_entry);
    let vault_venue = read_vault_venue(&svm, vault_venue_pubkey);
    assert_eq!(vault_venue.vault, vault_pubkey);
    assert_eq!(vault_venue.venue_entry, venue_entry);
    assert_eq!(vault_venue.target_program, spl_token::ID);
    assert!(vault_venue.routine_safe);
    assert!(!vault_venue.paused);
    assert_eq!(vault_venue.position_count, 0);

    RemoveVaultVenueBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("remove zero-position venue approval should succeed");
    assert!(closed_or_empty(&svm, vault_venue_pubkey));
}

#[test]
fn test_hot_manager_deploys_and_pulls_primary_position() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        hot_manager,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .hot_manager(hot_manager.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set hot manager should succeed");

    let venue_id = venue_id(15);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register routine-safe venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        None,
        venue_entry,
    )
    .expect("create position should succeed");

    let accounts = PositionAccounts {
        vault: vault_pubkey,
        asset_mint: asset_mint.pubkey(),
        vault_asset: None,
        venue_entry,
        vault_venue: derive_vault_venue(vault_pubkey, venue_entry),
        position: derive_position(vault_pubkey, venue_entry, asset_mint.pubkey()),
        vault_token_account: reserve_pubkey,
        position_token_account: derive_position_token(
            vault_pubkey,
            venue_entry,
            asset_mint.pubkey(),
        ),
    };
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000,
        &spl_token::ID,
    );

    deploy_position(&mut svm, &hot_manager, &accounts, 400)
        .expect("routine-safe hot manager deploy should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        600
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&accounts.position_token_account).unwrap()),
        400
    );
    assert_eq!(read_position(&svm, accounts.position).amount, 400);

    pull_position(&mut svm, &hot_manager, &accounts, 150)
        .expect("routine-safe hot manager pull should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        750
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&accounts.position_token_account).unwrap()),
        250
    );
    assert_eq!(read_position(&svm, accounts.position).amount, 250);

    pull_position(&mut svm, &authority, &accounts, 250).expect("manager pull should succeed");
    RemoveVenuePositionBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .venue_entry(venue_entry)
        .vault_venue(accounts.vault_venue)
        .position(accounts.position)
        .position_token_account(accounts.position_token_account)
        .asset_token_program(spl_token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("remove zero-balance position should succeed");
    assert!(closed_or_empty(&svm, accounts.position));
    assert!(closed_or_empty(&svm, accounts.position_token_account));

    RemoveVaultVenueBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(accounts.vault_venue)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("remove vault venue after position removal should succeed");
}

#[test]
fn test_manager_deploys_and_pulls_secondary_position_updates_asset_ledger() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        _primary_asset_mint,
        share_mint,
        hot_manager,
        _operator,
        _fee_recipient,
        _primary_reserve_pubkey,
        vault_pubkey,
        _primary_pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .hot_manager(hot_manager.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set hot manager should succeed");

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &spl_token::ID);
    let user_asset_account =
        create_ata(&mut svm, &hot_manager, &asset_mint.pubkey(), &spl_token::ID);
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &user_asset_account,
        &mint_authority,
        1_000,
        &spl_token::ID,
    );

    let (vault_asset_pubkey, reserve_pubkey, pending_vault_pubkey) =
        derive_asset_accounts(vault_pubkey, asset_mint.pubkey());
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        1_000,
    )
    .expect("add secondary asset should succeed");

    let request_keypair = Keypair::new();
    CreateDepositRequestBuilder::new()
        .user(hot_manager.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .request(request_keypair.pubkey())
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(spl_token::ID)
        .args(RequestArgs {
            amount: 1_000,
            operator: None,
        })
        .instruction()
        .send_transaction(
            &mut svm,
            &hot_manager.pubkey(),
            &[&hot_manager, &request_keypair],
        )
        .expect("secondary deposit request should succeed");

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
        .asset_token_program(spl_token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("approve secondary deposit should succeed");

    let funded_asset = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(funded_asset.idle_balance, 1_000);
    assert_eq!(funded_asset.deployed_balance, 0);

    let venue_id = venue_id(19);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register routine-safe venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        Some(vault_asset_pubkey),
        venue_entry,
    )
    .expect("create secondary position should succeed");

    let accounts = PositionAccounts {
        vault: vault_pubkey,
        asset_mint: asset_mint.pubkey(),
        vault_asset: Some(vault_asset_pubkey),
        venue_entry,
        vault_venue: derive_vault_venue(vault_pubkey, venue_entry),
        position: derive_position(vault_pubkey, venue_entry, asset_mint.pubkey()),
        vault_token_account: reserve_pubkey,
        position_token_account: derive_position_token(
            vault_pubkey,
            venue_entry,
            asset_mint.pubkey(),
        ),
    };

    deploy_position(&mut svm, &hot_manager, &accounts, 400)
        .expect("routine-safe hot manager secondary deploy should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        600
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&accounts.position_token_account).unwrap()),
        400
    );
    assert_eq!(read_position(&svm, accounts.position).amount, 400);
    let deployed_asset = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(deployed_asset.idle_balance, 600);
    assert_eq!(deployed_asset.deployed_balance, 400);

    pull_position(&mut svm, &hot_manager, &accounts, 150)
        .expect("routine-safe hot manager secondary pull should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap()),
        750
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&accounts.position_token_account).unwrap()),
        250
    );
    let pulled_asset = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(pulled_asset.idle_balance, 750);
    assert_eq!(pulled_asset.deployed_balance, 250);

    pull_position(&mut svm, &authority, &accounts, 250)
        .expect("manager secondary pull should succeed");
    let final_asset = read_vault_asset(&svm, vault_asset_pubkey);
    assert_eq!(final_asset.idle_balance, 1_000);
    assert_eq!(final_asset.deployed_balance, 0);

    RemoveVenuePositionBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(Some(vault_asset_pubkey))
        .venue_entry(venue_entry)
        .vault_venue(accounts.vault_venue)
        .position(accounts.position)
        .position_token_account(accounts.position_token_account)
        .asset_token_program(spl_token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("remove zero-balance secondary position should succeed");
    assert!(closed_or_empty(&svm, accounts.position));
    assert!(closed_or_empty(&svm, accounts.position_token_account));
}

#[test]
fn test_secondary_position_requires_vault_asset_account() {
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
        _primary_reserve_pubkey,
        vault_pubkey,
        _primary_pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);

    let asset_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &asset_mint, &spl_token::ID);
    add_vault_asset(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        1_000,
    )
    .expect("add secondary asset should succeed");

    let venue_id = venue_id(20);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");

    let err = create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        None,
        venue_entry,
    )
    .unwrap_err();
    assert_error_code(&err, INVALID_ASSET_MINT, "InvalidAssetMint");
}

#[test]
fn test_secondary_manager_rolling_buckets_are_per_asset() {
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
        _primary_reserve_pubkey,
        vault_pubkey,
        _primary_pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .rolling_limit_window_slots(10)
        .manager_rolling_limit(600)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set manager rolling limit should succeed");

    let venue_id = venue_id(21);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");

    let asset_a = add_and_fund_secondary_asset(
        &mut svm,
        &payer,
        &authority,
        &mint_authority,
        &user,
        share_mint.pubkey(),
        vault_pubkey,
        1_000,
        1_000,
    );
    let asset_b = add_and_fund_secondary_asset(
        &mut svm,
        &payer,
        &authority,
        &mint_authority,
        &user,
        share_mint.pubkey(),
        vault_pubkey,
        1_000,
        1_000,
    );

    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_a.asset_mint,
        Some(asset_a.vault_asset),
        venue_entry,
    )
    .expect("create first secondary position should succeed");
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_b.asset_mint,
        Some(asset_b.vault_asset),
        venue_entry,
    )
    .expect("create second secondary position should succeed");

    let accounts_a = position_accounts(
        vault_pubkey,
        asset_a.asset_mint,
        Some(asset_a.vault_asset),
        venue_entry,
        asset_a.reserve,
    );
    let accounts_b = position_accounts(
        vault_pubkey,
        asset_b.asset_mint,
        Some(asset_b.vault_asset),
        venue_entry,
        asset_b.reserve,
    );

    deploy_position(&mut svm, &authority, &accounts_a, 600)
        .expect("first secondary deploy should consume first asset bucket");
    svm.expire_blockhash();
    deploy_position(&mut svm, &authority, &accounts_b, 600)
        .expect("second secondary deploy should consume independent asset bucket");

    let vault = read_vault(&svm, vault_pubkey);
    assert_eq!(vault.manager_window_amount, 0);

    let state_a = read_vault_asset(&svm, asset_a.vault_asset);
    assert_eq!(state_a.idle_balance, 400);
    assert_eq!(state_a.deployed_balance, 600);
    assert_eq!(state_a.manager_window_amount, 600);

    let state_b = read_vault_asset(&svm, asset_b.vault_asset);
    assert_eq!(state_b.idle_balance, 400);
    assert_eq!(state_b.deployed_balance, 600);
    assert_eq!(state_b.manager_window_amount, 600);
}

#[test]
fn test_secondary_manager_rolling_bucket_rejects_same_asset_over_limit_and_resets() {
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
        _primary_reserve_pubkey,
        vault_pubkey,
        _primary_pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    initialize_and_set_nav(&mut svm, &authority, share_mint.pubkey(), vault_pubkey);
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .rolling_limit_window_slots(10)
        .manager_rolling_limit(600)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set manager rolling limit should succeed");

    let venue_id = venue_id(22);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");

    let asset = add_and_fund_secondary_asset(
        &mut svm,
        &payer,
        &authority,
        &mint_authority,
        &user,
        share_mint.pubkey(),
        vault_pubkey,
        1_000,
        1_000,
    );
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset.asset_mint,
        Some(asset.vault_asset),
        venue_entry,
    )
    .expect("create secondary position should succeed");

    let accounts = position_accounts(
        vault_pubkey,
        asset.asset_mint,
        Some(asset.vault_asset),
        venue_entry,
        asset.reserve,
    );

    deploy_position(&mut svm, &authority, &accounts, 400)
        .expect("secondary deploy inside rolling limit should succeed");
    svm.expire_blockhash();

    let err = pull_position(&mut svm, &authority, &accounts, 250).unwrap_err();
    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");
    let state_after_reject = read_vault_asset(&svm, asset.vault_asset);
    assert_eq!(state_after_reject.manager_window_amount, 400);
    assert_eq!(state_after_reject.idle_balance, 600);
    assert_eq!(state_after_reject.deployed_balance, 400);

    let mut clock = svm.get_sysvar::<solana_sdk::clock::Clock>();
    clock.slot = clock.slot.saturating_add(10);
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    pull_position(&mut svm, &authority, &accounts, 250)
        .expect("secondary pull should succeed after asset bucket resets");
    let state_after_reset = read_vault_asset(&svm, asset.vault_asset);
    assert_eq!(state_after_reset.manager_window_amount, 250);
    assert_eq!(state_after_reset.idle_balance, 850);
    assert_eq!(state_after_reset.deployed_balance, 150);
}

#[test]
fn test_hot_manager_rejected_for_non_routine_safe_position() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        hot_manager,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .hot_manager(hot_manager.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set hot manager should succeed");

    let venue_id = venue_id(16);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue_with_routine_safe(&mut svm, &payer, &authority, venue_id, 1, false)
        .expect("register non-routine venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        None,
        venue_entry,
    )
    .expect("create position should succeed");
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000,
        &spl_token::ID,
    );

    let accounts = PositionAccounts {
        vault: vault_pubkey,
        asset_mint: asset_mint.pubkey(),
        vault_asset: None,
        venue_entry,
        vault_venue: derive_vault_venue(vault_pubkey, venue_entry),
        position: derive_position(vault_pubkey, venue_entry, asset_mint.pubkey()),
        vault_token_account: reserve_pubkey,
        position_token_account: derive_position_token(
            vault_pubkey,
            venue_entry,
            asset_mint.pubkey(),
        ),
    };
    let err = deploy_position(&mut svm, &hot_manager, &accounts, 100).unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
}

#[test]
fn test_manager_deploy_pull_respects_rolling_limit() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        _user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .rolling_limit_window_slots(10)
        .manager_rolling_limit(600)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set manager rolling limit should succeed");

    let venue_id = venue_id(18);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        None,
        venue_entry,
    )
    .expect("create position should succeed");
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000,
        &spl_token::ID,
    );

    let accounts = PositionAccounts {
        vault: vault_pubkey,
        asset_mint: asset_mint.pubkey(),
        vault_asset: None,
        venue_entry,
        vault_venue: derive_vault_venue(vault_pubkey, venue_entry),
        position: derive_position(vault_pubkey, venue_entry, asset_mint.pubkey()),
        vault_token_account: reserve_pubkey,
        position_token_account: derive_position_token(
            vault_pubkey,
            venue_entry,
            asset_mint.pubkey(),
        ),
    };

    deploy_position(&mut svm, &authority, &accounts, 400)
        .expect("manager deploy inside rolling limit should succeed");
    svm.expire_blockhash();

    let err = pull_position(&mut svm, &authority, &accounts, 250).unwrap_err();
    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");

    let mut clock = svm.get_sysvar::<solana_sdk::clock::Clock>();
    clock.slot = clock.slot.saturating_add(10);
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    pull_position(&mut svm, &authority, &accounts, 250)
        .expect("manager pull should succeed after rolling window resets");
}

#[test]
fn test_remove_venue_position_rejects_nonzero_balance() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        _user,
        _operator,
        _fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    let venue_id = venue_id(17);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");
    approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry)
        .expect("approve vault venue should succeed");
    create_venue_position(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        asset_mint.pubkey(),
        None,
        venue_entry,
    )
    .expect("create position should succeed");
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000,
        &spl_token::ID,
    );

    let accounts = PositionAccounts {
        vault: vault_pubkey,
        asset_mint: asset_mint.pubkey(),
        vault_asset: None,
        venue_entry,
        vault_venue: derive_vault_venue(vault_pubkey, venue_entry),
        position: derive_position(vault_pubkey, venue_entry, asset_mint.pubkey()),
        vault_token_account: reserve_pubkey,
        position_token_account: derive_position_token(
            vault_pubkey,
            venue_entry,
            asset_mint.pubkey(),
        ),
    };
    deploy_position(&mut svm, &authority, &accounts, 100).expect("manager deploy should succeed");

    let err = RemoveVenuePositionBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .venue_entry(venue_entry)
        .vault_venue(accounts.vault_venue)
        .position(accounts.position)
        .position_token_account(accounts.position_token_account)
        .asset_token_program(spl_token::ID)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, POSITION_BALANCE_NON_ZERO, "PositionBalanceNonZero");
}

#[test]
fn test_approve_vault_venue_rejects_non_curator() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
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
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    let venue_id = venue_id(12);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");

    let err = approve_vault_venue(&mut svm, &payer, &user, vault_pubkey, venue_entry).unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
}

#[test]
fn test_approve_vault_venue_rejects_paused_venue() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
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
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    let venue_id = venue_id(13);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");
    SetVenueEntryPausedBuilder::new()
        .registry_authority(authority.pubkey())
        .venue_entry(venue_entry)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("pause venue should succeed");

    let err =
        approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry).unwrap_err();
    assert_error_code(&err, VENUE_PAUSED, "VenuePaused");
}

#[test]
fn test_approve_vault_venue_requires_timelock_queue_when_enabled() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);
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
    ) = set_up_async_vault_v2(&mut svm, spl_token::ID, None, spl_token::ID, 0);

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(1)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set timelock should succeed");

    let venue_id = venue_id(14);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    register_venue(&mut svm, &payer, &authority, venue_id, 1)
        .expect("register venue should succeed");

    let err =
        approve_vault_venue(&mut svm, &payer, &authority, vault_pubkey, venue_entry).unwrap_err();
    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");
}
