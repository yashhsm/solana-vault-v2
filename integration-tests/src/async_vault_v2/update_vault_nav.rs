use anchor_spl::token;
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, NavMode, UpdateVaultBuilder, UpdateVaultNavBuilder,
    Vault,
};
use borsh::BorshSerialize;
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, clock::Clock, instruction::AccountMeta, signature::Keypair,
    signer::Signer,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{
        assert_error_code, create_ata, get_mint_supply, get_token_account_amount,
        initialize_and_activate_protocol_fee_config, set_share_balance, set_up_async_vault_v2,
    },
    async_vault_v2::constants::{
        ARITHMETIC_ERROR, INVALID_ROLLING_LIMIT_CONFIG, MISSING_REQUIRED_ACCOUNT, NAV_APY_EXCEEDED,
        NAV_DELTA_EXCEEDED, UNAUTHORIZED_SIGNER, UNSUPPORTED_PHASE_CONFIG,
    },
};

#[test_case(200 ; "update nav succeeds")]
#[test_case(0 ; "update nav to zero succeeds")]
#[test_case(u128::MAX ; "update nav to max succeeds")]

fn test_update_vault_nav(updated_nav: u128) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let vault_before = Vault::from_bytes(
        svm.get_account(&vault_pubkey)
            .expect("vault should exist")
            .data(),
    )
    .unwrap();
    let nav_version_before = vault_before.nav_version;

    let result = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(updated_nav)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority]);

    result.expect("update_vault_nav should succeed");

    let vault_account = svm.get_account(&vault_pubkey).expect("vault should exist");
    let vault_config = Vault::from_bytes(vault_account.data()).unwrap();

    assert_eq!(vault_config.nav, updated_nav);
    assert_eq!(vault_config.nav_version, nav_version_before + 1);
}

#[test]
fn test_update_vault_nav_initializes_high_water_mark_without_fee_accounts() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .performance_fee_bps(2_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set performance fee should succeed");

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("first nav should initialize HWM without fee accounts");

    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.high_water_mark, 1_000_000_000);
    assert_eq!(vault.performance_fee_bps, 2_000);
}

#[test]
fn test_update_vault_nav_performance_fee_requires_remaining_accounts_when_due() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should succeed");

    svm.expire_blockhash();
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .performance_fee_bps(2_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set performance fee should succeed");

    svm.expire_blockhash();
    let err = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_500_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, MISSING_REQUIRED_ACCOUNT, "MissingRequiredAccount");
}

#[test]
fn test_update_vault_nav_mints_performance_fee_shares_on_new_high_water_mark() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _operator,
        fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let fee_recipient_share_account =
        create_ata(&mut svm, &fee_recipient, &share_mint.pubkey(), &token::ID);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should succeed");

    svm.expire_blockhash();
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .performance_fee_bps(2_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set performance fee should succeed");

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_500_000_000)
        .add_remaining_accounts(&[
            AccountMeta::new(share_mint.pubkey(), false),
            AccountMeta::new(fee_recipient_share_account, false),
            AccountMeta::new_readonly(token::ID, false),
        ])
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("performance fee crystallization should succeed");

    let expected_fee_shares = 71_428_571;
    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_share_account).unwrap()),
        expected_fee_shares
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        1_000_000_000 + expected_fee_shares
    );
    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.high_water_mark, 1_500_000_000);
    assert_eq!(vault.nav, 1_500_000_000);
}

#[test]
fn test_update_vault_nav_protocol_fee_splits_performance_fee_shares() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _operator,
        fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let protocol_fee_recipient = Keypair::new();
    svm.airdrop(&protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let fee_recipient_share_account =
        create_ata(&mut svm, &fee_recipient, &share_mint.pubkey(), &token::ID);
    let protocol_fee_recipient_share_account = create_ata(
        &mut svm,
        &protocol_fee_recipient,
        &share_mint.pubkey(),
        &token::ID,
    );
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should succeed");

    svm.expire_blockhash();
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .performance_fee_bps(2_000)
        .protocol_fee_bps(2_500)
        .protocol_fee_recipient(protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set performance and protocol fee should succeed");

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_500_000_000)
        .add_remaining_accounts(&[
            AccountMeta::new(share_mint.pubkey(), false),
            AccountMeta::new(fee_recipient_share_account, false),
            AccountMeta::new(protocol_fee_recipient_share_account, false),
            AccountMeta::new_readonly(token::ID, false),
        ])
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("performance fee crystallization should split protocol shares");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_share_account).unwrap()),
        53_571_428
    );
    assert_eq!(
        get_token_account_amount(
            &svm.get_account(&protocol_fee_recipient_share_account)
                .unwrap()
        ),
        17_857_143
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        1_071_428_571
    );
}

#[test]
fn test_update_vault_nav_protocol_fee_uses_program_config_recipient_when_supplied() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _operator,
        fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let vault_protocol_fee_recipient = Keypair::new();
    let configured_protocol_fee_recipient = Keypair::new();
    svm.airdrop(&vault_protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    svm.airdrop(&configured_protocol_fee_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let fee_recipient_share_account =
        create_ata(&mut svm, &fee_recipient, &share_mint.pubkey(), &token::ID);
    let vault_protocol_fee_share_account = create_ata(
        &mut svm,
        &vault_protocol_fee_recipient,
        &share_mint.pubkey(),
        &token::ID,
    );
    let configured_protocol_fee_share_account = create_ata(
        &mut svm,
        &configured_protocol_fee_recipient,
        &share_mint.pubkey(),
        &token::ID,
    );
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should succeed");

    svm.expire_blockhash();
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .performance_fee_bps(2_000)
        .protocol_fee_bps(2_500)
        .protocol_fee_recipient(vault_protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set performance and protocol fee should succeed");

    let protocol_fee_config = initialize_and_activate_protocol_fee_config(
        &mut svm,
        &authority,
        configured_protocol_fee_recipient.pubkey(),
    );

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_500_000_000)
        .add_remaining_accounts(&[
            AccountMeta::new(share_mint.pubkey(), false),
            AccountMeta::new(fee_recipient_share_account, false),
            AccountMeta::new_readonly(protocol_fee_config, false),
            AccountMeta::new(configured_protocol_fee_share_account, false),
            AccountMeta::new_readonly(token::ID, false),
        ])
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("performance fee crystallization should use program-level protocol recipient");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_share_account).unwrap()),
        53_571_428
    );
    assert_eq!(
        get_token_account_amount(
            &svm.get_account(&configured_protocol_fee_share_account)
                .unwrap()
        ),
        17_857_143
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&vault_protocol_fee_share_account).unwrap()),
        0
    );
}

#[test]
fn test_update_vault_nav_respects_performance_fee_crystallization_interval() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _operator,
        fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let fee_recipient_share_account =
        create_ata(&mut svm, &fee_recipient, &share_mint.pubkey(), &token::ID);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );

    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 10;
    svm.set_sysvar(&clock);
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should initialize HWM");

    svm.expire_blockhash();
    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .performance_fee_bps(2_000)
        .performance_fee_crystallization_interval_seconds(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set performance fee interval should succeed");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 20;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_500_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("early nav increase should skip fee crystallization");

    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.nav, 1_500_000_000);
    assert_eq!(vault.high_water_mark, 1_000_000_000);
    assert_eq!(vault.last_fee_crystallization_timestamp, 10);
    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_share_account).unwrap()),
        0
    );

    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 111;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_600_000_000)
        .add_remaining_accounts(&[
            AccountMeta::new(share_mint.pubkey(), false),
            AccountMeta::new(fee_recipient_share_account, false),
            AccountMeta::new_readonly(token::ID, false),
        ])
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("post-interval nav increase should crystallize fee");

    let expected_fee_shares = 81_081_081;
    assert_eq!(
        get_token_account_amount(&svm.get_account(&fee_recipient_share_account).unwrap()),
        expected_fee_shares
    );
    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.high_water_mark, 1_600_000_000);
    assert_eq!(vault.last_fee_crystallization_timestamp, 111);
}

#[test]
fn test_update_vault_nav_unauthorized_signer_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        _authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();

    let result = UpdateVaultNavBuilder::new()
        .authority(unauthorized.pubkey())
        .vault(vault_pubkey)
        .updated_nav(200)
        .instruction()
        .send_transaction(&mut svm, &unauthorized.pubkey(), &[&unauthorized]);

    assert_error_code(
        &result.unwrap_err(),
        UNAUTHORIZED_SIGNER,
        "UnauthorizedSigner",
    );
}

#[test]
fn test_update_vault_nav_version_overflow_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let mut account = svm.get_account(&vault_pubkey).expect("vault should exist");
    let mut vault = Vault::from_bytes(account.data()).unwrap();
    vault.nav_version = u64::MAX;
    let mut buf = Vec::new();
    vault.serialize(&mut buf).unwrap();
    let tlv_bytes = account.data()[buf.len()..].to_vec();
    buf.extend_from_slice(&tlv_bytes);
    account.data = buf;
    svm.set_account(vault_pubkey, account).unwrap();

    let result = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(200)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority]);

    assert_error_code(&result.unwrap_err(), ARITHMETIC_ERROR, "ArithmeticError");
}

#[test]
fn test_update_vault_nav_delta_bound_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(10_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should succeed");

    svm.expire_blockhash();

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .max_nav_delta_bps(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set delta bound should succeed");

    svm.expire_blockhash();

    let err = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(20_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, NAV_DELTA_EXCEEDED, "NavDeltaExceeded");
}

#[test]
fn test_update_vault_nav_implied_apy_bound_fails() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 1;
    svm.set_sysvar(&clock);

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(10_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial nav should succeed");

    svm.expire_blockhash();

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .max_implied_apy_bps(100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("set APY bound should succeed");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot += 1;
    clock.unix_timestamp = 2;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    let err = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(10_100)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, NAV_APY_EXCEEDED, "NavApyExceeded");
}

#[test]
fn test_update_vault_rejects_unsupported_nav_modes() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let err = UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .nav_mode(NavMode::Oracle)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, UNSUPPORTED_PHASE_CONFIG, "UnsupportedPhaseConfig");
}

#[test]
fn test_update_vault_accepts_protocol_fee_fields() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
        _mint_authority,
        _asset_mint,
        share_mint,
        _user,
        _operator,
        fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .protocol_fee_bps(250)
        .protocol_fee_recipient(fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("protocol fee config should succeed");

    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.protocol_fee_bps, 250);
    assert_eq!(vault.protocol_fee_recipient, fee_recipient.pubkey());
}

#[test]
fn test_update_vault_rejects_rolling_limit_without_window() {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let err = UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .external_withdraw_rolling_limit(1)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(
        &err,
        INVALID_ROLLING_LIMIT_CONFIG,
        "InvalidRollingLimitConfig",
    );

    let err = UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .manager_rolling_limit(1)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(
        &err,
        INVALID_ROLLING_LIMIT_CONFIG,
        "InvalidRollingLimitConfig",
    );
}
