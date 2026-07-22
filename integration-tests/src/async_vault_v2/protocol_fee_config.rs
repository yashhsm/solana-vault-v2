use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, AcceptProtocolFeeAuthorityTransferBuilder,
    CancelProtocolFeeAuthorityTransferBuilder, CancelProtocolFeeConfigUpdateBuilder,
    ExecuteProtocolFeeConfigUpdateBuilder, InitializeProtocolFeeConfigBuilder,
    InitializeProtocolFeeConfigV2Builder, PauseProtocolFeeConfigBuilder,
    PendingProtocolFeeAuthorityTransfer, PendingProtocolFeeConfigUpdate, ProtocolFeeConfig,
    ProtocolFeeConfigUpdateArgs, ProtocolFeeGovernance, QueueProtocolFeeAuthorityTransferBuilder,
    QueueProtocolFeeConfigUpdateBuilder, UpdateProtocolFeeConfigBuilder,
    PROTOCOL_FEE_CONFIG_DISCRIMINATOR,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::{Account, ReadableAccount},
    clock::Clock,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
};
use std::str::FromStr;

use crate::{
    async_helper_functions::assert_error_code,
    async_vault_v2::constants::{
        INVALID_FEE_RECIPIENT, INVALID_PROTOCOL_FEE_TIMELOCK,
        LEGACY_PROTOCOL_FEE_INSTRUCTION_DISABLED, STALE_PROTOCOL_FEE_GOVERNANCE_VERSION,
        TIMELOCK_NOT_READY, UNAUTHORIZED_SIGNER,
    },
};

const PROTOCOL_FEE_CONFIG_SEED: &[u8] = b"protocol_fee_config";
const PROTOCOL_FEE_GOVERNANCE_SEED: &[u8] = b"protocol_fee_governance";
const INITIAL_TIMELOCK_DELAY_SLOTS: u64 = 5;

struct Fixture {
    svm: LiteSVM,
    payer: Keypair,
    upgrade_authority: Keypair,
    authority: Keypair,
    breaker: Keypair,
    protocol_fee_config: Pubkey,
    protocol_fee_governance: Pubkey,
    program_data: Pubkey,
}

fn protocol_fee_config_pda() -> Pubkey {
    Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &program_id()).0
}

fn protocol_fee_governance_pda() -> Pubkey {
    Pubkey::find_program_address(&[PROTOCOL_FEE_GOVERNANCE_SEED], &program_id()).0
}

fn set_program_upgrade_authority(svm: &mut LiteSVM, upgrade_authority: Pubkey) -> Pubkey {
    let upgradeable_loader =
        Pubkey::from_str("BPFLoaderUpgradeab1e11111111111111111111111").unwrap();
    let program_data =
        Pubkey::find_program_address(&[program_id().as_ref()], &upgradeable_loader).0;
    let mut account = svm
        .get_account(&program_data)
        .expect("program data account should exist");

    // UpgradeableLoaderState::ProgramData is bincode encoded as:
    // enum tag (u32), slot (u64), Option tag (u8), authority (32 bytes).
    assert_eq!(
        u32::from_le_bytes(account.data[0..4].try_into().unwrap()),
        3,
        "expected an upgradeable ProgramData account"
    );
    assert!(
        account.data.len() >= 45,
        "program data metadata is truncated"
    );
    account.data[12] = 1;
    account.data[13..45].copy_from_slice(upgrade_authority.as_ref());
    svm.set_account(program_data, account).unwrap();
    program_data
}

fn setup_svm() -> Fixture {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();

    let payer = Keypair::new();
    let upgrade_authority = Keypair::new();
    let authority = Keypair::new();
    let breaker = Keypair::new();
    for signer in [&payer, &upgrade_authority, &authority, &breaker] {
        svm.airdrop(&signer.pubkey(), 1_000_000_000).unwrap();
    }
    let program_data = set_program_upgrade_authority(&mut svm, upgrade_authority.pubkey());

    Fixture {
        svm,
        payer,
        upgrade_authority,
        authority,
        breaker,
        protocol_fee_config: protocol_fee_config_pda(),
        protocol_fee_governance: protocol_fee_governance_pda(),
        program_data,
    }
}

fn initialize_v2(fixture: &mut Fixture) {
    InitializeProtocolFeeConfigV2Builder::new()
        .payer(fixture.payer.pubkey())
        .upgrade_authority(fixture.upgrade_authority.pubkey())
        .program_data(fixture.program_data)
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .authority(fixture.authority.pubkey())
        .breaker(fixture.breaker.pubkey())
        .timelock_delay_slots(INITIAL_TIMELOCK_DELAY_SLOTS)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.upgrade_authority],
        )
        .expect("upgrade authority should initialize protocol-fee governance");
}

fn read_config(fixture: &Fixture) -> ProtocolFeeConfig {
    ProtocolFeeConfig::from_bytes(
        fixture
            .svm
            .get_account(&fixture.protocol_fee_config)
            .expect("protocol fee config should exist")
            .data(),
    )
    .unwrap()
}

fn read_governance(fixture: &Fixture) -> ProtocolFeeGovernance {
    ProtocolFeeGovernance::from_bytes(
        fixture
            .svm
            .get_account(&fixture.protocol_fee_governance)
            .expect("protocol fee governance should exist")
            .data(),
    )
    .unwrap()
}

fn queue_config_update(
    fixture: &mut Fixture,
    authority: &Keypair,
    recipient: Pubkey,
    timelock_delay_slots: Option<u64>,
) -> Keypair {
    let pending = Keypair::new();
    QueueProtocolFeeConfigUpdateBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(pending.pubkey())
        .args(ProtocolFeeConfigUpdateArgs {
            protocol_fee_recipient: recipient,
            timelock_delay_slots,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, authority, &pending],
        )
        .expect("authority should queue protocol-fee update");
    pending
}

fn advance_to_slot(svm: &mut LiteSVM, slot: u64) {
    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = slot;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
}

#[test]
fn test_legacy_bootstrap_is_disabled_and_v2_requires_upgrade_authority() {
    let mut fixture = setup_svm();
    let attacker = Keypair::new();
    fixture
        .svm
        .airdrop(&attacker.pubkey(), 1_000_000_000)
        .unwrap();

    let err = InitializeProtocolFeeConfigBuilder::new()
        .payer(attacker.pubkey())
        .authority(attacker.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_recipient(attacker.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &attacker.pubkey(), &[&attacker])
        .unwrap_err();
    assert_error_code(
        &err,
        LEGACY_PROTOCOL_FEE_INSTRUCTION_DISABLED,
        "LegacyProtocolFeeInstructionDisabled",
    );
    assert!(fixture
        .svm
        .get_account(&fixture.protocol_fee_config)
        .is_none());

    fixture.svm.expire_blockhash();
    let err = InitializeProtocolFeeConfigV2Builder::new()
        .payer(attacker.pubkey())
        .upgrade_authority(attacker.pubkey())
        .program_data(fixture.program_data)
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .authority(attacker.pubkey())
        .breaker(attacker.pubkey())
        .timelock_delay_slots(INITIAL_TIMELOCK_DELAY_SLOTS)
        .instruction()
        .send_transaction(&mut fixture.svm, &attacker.pubkey(), &[&attacker])
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
    assert!(fixture
        .svm
        .get_account(&fixture.protocol_fee_config)
        .is_none());

    fixture.svm.expire_blockhash();
    let err = InitializeProtocolFeeConfigV2Builder::new()
        .payer(fixture.payer.pubkey())
        .upgrade_authority(fixture.upgrade_authority.pubkey())
        .program_data(fixture.program_data)
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .authority(fixture.authority.pubkey())
        .breaker(fixture.breaker.pubkey())
        .timelock_delay_slots(0)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.upgrade_authority],
        )
        .unwrap_err();
    assert_error_code(
        &err,
        INVALID_PROTOCOL_FEE_TIMELOCK,
        "InvalidProtocolFeeTimelock",
    );

    fixture.svm.expire_blockhash();
    initialize_v2(&mut fixture);
    let config = read_config(&fixture);
    let governance = read_governance(&fixture);
    assert_eq!(config.authority, fixture.authority.pubkey());
    assert_eq!(config.protocol_fee_recipient, Pubkey::default());
    assert_eq!(governance.protocol_fee_config, fixture.protocol_fee_config);
    assert_eq!(governance.breaker, fixture.breaker.pubkey());
    assert_eq!(
        governance.timelock_delay_slots,
        INITIAL_TIMELOCK_DELAY_SLOTS
    );
    assert!(governance.paused);
    assert_eq!(governance.version, 0);

    fixture.svm.expire_blockhash();
    InitializeProtocolFeeConfigV2Builder::new()
        .payer(fixture.payer.pubkey())
        .upgrade_authority(fixture.upgrade_authority.pubkey())
        .program_data(fixture.program_data)
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .authority(attacker.pubkey())
        .breaker(attacker.pubkey())
        .timelock_delay_slots(1)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.upgrade_authority],
        )
        .expect_err("the governance sidecar makes bootstrap one-time");
    assert_eq!(read_config(&fixture).authority, fixture.authority.pubkey());
}

#[test]
fn test_v2_bootstrap_migrates_existing_legacy_config_fail_closed() {
    let mut fixture = setup_svm();
    let legacy_authority = Keypair::new().pubkey();
    let legacy_recipient = Keypair::new().pubkey();
    let (_, bump) = Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &program_id());
    let legacy_config = ProtocolFeeConfig {
        discriminator: PROTOCOL_FEE_CONFIG_DISCRIMINATOR,
        authority: legacy_authority,
        protocol_fee_recipient: legacy_recipient,
        bump,
    };
    let data = borsh::to_vec(&legacy_config).unwrap();
    fixture
        .svm
        .set_account(
            fixture.protocol_fee_config,
            Account {
                lamports: fixture.svm.minimum_balance_for_rent_exemption(data.len()),
                data,
                owner: program_id(),
                executable: false,
                rent_epoch: 0,
            },
        )
        .unwrap();

    initialize_v2(&mut fixture);

    let migrated = read_config(&fixture);
    assert_eq!(migrated.authority, fixture.authority.pubkey());
    assert_eq!(migrated.protocol_fee_recipient, Pubkey::default());
    let governance = read_governance(&fixture);
    assert!(governance.paused);
    assert_eq!(governance.version, 0);
}

#[test]
fn test_config_update_is_timelocked_permissionless_and_cancellable() {
    let mut fixture = setup_svm();
    initialize_v2(&mut fixture);
    let recipient = Keypair::new().pubkey();
    let authority = fixture.authority.insecure_clone();
    let pending = queue_config_update(&mut fixture, &authority, recipient, Some(7));
    let pending_state = PendingProtocolFeeConfigUpdate::from_bytes(
        fixture
            .svm
            .get_account(&pending.pubkey())
            .expect("pending update should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(pending_state.expected_version, 0);
    assert_eq!(
        pending_state.eta_slot,
        pending_state.created_slot + INITIAL_TIMELOCK_DELAY_SLOTS
    );

    let executor = Keypair::new();
    fixture
        .svm
        .airdrop(&executor.pubkey(), 1_000_000_000)
        .unwrap();
    let err = ExecuteProtocolFeeConfigUpdateBuilder::new()
        .executor(executor.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(pending.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &executor.pubkey(), &[&executor])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_NOT_READY, "TimelockNotReady");

    advance_to_slot(&mut fixture.svm, pending_state.eta_slot);
    ExecuteProtocolFeeConfigUpdateBuilder::new()
        .executor(executor.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(pending.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &executor.pubkey(), &[&executor])
        .expect("any signer should execute a mature update");

    let config = read_config(&fixture);
    let governance = read_governance(&fixture);
    assert_eq!(config.protocol_fee_recipient, recipient);
    assert!(!governance.paused);
    assert_eq!(governance.timelock_delay_slots, 7);
    assert_eq!(governance.version, 1);
    assert!(fixture.svm.get_account(&pending.pubkey()).is_none());

    fixture.svm.expire_blockhash();
    let err = UpdateProtocolFeeConfigBuilder::new()
        .authority(fixture.authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_recipient(Keypair::new().pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap_err();
    assert_error_code(
        &err,
        LEGACY_PROTOCOL_FEE_INSTRUCTION_DISABLED,
        "LegacyProtocolFeeInstructionDisabled",
    );
    assert_eq!(read_config(&fixture).protocol_fee_recipient, recipient);

    fixture.svm.expire_blockhash();
    let cancelled = queue_config_update(&mut fixture, &authority, Keypair::new().pubkey(), None);
    CancelProtocolFeeConfigUpdateBuilder::new()
        .authority(fixture.authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(cancelled.pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .expect("authority should cancel a queued update");
    assert!(fixture.svm.get_account(&cancelled.pubkey()).is_none());
}

#[test]
fn test_breaker_pause_disables_override_and_invalidates_queued_changes() {
    let mut fixture = setup_svm();
    initialize_v2(&mut fixture);
    let authority = fixture.authority.insecure_clone();
    let pending = queue_config_update(&mut fixture, &authority, Keypair::new().pubkey(), None);
    let pending_state = PendingProtocolFeeConfigUpdate::from_bytes(
        fixture.svm.get_account(&pending.pubkey()).unwrap().data(),
    )
    .unwrap();

    let outsider = Keypair::new();
    fixture
        .svm
        .airdrop(&outsider.pubkey(), 1_000_000_000)
        .unwrap();
    let err = PauseProtocolFeeConfigBuilder::new()
        .authority(outsider.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .instruction()
        .send_transaction(&mut fixture.svm, &outsider.pubkey(), &[&outsider])
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    fixture.svm.expire_blockhash();
    PauseProtocolFeeConfigBuilder::new()
        .authority(fixture.breaker.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.breaker.pubkey(),
            &[&fixture.breaker],
        )
        .expect("breaker should pause immediately");
    assert_eq!(
        read_config(&fixture).protocol_fee_recipient,
        Pubkey::default()
    );
    let governance = read_governance(&fixture);
    assert!(governance.paused);
    assert_eq!(governance.version, 1);

    let executor = Keypair::new();
    fixture
        .svm
        .airdrop(&executor.pubkey(), 1_000_000_000)
        .unwrap();
    advance_to_slot(&mut fixture.svm, pending_state.eta_slot);
    let err = ExecuteProtocolFeeConfigUpdateBuilder::new()
        .executor(executor.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(pending.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &executor.pubkey(), &[&executor])
        .unwrap_err();
    assert_error_code(
        &err,
        STALE_PROTOCOL_FEE_GOVERNANCE_VERSION,
        "StaleProtocolFeeGovernanceVersion",
    );
}

#[test]
fn test_authority_transfer_requires_delay_and_new_authority_acceptance() {
    let mut fixture = setup_svm();
    initialize_v2(&mut fixture);
    let pending = Keypair::new();
    let new_authority = Keypair::new();
    fixture
        .svm
        .airdrop(&new_authority.pubkey(), 1_000_000_000)
        .unwrap();

    QueueProtocolFeeAuthorityTransferBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_transfer(pending.pubkey())
        .new_authority(new_authority.pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority, &pending],
        )
        .expect("authority should queue its successor");
    let pending_state = PendingProtocolFeeAuthorityTransfer::from_bytes(
        fixture.svm.get_account(&pending.pubkey()).unwrap().data(),
    )
    .unwrap();

    let wrong_signer = Keypair::new();
    fixture
        .svm
        .airdrop(&wrong_signer.pubkey(), 1_000_000_000)
        .unwrap();
    let err = AcceptProtocolFeeAuthorityTransferBuilder::new()
        .new_authority(wrong_signer.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_transfer(pending.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &wrong_signer.pubkey(), &[&wrong_signer])
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    fixture.svm.expire_blockhash();
    let err = AcceptProtocolFeeAuthorityTransferBuilder::new()
        .new_authority(new_authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_transfer(pending.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &new_authority.pubkey(), &[&new_authority])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_NOT_READY, "TimelockNotReady");

    advance_to_slot(&mut fixture.svm, pending_state.eta_slot);
    AcceptProtocolFeeAuthorityTransferBuilder::new()
        .new_authority(new_authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_transfer(pending.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &new_authority.pubkey(), &[&new_authority])
        .expect("new authority should accept a mature transfer");
    assert_eq!(read_config(&fixture).authority, new_authority.pubkey());
    assert_eq!(read_governance(&fixture).version, 1);
    assert!(fixture.svm.get_account(&pending.pubkey()).is_none());

    fixture.svm.expire_blockhash();
    let rejected_pending = Keypair::new();
    let err = QueueProtocolFeeConfigUpdateBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(rejected_pending.pubkey())
        .args(ProtocolFeeConfigUpdateArgs {
            protocol_fee_recipient: Keypair::new().pubkey(),
            timelock_delay_slots: None,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority, &rejected_pending],
        )
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    fixture.svm.expire_blockhash();
    let cancellable = Keypair::new();
    QueueProtocolFeeAuthorityTransferBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(new_authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_transfer(cancellable.pubkey())
        .new_authority(Keypair::new().pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &new_authority, &cancellable],
        )
        .expect("new authority should control governance");
    CancelProtocolFeeAuthorityTransferBuilder::new()
        .authority(new_authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_transfer(cancellable.pubkey())
        .instruction()
        .send_transaction(&mut fixture.svm, &new_authority.pubkey(), &[&new_authority])
        .expect("authority should cancel a queued transfer");
    assert!(fixture.svm.get_account(&cancellable.pubkey()).is_none());
}

#[test]
fn test_config_update_rejects_invalid_recipient() {
    let mut fixture = setup_svm();
    initialize_v2(&mut fixture);
    let pending = Keypair::new();

    let err = QueueProtocolFeeConfigUpdateBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .protocol_fee_config(fixture.protocol_fee_config)
        .protocol_fee_governance(fixture.protocol_fee_governance)
        .pending_update(pending.pubkey())
        .args(ProtocolFeeConfigUpdateArgs {
            protocol_fee_recipient: Pubkey::default(),
            timelock_delay_slots: None,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority, &pending],
        )
        .unwrap_err();
    assert_error_code(&err, INVALID_FEE_RECIPIENT, "InvalidFeeRecipient");
}
