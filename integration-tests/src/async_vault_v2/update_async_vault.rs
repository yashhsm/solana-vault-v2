use anchor_spl::token;
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, AcceptAuthorityInvitationBuilder,
    CancelVaultUpdateBuilder, ExecuteVaultUpdateBuilder, InviteNewAuthorityBuilder,
    PauseVaultBuilder, PendingVaultUpdate, QueueVaultUpdateBuilder,
    UpdateVaultArgs as GeneratedUpdateVaultArgs, UpdateVaultBuilder as UpdateVaultAsyncBuilder,
    Vault,
};
use litesvm::LiteSVM;
use solana_sdk::{account::ReadableAccount, clock::Clock, signature::Keypair, signer::Signer};
use test_case::test_case;

use crate::{
    async_helper_functions::{assert_error_code, set_up_async_vault_v2},
    async_vault_v2::constants::{
        INVALID_TIMELOCK_CHANGE, STALE_TIMELOCK_AUTHORITY, TIMELOCK_NOT_CONFIGURED,
        TIMELOCK_NOT_READY, TIMELOCK_REQUIRED, UNAUTHORIZED_SIGNER,
    },
};

fn empty_update_args() -> GeneratedUpdateVaultArgs {
    GeneratedUpdateVaultArgs {
        paused: None,
        fee_recipient: None,
        manager: None,
        hot_manager: None,
        fulfiller: None,
        breaker: None,
        nav_mode: None,
        require_fresh_nav: None,
        max_nav_delta_bps: None,
        max_implied_apy_bps: None,
        max_nav_staleness_slots: None,
        deposit_cap: None,
        rolling_limit_window_slots: None,
        manager_rolling_limit: None,
        external_withdraw_rolling_limit: None,
        redemption_rolling_limit: None,
        timelock_delay_slots: None,
        performance_fee_bps: None,
        performance_fee_crystallization_interval_seconds: None,
        protocol_fee_bps: None,
        protocol_fee_recipient: None,
        instant_redemption_fee_bps: None,
    }
}

#[test_case(Some(true), false; "pause vault")]
#[test_case(Some(false), false; "unpause vault")]
#[test_case(None, true; "update fee_recipient only")]
fn test_update_async_vault_v2(paused: Option<bool>, update_fee_recipient: bool) {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let vault_account = svm.get_account(&vault_pubkey).unwrap();
    let vault_before = Vault::from_bytes(vault_account.data()).unwrap();

    let new_fee_recipient = Keypair::new();

    let mut builder = UpdateVaultAsyncBuilder::new();
    builder
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey);
    if let Some(p) = paused {
        builder.paused(p);
    }
    if update_fee_recipient {
        builder.fee_recipient(new_fee_recipient.pubkey());
    }

    builder
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update vault should succeed");

    let vault_account = svm.get_account(&vault_pubkey).unwrap();
    let vault_after = Vault::from_bytes(vault_account.data()).unwrap();

    if let Some(p) = paused {
        assert_eq!(vault_after.paused, p);
    } else {
        assert_eq!(vault_after.paused, vault_before.paused);
    }

    if update_fee_recipient {
        assert_eq!(vault_after.fee_recipient, new_fee_recipient.pubkey());
    } else {
        assert_eq!(vault_after.fee_recipient, vault_before.fee_recipient);
    }

    assert_eq!(vault_after.authority, vault_before.authority);
    assert_eq!(vault_after.asset_mint, vault_before.asset_mint);
    assert_eq!(vault_after.share_mint, vault_before.share_mint);
    assert_eq!(vault_after.nav, vault_before.nav);
    assert_eq!(vault_after.nav_version, vault_before.nav_version);
}

#[test]
fn test_update_async_vault_v2_unauthorized_signer_fails() {
    let mut svm = LiteSVM::new();

    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
    let (
        _authority,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();

    let result = UpdateVaultAsyncBuilder::new()
        .authority(unauthorized.pubkey())
        .share_mint(share_mint.pubkey())
        .paused(true)
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &unauthorized.pubkey(), &[&unauthorized]);

    assert_error_code(
        &result.unwrap_err(),
        UNAUTHORIZED_SIGNER,
        "UnauthorizedSigner",
    );
}

#[test]
fn test_breaker_can_pause_but_not_unpause() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let breaker = Keypair::new();
    svm.airdrop(&breaker.pubkey(), 1_000_000_000).unwrap();

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .breaker(breaker.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("curator should set breaker");

    svm.expire_blockhash();

    PauseVaultBuilder::new()
        .breaker(breaker.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &breaker.pubkey(), &[&breaker])
        .expect("breaker should pause");

    let vault_after_pause =
        Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert!(vault_after_pause.paused);

    svm.expire_blockhash();

    let unpause_err = UpdateVaultAsyncBuilder::new()
        .authority(breaker.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .paused(false)
        .instruction()
        .send_transaction(&mut svm, &breaker.pubkey(), &[&breaker])
        .unwrap_err();
    assert_error_code(&unpause_err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
}

#[test]
fn test_timelocked_vault_update_executes_after_eta() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(5)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enabling timelock from zero should succeed");

    svm.expire_blockhash();

    let direct_err = UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .deposit_cap(1_234)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&direct_err, TIMELOCK_REQUIRED, "TimelockRequired");

    svm.expire_blockhash();

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("pause remains immediate even when timelock is enabled");

    let mut args = empty_update_args();
    args.deposit_cap = Some(1_234);

    let pending_update = Keypair::new();
    QueueVaultUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .pending_update(pending_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_update],
        )
        .expect("queue vault update should succeed");

    let pending = PendingVaultUpdate::from_bytes(
        svm.get_account(&pending_update.pubkey())
            .expect("pending update should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(pending.vault, vault_pubkey);
    assert_eq!(pending.queued_by, authority.pubkey());
    assert_eq!(pending.eta_slot, pending.created_slot + 5);

    let executor = Keypair::new();
    svm.airdrop(&executor.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();

    let early_err = ExecuteVaultUpdateBuilder::new()
        .executor(executor.pubkey())
        .pending_update(pending_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &executor.pubkey(), &[&executor])
        .unwrap_err();
    assert_error_code(&early_err, TIMELOCK_NOT_READY, "TimelockNotReady");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    ExecuteVaultUpdateBuilder::new()
        .executor(executor.pubkey())
        .pending_update(pending_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &executor.pubkey(), &[&executor])
        .expect("execute after eta should succeed");

    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.deposit_cap, 1_234);
    assert!(
        svm.get_account(&pending_update.pubkey()).is_none(),
        "executed update should close pending account"
    );
}

#[test]
fn test_timelocked_vault_update_cancel_closes_without_applying() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enabling timelock should succeed");

    let mut args = empty_update_args();
    args.max_nav_staleness_slots = Some(7);

    let pending_update = Keypair::new();
    svm.expire_blockhash();
    QueueVaultUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .pending_update(pending_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_update],
        )
        .expect("queue vault update should succeed");

    svm.expire_blockhash();
    CancelVaultUpdateBuilder::new()
        .authority(authority.pubkey())
        .pending_update(pending_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("curator should cancel queued update");

    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.max_nav_staleness_slots, 0);
    assert!(
        svm.get_account(&pending_update.pubkey()).is_none(),
        "canceled update should close pending account"
    );
}

#[test]
fn test_timelock_queue_rejects_pause_change() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enabling timelock should succeed");

    let mut args = empty_update_args();
    args.paused = Some(true);

    let pending_update = Keypair::new();
    svm.expire_blockhash();
    let err = QueueVaultUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .pending_update(pending_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_update],
        )
        .unwrap_err();

    assert_error_code(&err, INVALID_TIMELOCK_CHANGE, "InvalidTimelockChange");
}

#[test]
fn test_timelock_allows_direct_breaker_change_but_rejects_queued_breaker_change() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enabling timelock should succeed");

    let new_breaker = Keypair::new();
    svm.expire_blockhash();
    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .breaker(new_breaker.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("breaker rotation remains immediate");

    let vault_after = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault_after.breaker, new_breaker.pubkey());

    let mut args = empty_update_args();
    args.breaker = Some(authority.pubkey());

    let pending_update = Keypair::new();
    svm.expire_blockhash();
    let err = QueueVaultUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .pending_update(pending_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_update],
        )
        .unwrap_err();

    assert_error_code(&err, INVALID_TIMELOCK_CHANGE, "InvalidTimelockChange");
}

#[test]
fn test_timelock_queue_requires_configured_delay() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    let mut args = empty_update_args();
    args.deposit_cap = Some(100);

    let pending_update = Keypair::new();
    let err = QueueVaultUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .pending_update(pending_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_update],
        )
        .unwrap_err();

    assert_error_code(&err, TIMELOCK_NOT_CONFIGURED, "TimelockNotConfigured");
}

#[test]
fn test_timelock_execute_rejects_stale_queued_curator() {
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultAsyncBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(2)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enabling timelock should succeed");

    let mut args = empty_update_args();
    args.deposit_cap = Some(9_999);

    let pending_update = Keypair::new();
    svm.expire_blockhash();
    QueueVaultUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .pending_update(pending_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_update],
        )
        .expect("queue vault update should succeed");

    let pending = PendingVaultUpdate::from_bytes(
        svm.get_account(&pending_update.pubkey())
            .expect("pending update should exist")
            .data(),
    )
    .unwrap();

    let new_authority = Keypair::new();
    svm.airdrop(&new_authority.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    InviteNewAuthorityBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .new_authority(new_authority.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("invite new authority should succeed");

    svm.expire_blockhash();
    AcceptAuthorityInvitationBuilder::new()
        .new_authority(new_authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &new_authority.pubkey(), &[&new_authority])
        .expect("accept authority should succeed");

    let executor = Keypair::new();
    svm.airdrop(&executor.pubkey(), 1_000_000_000).unwrap();
    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    let err = ExecuteVaultUpdateBuilder::new()
        .executor(executor.pubkey())
        .pending_update(pending_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &executor.pubkey(), &[&executor])
        .unwrap_err();

    assert_error_code(&err, STALE_TIMELOCK_AUTHORITY, "StaleTimelockAuthority");
}
