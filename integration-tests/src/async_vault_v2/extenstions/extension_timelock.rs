use anchor_spl::token;
use async_vault_v2_client::{
    extensions::{min_redemption, min_subscription, pausable_redemptions, pausable_subscriptions},
    lite::SendTransaction,
    sdk::program_id,
    AcceptAuthorityInvitationBuilder, CancelExtensionUpdateBuilder, ExecuteExtensionUpdateBuilder,
    ExtensionUpdateArgs, ExtensionUpdateKind, InitializeMinRedemptionBuilder,
    InitializeMinSubscriptionBuilder, InitializePausableRedemptionsBuilder,
    InitializePausableSubscriptionsBuilder, InitializeVaultBuilder as InitializeAsyncVaultBuilder,
    InviteNewAuthorityBuilder, PendingExtensionUpdate, QueueExtensionUpdateBuilder,
    UpdateMinRedemptionBuilder, UpdateMinSubscriptionBuilder, UpdatePausableRedemptionsBuilder,
    UpdatePausableSubscriptionsBuilder, UpdateVaultBuilder,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, clock::Clock, pubkey::Pubkey, signature::Keypair, signer::Signer,
};

use crate::{
    async_helper_functions::{assert_error_code, set_up_async_vault_v2},
    async_vault_v2::constants::{
        INVALID_EXTENSION_DATA, STALE_TIMELOCK_AUTHORITY, TIMELOCK_NOT_CONFIGURED,
        TIMELOCK_NOT_READY, TIMELOCK_REQUIRED, UNAUTHORIZED_SIGNER, UNINITIALIZED_EXTENSION,
    },
};

const MIN_SUBSCRIPTION_THRESHOLD: u64 = 1_000_000;
const MIN_REDEMPTION_THRESHOLD: u64 = 2_000_000;

fn setup_vault() -> (LiteSVM, Keypair, Keypair, Pubkey) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../../target/deploy/async_vault_v2.so");
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

    (svm, authority, share_mint, vault_pubkey)
}

fn initialize_all_mutable_extensions(svm: &mut LiteSVM, authority: &Keypair, vault_pubkey: Pubkey) {
    InitializeMinSubscriptionBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .threshold(MIN_SUBSCRIPTION_THRESHOLD)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize min subscription should succeed");

    InitializeMinRedemptionBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .threshold(MIN_REDEMPTION_THRESHOLD)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize min redemption should succeed");

    InitializePausableSubscriptionsBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .paused(false)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize pausable subscriptions should succeed");

    InitializePausableRedemptionsBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .paused(true)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize pausable redemptions should succeed");
}

fn initialize_vault_and_timelock(
    svm: &mut LiteSVM,
    authority: &Keypair,
    share_mint: &Keypair,
    vault_pubkey: Pubkey,
    delay_slots: u64,
) {
    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize vault should succeed");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(delay_slots)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("enable timelock should succeed");
}

fn queue_extension_update(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault_pubkey: Pubkey,
    pending_extension_update: &Keypair,
    args: ExtensionUpdateArgs,
) -> litesvm::types::TransactionResult {
    QueueExtensionUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .pending_extension_update(pending_extension_update.pubkey())
        .args(args)
        .instruction()
        .send_transaction(
            svm,
            &authority.pubkey(),
            &[authority, pending_extension_update],
        )
}

fn execute_extension_update(
    svm: &mut LiteSVM,
    executor: &Keypair,
    vault_pubkey: Pubkey,
    pending_extension_update: Pubkey,
) -> litesvm::types::TransactionResult {
    ExecuteExtensionUpdateBuilder::new()
        .executor(executor.pubkey())
        .pending_extension_update(pending_extension_update)
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(svm, &executor.pubkey(), &[executor])
}

fn transfer_authority(
    svm: &mut LiteSVM,
    authority: &Keypair,
    new_authority: &Keypair,
    vault_pubkey: Pubkey,
) {
    svm.airdrop(&new_authority.pubkey(), 1_000_000_000).unwrap();
    InviteNewAuthorityBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .new_authority(new_authority.pubkey())
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("invite new authority should succeed");

    svm.expire_blockhash();
    AcceptAuthorityInvitationBuilder::new()
        .new_authority(new_authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(svm, &new_authority.pubkey(), &[new_authority])
        .expect("accept authority invitation should succeed");
}

fn vault_data(svm: &LiteSVM, vault_pubkey: Pubkey) -> Vec<u8> {
    svm.get_account(&vault_pubkey)
        .expect("vault account should exist")
        .data()
        .to_vec()
}

#[test]
fn test_direct_mutable_non_fee_extension_updates_require_timelock_queue() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    initialize_all_mutable_extensions(&mut svm, &authority, vault_pubkey);
    initialize_vault_and_timelock(&mut svm, &authority, &share_mint, vault_pubkey, 3);

    svm.expire_blockhash();
    let err = UpdateMinSubscriptionBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .threshold(MIN_SUBSCRIPTION_THRESHOLD + 1)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");

    svm.expire_blockhash();
    let err = UpdateMinRedemptionBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .threshold(MIN_REDEMPTION_THRESHOLD + 1)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");

    svm.expire_blockhash();
    let err = UpdatePausableSubscriptionsBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .paused(true)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");

    svm.expire_blockhash();
    let err = UpdatePausableRedemptionsBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .paused(false)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");
}

#[test]
fn test_queued_min_subscription_update_waits_for_eta_then_applies() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    initialize_all_mutable_extensions(&mut svm, &authority, vault_pubkey);
    initialize_vault_and_timelock(&mut svm, &authority, &share_mint, vault_pubkey, 4);

    let pending_extension_update = Keypair::new();
    let args = ExtensionUpdateArgs {
        kind: ExtensionUpdateKind::MinSubscription,
        threshold: MIN_SUBSCRIPTION_THRESHOLD * 2,
        paused: false,
    };
    svm.expire_blockhash();
    queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_extension_update,
        args.clone(),
    )
    .expect("queue extension update should succeed");

    let pending = PendingExtensionUpdate::from_bytes(
        svm.get_account(&pending_extension_update.pubkey())
            .expect("pending extension update should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(pending.vault, vault_pubkey);
    assert_eq!(pending.queued_by, authority.pubkey());
    assert_eq!(pending.eta_slot, pending.created_slot + 4);
    assert_eq!(pending.args, args);

    let executor = Keypair::new();
    svm.airdrop(&executor.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    let err = execute_extension_update(
        &mut svm,
        &executor,
        vault_pubkey,
        pending_extension_update.pubkey(),
    )
    .unwrap_err();
    assert_error_code(&err, TIMELOCK_NOT_READY, "TimelockNotReady");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    execute_extension_update(
        &mut svm,
        &executor,
        vault_pubkey,
        pending_extension_update.pubkey(),
    )
    .expect("execute extension update should succeed");

    let state = min_subscription::get_state(&vault_data(&svm, vault_pubkey))
        .expect("min subscription should exist");
    assert_eq!(state.threshold, MIN_SUBSCRIPTION_THRESHOLD * 2);
    assert!(
        svm.get_account(&pending_extension_update.pubkey())
            .is_none(),
        "executed extension update should close pending account"
    );
}

#[test]
fn test_queued_min_redemption_update_can_be_canceled_without_applying() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    initialize_all_mutable_extensions(&mut svm, &authority, vault_pubkey);
    initialize_vault_and_timelock(&mut svm, &authority, &share_mint, vault_pubkey, 4);

    let pending_extension_update = Keypair::new();
    svm.expire_blockhash();
    queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_extension_update,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::MinRedemption,
            threshold: MIN_REDEMPTION_THRESHOLD * 2,
            paused: false,
        },
    )
    .expect("queue extension update should succeed");

    svm.expire_blockhash();
    CancelExtensionUpdateBuilder::new()
        .authority(authority.pubkey())
        .pending_extension_update(pending_extension_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("cancel extension update should succeed");

    let state = min_redemption::get_state(&vault_data(&svm, vault_pubkey))
        .expect("min redemption should exist");
    assert_eq!(state.threshold, MIN_REDEMPTION_THRESHOLD);
    assert!(
        svm.get_account(&pending_extension_update.pubkey())
            .is_none(),
        "canceled extension update should close pending account"
    );
}

#[test]
fn test_queued_pausable_updates_toggle_after_timelock() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    initialize_all_mutable_extensions(&mut svm, &authority, vault_pubkey);
    initialize_vault_and_timelock(&mut svm, &authority, &share_mint, vault_pubkey, 2);

    let pending_subscriptions = Keypair::new();
    let pending_redemptions = Keypair::new();
    svm.expire_blockhash();
    queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_subscriptions,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::PausableSubscriptions,
            threshold: 0,
            paused: true,
        },
    )
    .expect("queue pausable subscriptions update should succeed");
    svm.expire_blockhash();
    queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_redemptions,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::PausableRedemptions,
            threshold: 0,
            paused: false,
        },
    )
    .expect("queue pausable redemptions update should succeed");

    let pending = PendingExtensionUpdate::from_bytes(
        svm.get_account(&pending_subscriptions.pubkey())
            .expect("pending subscriptions update should exist")
            .data(),
    )
    .unwrap();
    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);

    let executor = Keypair::new();
    svm.airdrop(&executor.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    execute_extension_update(
        &mut svm,
        &executor,
        vault_pubkey,
        pending_subscriptions.pubkey(),
    )
    .expect("execute pausable subscriptions update should succeed");
    svm.expire_blockhash();
    execute_extension_update(
        &mut svm,
        &executor,
        vault_pubkey,
        pending_redemptions.pubkey(),
    )
    .expect("execute pausable redemptions update should succeed");

    let data = vault_data(&svm, vault_pubkey);
    let subscriptions =
        pausable_subscriptions::get_state(&data).expect("pausable subscriptions should exist");
    let redemptions =
        pausable_redemptions::get_state(&data).expect("pausable redemptions should exist");
    assert!(subscriptions.paused);
    assert!(!redemptions.paused);
}

#[test]
fn test_queue_extension_update_validates_authority_timelock_extension_and_args() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    InitializeMinSubscriptionBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .threshold(MIN_SUBSCRIPTION_THRESHOLD)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize min subscription should succeed");
    InitializeAsyncVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize vault should succeed");

    let pending_without_timelock = Keypair::new();
    svm.expire_blockhash();
    let err = queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_without_timelock,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::MinSubscription,
            threshold: MIN_SUBSCRIPTION_THRESHOLD * 2,
            paused: false,
        },
    )
    .unwrap_err();
    assert_error_code(&err, TIMELOCK_NOT_CONFIGURED, "TimelockNotConfigured");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(2)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enable timelock should succeed");

    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    let pending_unauthorized = Keypair::new();
    svm.expire_blockhash();
    let err = QueueExtensionUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(attacker.pubkey())
        .vault(vault_pubkey)
        .pending_extension_update(pending_unauthorized.pubkey())
        .args(ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::MinSubscription,
            threshold: MIN_SUBSCRIPTION_THRESHOLD * 2,
            paused: false,
        })
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &attacker, &pending_unauthorized],
        )
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    let pending_invalid_args = Keypair::new();
    svm.expire_blockhash();
    let err = queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_invalid_args,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::PausableSubscriptions,
            threshold: 1,
            paused: true,
        },
    )
    .unwrap_err();
    assert_error_code(&err, INVALID_EXTENSION_DATA, "InvalidExtensionData");
    assert!(
        svm.get_account(&pending_invalid_args.pubkey()).is_none(),
        "invalid queue should not leave a pending account"
    );

    let pending_missing_extension = Keypair::new();
    svm.expire_blockhash();
    let err = queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_missing_extension,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::MinRedemption,
            threshold: MIN_REDEMPTION_THRESHOLD * 2,
            paused: false,
        },
    )
    .unwrap_err();
    assert_error_code(&err, UNINITIALIZED_EXTENSION, "UninitializedExtension");
    assert!(
        svm.get_account(&pending_missing_extension.pubkey())
            .is_none(),
        "missing-extension queue should not leave a pending account"
    );
}

#[test]
fn test_cancel_extension_update_requires_current_curator() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    initialize_all_mutable_extensions(&mut svm, &authority, vault_pubkey);
    initialize_vault_and_timelock(&mut svm, &authority, &share_mint, vault_pubkey, 2);

    let pending_extension_update = Keypair::new();
    svm.expire_blockhash();
    queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_extension_update,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::MinSubscription,
            threshold: MIN_SUBSCRIPTION_THRESHOLD * 2,
            paused: false,
        },
    )
    .expect("queue extension update should succeed");

    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    let err = CancelExtensionUpdateBuilder::new()
        .authority(attacker.pubkey())
        .pending_extension_update(pending_extension_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &attacker.pubkey(), &[&attacker])
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    assert!(
        svm.get_account(&pending_extension_update.pubkey())
            .is_some(),
        "failed cancel should keep pending account"
    );
}

#[test]
fn test_execute_extension_update_rejects_stale_curator_after_authority_transfer() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();
    initialize_all_mutable_extensions(&mut svm, &authority, vault_pubkey);
    initialize_vault_and_timelock(&mut svm, &authority, &share_mint, vault_pubkey, 2);

    let pending_extension_update = Keypair::new();
    svm.expire_blockhash();
    queue_extension_update(
        &mut svm,
        &authority,
        vault_pubkey,
        &pending_extension_update,
        ExtensionUpdateArgs {
            kind: ExtensionUpdateKind::MinSubscription,
            threshold: MIN_SUBSCRIPTION_THRESHOLD * 2,
            paused: false,
        },
    )
    .expect("queue extension update should succeed");

    let pending = PendingExtensionUpdate::from_bytes(
        svm.get_account(&pending_extension_update.pubkey())
            .expect("pending extension update should exist")
            .data(),
    )
    .unwrap();

    let new_authority = Keypair::new();
    transfer_authority(&mut svm, &authority, &new_authority, vault_pubkey);

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);

    let executor = Keypair::new();
    svm.airdrop(&executor.pubkey(), 1_000_000_000).unwrap();
    svm.expire_blockhash();
    let err = execute_extension_update(
        &mut svm,
        &executor,
        vault_pubkey,
        pending_extension_update.pubkey(),
    )
    .unwrap_err();
    assert_error_code(&err, STALE_TIMELOCK_AUTHORITY, "StaleTimelockAuthority");

    let state = min_subscription::get_state(&vault_data(&svm, vault_pubkey))
        .expect("min subscription should exist");
    assert_eq!(state.threshold, MIN_SUBSCRIPTION_THRESHOLD);
    assert!(
        svm.get_account(&pending_extension_update.pubkey())
            .is_some(),
        "failed stale execute should keep pending account for current-curator cancellation"
    );

    svm.expire_blockhash();
    CancelExtensionUpdateBuilder::new()
        .authority(new_authority.pubkey())
        .pending_extension_update(pending_extension_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &new_authority.pubkey(), &[&new_authority])
        .expect("current curator should be able to cancel stale extension update");
}
