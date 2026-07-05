use anchor_spl::token;
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, ApproveVaultVenueBuilder,
    InitializeExternallyManagedWithdrawalsBuilder,
    InitializeVaultBuilder as InitializeAsyncVaultBuilder, RegisterVenueBuilder,
    SetVenueEntryPausedBuilder, UpdateVaultBuilder as UpdateVaultAsyncBuilder,
    UpdateVaultNavBuilder, VenueType, WithdrawAssetsBuilder,
};
use litesvm::LiteSVM;
use solana_sdk::{clock::Clock, pubkey::Pubkey, signature::Keypair, signer::Signer};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        assert_error_code, create_ata, get_token_account_amount, helper_mint_to,
        set_up_async_vault_v2,
    },
    async_vault_v2::constants::{
        EXTERNALLY_MANAGED_WITHDRAWALS_DISABLED, INVALID_VENUE_RECIPIENT, PAUSED_VAULT,
        ROLLING_LIMIT_EXCEEDED, UNAUTHORIZED_SIGNER, VENUE_PAUSED,
    },
};

const ANCHOR_CONSTRAINT_SEEDS: u32 = 2006;
const VENUE_ENTRY_SEED: &[u8] = b"venue";
const VAULT_VENUE_SEED: &[u8] = b"vault_venue";

fn venue_id(byte: u8) -> [u8; 32] {
    [byte; 32]
}

fn venue_discriminators() -> [u8; 64] {
    let mut discriminators = [0_u8; 64];
    discriminators[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
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

fn approve_withdraw_venue(
    svm: &mut LiteSVM,
    payer: &Keypair,
    authority: &Keypair,
    vault: Pubkey,
    recipient_authority: Pubkey,
    byte: u8,
) -> (Pubkey, Pubkey) {
    let venue_id = venue_id(byte);
    let venue_entry = derive_venue_entry(authority.pubkey(), venue_id);
    RegisterVenueBuilder::new()
        .payer(payer.pubkey())
        .registry_authority(authority.pubkey())
        .venue_entry(venue_entry)
        .venue_id(venue_id)
        .target_program(token::ID)
        .allowed_discriminator_count(1)
        .allowed_discriminators(venue_discriminators())
        .risk_class(1)
        .venue_type(VenueType::ExternalProtocol)
        .routine_safe(false)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
        .expect("register withdraw venue should succeed");

    let vault_venue = derive_vault_venue(vault, venue_entry);
    ApproveVaultVenueBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .recipient_authority(recipient_authority)
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[payer, authority])
        .expect("approve withdraw venue should succeed");

    (venue_entry, vault_venue)
}

fn initialize_externally_managed_withdrawals(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault: Pubkey,
) {
    InitializeExternallyManagedWithdrawalsBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize externally managed withdrawals should succeed");
}

#[test]
fn test_withdraw_assets_disabled_without_extension() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 10_000_000);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    let (venue_entry, vault_venue) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        1,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000_000,
        &token::ID,
    );

    let recipient_ata = create_ata(&mut svm, &authority, &asset_mint.pubkey(), &token::ID);

    let err = WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(500_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(
        &err,
        EXTERNALLY_MANAGED_WITHDRAWALS_DISABLED,
        "ExternallyManagedWithdrawalsDisabled",
    );
}

#[test_case(1_000_000, 500_000 ; "withdraw partial amount")]
#[test_case(1_000_000, 1_000_000 ; "withdraw full amount")]
#[test_case(1_000_000, 1 ; "withdraw minimum amount")]
fn test_withdraw_assets_success(deposit_amount: u64, withdraw_amount: u64) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let user_amount = 10_000_000;
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, user_amount);

    initialize_externally_managed_withdrawals(&mut svm, &authority, vault_pubkey);

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
    let (venue_entry, vault_venue) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        2,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        deposit_amount,
        &token::ID,
    );

    let recipient_ata = create_ata(&mut svm, &authority, &asset_mint.pubkey(), &token::ID);

    let reserve_before = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    assert_eq!(reserve_before, deposit_amount);

    WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(withdraw_amount)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("withdraw assets should succeed");

    let reserve_after = get_token_account_amount(&svm.get_account(&reserve_pubkey).unwrap());
    assert_eq!(reserve_after, deposit_amount - withdraw_amount);

    let recipient_balance = get_token_account_amount(&svm.get_account(&recipient_ata).unwrap());
    assert_eq!(recipient_balance, withdraw_amount);
}

#[test]
fn test_withdraw_assets_rejects_unapproved_recipient_authority() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 10_000_000);

    initialize_externally_managed_withdrawals(&mut svm, &authority, vault_pubkey);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    let approved_recipient = Keypair::new();
    let (venue_entry, vault_venue) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        approved_recipient.pubkey(),
        8,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000_000,
        &token::ID,
    );

    let unapproved_recipient = Keypair::new();
    svm.airdrop(&unapproved_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let unapproved_recipient_ata = create_ata(
        &mut svm,
        &unapproved_recipient,
        &asset_mint.pubkey(),
        &token::ID,
    );

    let err = WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(unapproved_recipient_ata)
        .asset_token_program(token::ID)
        .amount(500_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, INVALID_VENUE_RECIPIENT, "InvalidVenueRecipient");
}

#[test_case(true, false ; "unauthorized signer")]
#[test_case(false, true ; "paused vault")]
fn test_withdraw_assets_fails(use_wrong_signer: bool, pause_vault: bool) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

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
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 10_000_000);

    initialize_externally_managed_withdrawals(&mut svm, &authority, vault_pubkey);

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
    let (venue_entry, vault_venue) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        3,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000_000,
        &token::ID,
    );

    let recipient_ata = create_ata(&mut svm, &authority, &asset_mint.pubkey(), &token::ID);

    if pause_vault {
        UpdateVaultAsyncBuilder::new()
            .authority(authority.pubkey())
            .share_mint(share_mint.pubkey())
            .paused(true)
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("pause vault should succeed");
    }

    let signer = if use_wrong_signer { &user } else { &authority };

    let expected_error_code = if use_wrong_signer {
        UNAUTHORIZED_SIGNER
    } else {
        PAUSED_VAULT
    };

    let err = WithdrawAssetsBuilder::new()
        .authority(signer.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(500_000)
        .instruction()
        .send_transaction(&mut svm, &signer.pubkey(), &[signer])
        .unwrap_err();

    assert_error_code(&err, expected_error_code, "");
}

#[test]
fn test_withdraw_assets_rejects_paused_venue_entry() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 10_000_000);

    initialize_externally_managed_withdrawals(&mut svm, &authority, vault_pubkey);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");
    let (venue_entry, vault_venue) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        5,
    );

    SetVenueEntryPausedBuilder::new()
        .registry_authority(authority.pubkey())
        .venue_entry(venue_entry)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("pause venue entry should succeed");

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000_000,
        &token::ID,
    );

    let recipient_ata = create_ata(&mut svm, &authority, &asset_mint.pubkey(), &token::ID);

    let err = WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(500_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, VENUE_PAUSED, "VenuePaused");
}

#[test]
fn test_withdraw_assets_rejects_mismatched_vault_venue() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 10_000_000);

    initialize_externally_managed_withdrawals(&mut svm, &authority, vault_pubkey);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    let (venue_entry_a, vault_venue_a) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        6,
    );
    let (venue_entry_b, _vault_venue_b) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        7,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000_000,
        &token::ID,
    );

    let recipient_ata = create_ata(&mut svm, &authority, &asset_mint.pubkey(), &token::ID);

    let err = WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry_b)
        .vault_venue(vault_venue_a)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(500_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_ne!(venue_entry_a, venue_entry_b);
    assert_error_code(&err, ANCHOR_CONSTRAINT_SEEDS, "ConstraintSeeds");
}

#[test]
fn test_withdraw_assets_respects_external_withdraw_rolling_limit() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 10_000_000);

    initialize_externally_managed_withdrawals(&mut svm, &authority, vault_pubkey);

    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .rolling_limit_window_slots(10)
        .external_withdraw_rolling_limit(600)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set external withdraw rolling limit should succeed");
    let (venue_entry, vault_venue) = approve_withdraw_venue(
        &mut svm,
        &payer,
        &authority,
        vault_pubkey,
        authority.pubkey(),
        4,
    );

    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        1_000,
        &token::ID,
    );

    let recipient_ata = create_ata(&mut svm, &authority, &asset_mint.pubkey(), &token::ID);

    WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(500)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("first withdrawal inside rolling limit should succeed");

    svm.expire_blockhash();

    let err = WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(200)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot += 10;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    WithdrawAssetsBuilder::new()
        .authority(authority.pubkey())
        .asset_mint(asset_mint.pubkey())
        .vault(vault_pubkey)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .vault_token_account(reserve_pubkey)
        .recipient_token_account(recipient_ata)
        .asset_token_program(token::ID)
        .amount(200)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("withdrawal should succeed after rolling window resets");
}
