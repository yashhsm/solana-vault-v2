use anchor_spl::token;
use async_vault_v2_client::{
    extensions::{get_extension_bytes, ExtensionType, VAULT_TLV_START},
    lite::SendTransaction,
    sdk::program_id,
    CancelFeeUpdateBuilder, ExecuteFeeUpdateBuilder, FeeType, FeeUpdateArgs, FeeUpdateKind,
    InitializeDepositFeeBuilder, InitializeWithdrawalFeeBuilder, PendingFeeUpdate,
    QueueFeeUpdateBuilder, UpdateDepositFeeBuilder, UpdateVaultBuilder, UpdateWithdrawalFeeBuilder,
    Vault,
};
use litesvm::{
    types::{FailedTransactionMetadata, TransactionMetadata},
    LiteSVM,
};
use solana_sdk::{
    account::ReadableAccount, clock::Clock, pubkey::Pubkey, signature::Keypair, signer::Signer,
};
use test_case::test_case;

use crate::{
    async_helper_functions::{assert_error_code, set_up_async_vault_v2},
    async_vault_v2::constants::{
        EXTENSION_ALREADY_INITIALIZED, FEE_BPS_EXCEEDED, TIMELOCK_NOT_READY, TIMELOCK_REQUIRED,
        UNAUTHORIZED_SIGNER, UNINITIALIZED_EXTENSION,
    },
};

#[derive(Clone, Copy)]
enum FeeKind {
    Deposit,
    Withdrawal,
}

fn init_fee(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault: Pubkey,
    fee: FeeType,
    kind: FeeKind,
) -> Result<TransactionMetadata, FailedTransactionMetadata> {
    match kind {
        FeeKind::Deposit => InitializeDepositFeeBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault)
            .deposit_fee(fee)
            .instruction()
            .send_transaction(svm, &authority.pubkey(), &[authority]),
        FeeKind::Withdrawal => InitializeWithdrawalFeeBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault)
            .withdrawal_fee(fee)
            .instruction()
            .send_transaction(svm, &authority.pubkey(), &[authority]),
    }
}

fn update_fee(
    svm: &mut LiteSVM,
    authority: &Keypair,
    vault: Pubkey,
    fee: FeeType,
    kind: FeeKind,
) -> Result<TransactionMetadata, FailedTransactionMetadata> {
    match kind {
        FeeKind::Deposit => UpdateDepositFeeBuilder::new()
            .authority(authority.pubkey())
            .vault(vault)
            .new_deposit_fee(fee)
            .instruction()
            .send_transaction(svm, &authority.pubkey(), &[authority]),
        FeeKind::Withdrawal => UpdateWithdrawalFeeBuilder::new()
            .authority(authority.pubkey())
            .vault(vault)
            .new_withdrawal_fee(fee)
            .instruction()
            .send_transaction(svm, &authority.pubkey(), &[authority]),
    }
}

fn read_fee(svm: &LiteSVM, vault: Pubkey, kind: FeeKind) -> FeeType {
    let account = svm.get_account(&vault).expect("vault should exist");
    let ext_type = match kind {
        FeeKind::Deposit => ExtensionType::DepositFee,
        FeeKind::Withdrawal => ExtensionType::WithdrawalFee,
    };
    let bytes = get_extension_bytes(&account.data()[VAULT_TLV_START..], ext_type)
        .expect("fee extension should exist");
    match bytes[0] {
        0 => FeeType::FixedAmount {
            amount: u64::from_le_bytes(bytes[1..9].try_into().unwrap()),
        },
        1 => FeeType::Percentage {
            bps: u16::from_le_bytes(bytes[1..3].try_into().unwrap()),
        },
        _ => panic!("invalid fee discriminant"),
    }
}

fn setup_vault() -> (LiteSVM, Keypair, Keypair, Pubkey) {
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
    (svm, authority, share_mint, vault_pubkey)
}

#[test_case(FeeKind::Deposit, FeeType::FixedAmount { amount: 100 }, FeeType::Percentage { bps: 500 } ; "deposit")]
#[test_case(FeeKind::Withdrawal, FeeType::Percentage { bps: 200 }, FeeType::FixedAmount { amount: 50 } ; "withdrawal")]
fn test_initialize_and_update_fee(kind: FeeKind, initial_fee: FeeType, updated_fee: FeeType) {
    let (mut svm, authority, _share_mint, vault_pubkey) = setup_vault();

    init_fee(&mut svm, &authority, vault_pubkey, initial_fee, kind)
        .expect("init fee should succeed");

    let vault_account = svm.get_account(&vault_pubkey).unwrap();
    let vault_config = Vault::from_bytes(vault_account.data()).unwrap();
    assert!(!vault_config.initialized);

    update_fee(&mut svm, &authority, vault_pubkey, updated_fee, kind)
        .expect("update fee should succeed");
}

#[test]
fn test_initialize_both_fees() {
    let (mut svm, authority, _share_mint, vault_pubkey) = setup_vault();

    let deposit_fee = FeeType::FixedAmount { amount: 100 };
    InitializeDepositFeeBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .deposit_fee(deposit_fee)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("init deposit fee should succeed");

    let withdrawal_fee = FeeType::Percentage { bps: 300 };
    InitializeWithdrawalFeeBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .withdrawal_fee(withdrawal_fee)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("init withdrawal fee should succeed");
}

#[test_case(FeeKind::Deposit, FeeType::FixedAmount { amount: 100 } ; "deposit")]
#[test_case(FeeKind::Withdrawal, FeeType::Percentage { bps: 100 } ; "withdrawal")]
fn test_duplicate_init_fails(kind: FeeKind, fee: FeeType) {
    let (mut svm, authority, _share_mint, vault_pubkey) = setup_vault();

    init_fee(&mut svm, &authority, vault_pubkey, fee.clone(), kind)
        .expect("first init should succeed");

    svm.expire_blockhash();

    let result = init_fee(&mut svm, &authority, vault_pubkey, fee, kind);
    assert_error_code(
        &result.unwrap_err(),
        EXTENSION_ALREADY_INITIALIZED,
        "ExtensionAlreadyInitialized",
    );
}

#[test_case(FeeKind::Deposit, FeeType::FixedAmount { amount: 100 } ; "deposit")]
#[test_case(FeeKind::Withdrawal, FeeType::Percentage { bps: 100 } ; "withdrawal")]
fn test_update_before_init_fails(kind: FeeKind, fee: FeeType) {
    let (mut svm, authority, _share_mint, vault_pubkey) = setup_vault();

    let result = update_fee(&mut svm, &authority, vault_pubkey, fee, kind);
    assert_error_code(
        &result.unwrap_err(),
        UNINITIALIZED_EXTENSION,
        "UninitializedExtension",
    );
}

#[test_case(FeeKind::Deposit ; "deposit")]
#[test_case(FeeKind::Withdrawal ; "withdrawal")]
fn test_invalid_bps_init_fails(kind: FeeKind) {
    let (mut svm, authority, _share_mint, vault_pubkey) = setup_vault();

    let fee = FeeType::Percentage { bps: 10_001 };
    let result = init_fee(&mut svm, &authority, vault_pubkey, fee, kind);
    assert_error_code(&result.unwrap_err(), FEE_BPS_EXCEEDED, "FeeBpsExceeded");
}

#[test]
fn test_initialize_fee_unauthorized_signer_fails() {
    let (mut svm, _authority, _share_mint, vault_pubkey) = setup_vault();

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();

    let deposit_fee = FeeType::FixedAmount { amount: 100 };
    let result = InitializeDepositFeeBuilder::new()
        .payer(unauthorized.pubkey())
        .authority(unauthorized.pubkey())
        .vault(vault_pubkey)
        .deposit_fee(deposit_fee)
        .instruction()
        .send_transaction(&mut svm, &unauthorized.pubkey(), &[&unauthorized]);
    assert_error_code(
        &result.unwrap_err(),
        UNAUTHORIZED_SIGNER,
        "UnauthorizedSigner",
    );
}

#[test]
fn test_update_fee_unauthorized_signer_fails() {
    let (mut svm, authority, _share_mint, vault_pubkey) = setup_vault();

    let deposit_fee = FeeType::FixedAmount { amount: 100 };
    InitializeDepositFeeBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .deposit_fee(deposit_fee)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("init should succeed");

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();

    let new_fee = FeeType::FixedAmount { amount: 200 };
    let result = UpdateDepositFeeBuilder::new()
        .authority(unauthorized.pubkey())
        .vault(vault_pubkey)
        .new_deposit_fee(new_fee)
        .instruction()
        .send_transaction(&mut svm, &unauthorized.pubkey(), &[&unauthorized]);
    assert_error_code(
        &result.unwrap_err(),
        UNAUTHORIZED_SIGNER,
        "UnauthorizedSigner",
    );
}

#[test]
fn test_update_fee_requires_timelock_queue_when_timelock_enabled() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();

    let deposit_fee = FeeType::FixedAmount { amount: 100 };
    InitializeDepositFeeBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .deposit_fee(deposit_fee)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("init should succeed");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(2)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enabling timelock should succeed");

    svm.expire_blockhash();

    let err = UpdateDepositFeeBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .new_deposit_fee(FeeType::FixedAmount { amount: 200 })
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");
}

#[test]
fn test_queued_fee_update_executes_after_eta() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();

    init_fee(
        &mut svm,
        &authority,
        vault_pubkey,
        FeeType::FixedAmount { amount: 100 },
        FeeKind::Deposit,
    )
    .expect("init fee should succeed");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(4)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enable timelock should succeed");

    let pending_fee_update = Keypair::new();
    let args = FeeUpdateArgs {
        kind: FeeUpdateKind::Deposit,
        fee: FeeType::Percentage { bps: 250 },
    };
    svm.expire_blockhash();
    QueueFeeUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .pending_fee_update(pending_fee_update.pubkey())
        .args(args.clone())
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_fee_update],
        )
        .expect("queue fee update should succeed");

    let pending = PendingFeeUpdate::from_bytes(
        svm.get_account(&pending_fee_update.pubkey())
            .expect("pending fee update should exist")
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
    let err = ExecuteFeeUpdateBuilder::new()
        .executor(executor.pubkey())
        .pending_fee_update(pending_fee_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &executor.pubkey(), &[&executor])
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_NOT_READY, "TimelockNotReady");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();

    ExecuteFeeUpdateBuilder::new()
        .executor(executor.pubkey())
        .pending_fee_update(pending_fee_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &executor.pubkey(), &[&executor])
        .expect("execute fee update should succeed");

    assert_eq!(
        read_fee(&svm, vault_pubkey, FeeKind::Deposit),
        FeeType::Percentage { bps: 250 }
    );
    assert!(
        svm.get_account(&pending_fee_update.pubkey()).is_none(),
        "executed fee update should close pending account"
    );
}

#[test]
fn test_queued_fee_update_cancel_closes_without_applying() {
    let (mut svm, authority, share_mint, vault_pubkey) = setup_vault();

    init_fee(
        &mut svm,
        &authority,
        vault_pubkey,
        FeeType::Percentage { bps: 100 },
        FeeKind::Withdrawal,
    )
    .expect("init fee should succeed");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .timelock_delay_slots(4)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("enable timelock should succeed");

    let pending_fee_update = Keypair::new();
    svm.expire_blockhash();
    QueueFeeUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .pending_fee_update(pending_fee_update.pubkey())
        .args(FeeUpdateArgs {
            kind: FeeUpdateKind::Withdrawal,
            fee: FeeType::FixedAmount { amount: 42 },
        })
        .instruction()
        .send_transaction(
            &mut svm,
            &authority.pubkey(),
            &[&authority, &pending_fee_update],
        )
        .expect("queue fee update should succeed");

    svm.expire_blockhash();
    CancelFeeUpdateBuilder::new()
        .authority(authority.pubkey())
        .pending_fee_update(pending_fee_update.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("cancel fee update should succeed");

    assert_eq!(
        read_fee(&svm, vault_pubkey, FeeKind::Withdrawal),
        FeeType::Percentage { bps: 100 }
    );
    assert!(
        svm.get_account(&pending_fee_update.pubkey()).is_none(),
        "canceled fee update should close pending account"
    );
}
