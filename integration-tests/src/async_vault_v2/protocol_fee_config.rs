use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, InitializeProtocolFeeConfigBuilder, ProtocolFeeConfig,
    UpdateProtocolFeeConfigBuilder,
};
use litesvm::LiteSVM;
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey, signature::Keypair, signer::Signer};

use crate::{
    async_helper_functions::assert_error_code,
    async_vault_v2::constants::{INVALID_FEE_RECIPIENT, UNAUTHORIZED_SIGNER},
};

const PROTOCOL_FEE_CONFIG_SEED: &[u8] = b"protocol_fee_config";

fn protocol_fee_config_pda() -> Pubkey {
    Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &program_id()).0
}

fn setup_svm() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
    let authority = Keypair::new();
    svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    (svm, authority)
}

#[test]
fn test_initialize_protocol_fee_config_sets_authority_and_recipient() {
    let (mut svm, authority) = setup_svm();
    let protocol_fee_config = protocol_fee_config_pda();
    let protocol_fee_recipient = Keypair::new();

    InitializeProtocolFeeConfigBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize protocol fee config should succeed");

    let config = ProtocolFeeConfig::from_bytes(
        svm.get_account(&protocol_fee_config)
            .expect("protocol fee config should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(config.authority, authority.pubkey());
    assert_eq!(
        config.protocol_fee_recipient,
        protocol_fee_recipient.pubkey()
    );
}

#[test]
fn test_initialize_protocol_fee_config_rejects_default_recipient_and_reinit() {
    let (mut svm, authority) = setup_svm();
    let protocol_fee_config = protocol_fee_config_pda();
    let protocol_fee_recipient = Keypair::new();

    let err = InitializeProtocolFeeConfigBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(Pubkey::default())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, INVALID_FEE_RECIPIENT, "InvalidFeeRecipient");

    InitializeProtocolFeeConfigBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize protocol fee config should succeed");

    svm.expire_blockhash();
    let err = InitializeProtocolFeeConfigBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(protocol_fee_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("already in use")
            || format!("{err:?}").contains("AccountAlreadyInitialized")
            || format!("{err:?}").contains("custom program error: 0x0"),
        "expected re-initialize to fail, got {err:?}"
    );
}

#[test]
fn test_update_protocol_fee_config_requires_authority_and_non_default_recipient() {
    let (mut svm, authority) = setup_svm();
    let protocol_fee_config = protocol_fee_config_pda();
    let first_recipient = Keypair::new();
    let second_recipient = Keypair::new();

    InitializeProtocolFeeConfigBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(first_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initialize protocol fee config should succeed");

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();
    let err = UpdateProtocolFeeConfigBuilder::new()
        .authority(unauthorized.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(second_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &unauthorized.pubkey(), &[&unauthorized])
        .unwrap_err();
    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");

    let err = UpdateProtocolFeeConfigBuilder::new()
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(Pubkey::default())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, INVALID_FEE_RECIPIENT, "InvalidFeeRecipient");

    UpdateProtocolFeeConfigBuilder::new()
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_recipient(second_recipient.pubkey())
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("update protocol fee config should succeed");

    let config = ProtocolFeeConfig::from_bytes(
        svm.get_account(&protocol_fee_config)
            .expect("protocol fee config should exist")
            .data(),
    )
    .unwrap();
    assert_eq!(config.authority, authority.pubkey());
    assert_eq!(config.protocol_fee_recipient, second_recipient.pubkey());
}
