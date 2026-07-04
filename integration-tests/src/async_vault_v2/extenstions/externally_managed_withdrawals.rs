use anchor_spl::token;
use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, InitializeExternallyManagedWithdrawalsBuilder,
    InitializeVaultBuilder,
};
use litesvm::LiteSVM;
use solana_sdk::signer::Signer;
use test_case::test_case;

use crate::{
    async_helper_functions::{assert_error_code, set_up_async_vault_v2},
    async_vault_v2::constants::{
        EXTENSION_ALREADY_INITIALIZED, UNAUTHORIZED_SIGNER, VAULT_ALREADY_INITIALIZED,
    },
};

fn load_program(svm: &mut LiteSVM) {
    let program_bytes = include_bytes!("../../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
}

#[test]
fn test_initialize_externally_managed_withdrawals_rejects_non_curator() {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);

    let (
        authority,
        _payer,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let err = InitializeExternallyManagedWithdrawalsBuilder::new()
        .payer(authority.pubkey())
        .authority(user.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority, &user])
        .unwrap_err();

    assert_error_code(&err, UNAUTHORIZED_SIGNER, "UnauthorizedSigner");
}

#[test_case(true, false, VAULT_ALREADY_INITIALIZED, "VaultAlreadyInitialized" ; "after_vault_init")]
#[test_case(false, true, EXTENSION_ALREADY_INITIALIZED, "ExtensionAlreadyInitialized" ; "duplicate")]
fn test_initialize_externally_managed_withdrawals_fails(
    init_vault_first: bool,
    init_extension_first: bool,
    expected_error: u32,
    expected_name: &str,
) {
    let mut svm = LiteSVM::new();
    load_program(&mut svm);

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

    if init_vault_first {
        InitializeVaultBuilder::new()
            .share_mint(share_mint.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize vault should succeed");
    }

    if init_extension_first {
        InitializeExternallyManagedWithdrawalsBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("first initialize should succeed");
        svm.expire_blockhash();
    }

    let err = InitializeExternallyManagedWithdrawalsBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, expected_error, expected_name);
}
