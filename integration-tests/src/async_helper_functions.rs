use litesvm::LiteSVM;
use solana_sdk::{
    account::{Account, ReadableAccount},
    program_pack::Pack,
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::Transaction,
};
use solana_system_interface::instruction::create_account;

use async_vault_v2_client::{
    lite::SendTransaction, sdk::program_id, CreateVaultBuilder as CreateAsyncVaultBuilder,
    ExecuteProtocolFeeConfigUpdateBuilder, InitializeProtocolFeeConfigV2Builder,
    PendingProtocolFeeConfigUpdate, ProtocolFeeConfigUpdateArgs,
    QueueProtocolFeeConfigUpdateBuilder, Request, RequestType, UpdateVaultBuilder,
    Vault as AsyncVault,
};
use borsh::BorshSerialize;
use std::str::FromStr;

use anchor_spl::{
    associated_token::{
        get_associated_token_address_with_program_id,
        spl_associated_token_account::instruction::create_associated_token_account,
    },
    token::spl_token,
    token_2022::{
        self,
        spl_token_2022::{
            self,
            extension::{
                confidential_mint_burn::ConfidentialMintBurn,
                transfer_fee::instruction::initialize_transfer_fee_config,
                BaseStateWithExtensionsMut, ExtensionType, StateWithExtensions,
                StateWithExtensionsMut,
            },
            state::Mint,
        },
    },
};

use spl_token::state::Account as TokenAccount;
use spl_token_2022::state::{Account as TokenAccount2022, Mint as Token2022Mint};

pub const VAULT_CONFIG_SEED: &[u8] = b"vault";
pub const RESERVE_CONFIG_SEED: &[u8] = b"reserve";
pub const PENDING_VAULT_SEED: &[u8] = b"pending";
pub const PROTOCOL_FEE_CONFIG_SEED: &[u8] = b"protocol_fee_config";
pub const PROTOCOL_FEE_GOVERNANCE_SEED: &[u8] = b"protocol_fee_governance";

/// Securely bootstraps and activates the singleton protocol-fee override for tests.
/// Production callers should use distinct upgrade, governance, and breaker authorities.
pub fn initialize_and_activate_protocol_fee_config(
    svm: &mut LiteSVM,
    authority: &Keypair,
    protocol_fee_recipient: Pubkey,
) -> Pubkey {
    let upgradeable_loader =
        Pubkey::from_str("BPFLoaderUpgradeab1e11111111111111111111111").unwrap();
    let program_data =
        Pubkey::find_program_address(&[program_id().as_ref()], &upgradeable_loader).0;
    let mut program_data_account = svm
        .get_account(&program_data)
        .expect("program data account should exist");
    assert_eq!(
        u32::from_le_bytes(program_data_account.data[0..4].try_into().unwrap()),
        3,
        "expected an upgradeable ProgramData account"
    );
    assert!(program_data_account.data.len() >= 45);
    program_data_account.data[12] = 1;
    program_data_account.data[13..45].copy_from_slice(authority.pubkey().as_ref());
    svm.set_account(program_data, program_data_account).unwrap();

    let protocol_fee_config =
        Pubkey::find_program_address(&[PROTOCOL_FEE_CONFIG_SEED], &program_id()).0;
    let protocol_fee_governance =
        Pubkey::find_program_address(&[PROTOCOL_FEE_GOVERNANCE_SEED], &program_id()).0;
    InitializeProtocolFeeConfigV2Builder::new()
        .payer(authority.pubkey())
        .upgrade_authority(authority.pubkey())
        .program_data(program_data)
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_governance(protocol_fee_governance)
        .authority(authority.pubkey())
        .breaker(authority.pubkey())
        .timelock_delay_slots(1)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("initialize protocol-fee governance");

    let pending_update = Keypair::new();
    svm.expire_blockhash();
    QueueProtocolFeeConfigUpdateBuilder::new()
        .payer(authority.pubkey())
        .authority(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_governance(protocol_fee_governance)
        .pending_update(pending_update.pubkey())
        .args(ProtocolFeeConfigUpdateArgs {
            protocol_fee_recipient,
            timelock_delay_slots: None,
        })
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority, &pending_update])
        .expect("queue protocol-fee recipient activation");
    let pending = PendingProtocolFeeConfigUpdate::from_bytes(
        svm.get_account(&pending_update.pubkey())
            .expect("pending protocol-fee update")
            .data(),
    )
    .unwrap();
    let mut clock = svm.get_sysvar::<solana_sdk::clock::Clock>();
    clock.slot = pending.eta_slot;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    ExecuteProtocolFeeConfigUpdateBuilder::new()
        .executor(authority.pubkey())
        .protocol_fee_config(protocol_fee_config)
        .protocol_fee_governance(protocol_fee_governance)
        .pending_update(pending_update.pubkey())
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
        .expect("activate protocol-fee recipient");
    protocol_fee_config
}

pub fn create_mint(svm: &mut LiteSVM, signer: &Keypair, mint: &Keypair, token_program: &Pubkey) {
    let rent = svm.minimum_balance_for_rent_exemption(Mint::LEN);
    let init_account_ix: solana_sdk::instruction::Instruction = create_account(
        &signer.pubkey(),
        &mint.pubkey(),
        rent,
        Mint::LEN as u64,
        token_program,
    );
    let init_mint_ix = spl_token_2022::instruction::initialize_mint(
        token_program,
        &mint.pubkey(),
        &signer.pubkey(),
        None,
        9,
    )
    .unwrap();

    let init_tx = Transaction::new_signed_with_payer(
        &[init_account_ix, init_mint_ix],
        Some(&signer.pubkey()),
        &[&mint, &signer],
        svm.latest_blockhash(),
    );
    svm.send_transaction(init_tx)
        .expect("create_mint transaction failed");
}

pub fn create_ata(
    svm: &mut LiteSVM,
    owner: &Keypair,
    mint: &Pubkey,
    token_program: &Pubkey,
) -> Pubkey {
    let ata = get_associated_token_address_with_program_id(&owner.pubkey(), &mint, token_program);

    let ata_init_ix =
        create_associated_token_account(&owner.pubkey(), &owner.pubkey(), &mint, token_program);

    let init_tx = Transaction::new_signed_with_payer(
        &[ata_init_ix],
        Some(&owner.pubkey()),
        &[&owner],
        svm.latest_blockhash(),
    );
    svm.send_transaction(init_tx).unwrap();
    ata
}

pub fn helper_mint_to(
    svm: &mut LiteSVM,
    mint: &Pubkey,
    account: &Pubkey,
    authority: &Keypair,
    amount: u64,
    token_program: &Pubkey,
) {
    let mint_to_ix = spl_token_2022::instruction::mint_to(
        token_program,
        mint,
        account,
        &authority.pubkey(),
        &[],
        amount,
    )
    .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[mint_to_ix],
        Some(&authority.pubkey()),
        &[authority],
        svm.latest_blockhash(),
    );

    svm.send_transaction(tx).expect("Failed to mint tokens");
}

pub fn assert_error_code(
    tx_result: &litesvm::types::FailedTransactionMetadata,
    expected_code: u32,
    error_name: &str,
) {
    let error_string = format!("{:?}", tx_result);
    assert!(
        error_string.contains(&format!("Custom({})", expected_code))
            || (!error_name.is_empty() && error_string.contains(error_name)),
        "Expected error code {} ({}), got: {:?}",
        expected_code,
        error_name,
        error_string
    );
}

pub fn create_mint_with_transfer_fee(
    svm: &mut LiteSVM,
    signer: &Keypair,
    mint: &Keypair,
    transfer_fee_basis_points: u16,
    maximum_fee: u64,
) {
    // Calculate space needed for mint + transfer fee extension
    let space =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::TransferFeeConfig])
            .unwrap();

    let rent = svm.minimum_balance_for_rent_exemption(space);

    // Create account with proper space
    let create_account_ix = create_account(
        &signer.pubkey(),
        &mint.pubkey(),
        rent,
        space as u64,
        &spl_token_2022::id(),
    );

    // Initialize transfer fee extension BEFORE initializing mint
    let init_transfer_fee_ix = initialize_transfer_fee_config(
        &spl_token_2022::id(),
        &mint.pubkey(),
        Some(&signer.pubkey()), // transfer_fee_config_authority
        Some(&signer.pubkey()), // withdraw_withheld_authority
        transfer_fee_basis_points,
        maximum_fee,
    )
    .unwrap();

    // Initialize the mint (this must come AFTER extension initialization)
    let init_mint_ix = spl_token_2022::instruction::initialize_mint(
        &spl_token_2022::id(),
        &mint.pubkey(),
        &signer.pubkey(),
        None,
        9,
    )
    .unwrap();

    let tx = Transaction::new_signed_with_payer(
        &[create_account_ix, init_transfer_fee_ix, init_mint_ix],
        Some(&signer.pubkey()),
        &[&mint, &signer],
        svm.latest_blockhash(),
    );

    svm.send_transaction(tx)
        .expect("create_mint_with_transfer_fee transaction failed");
}

pub fn create_mint_with_confidential_mint_burn(svm: &mut LiteSVM, mint: &Keypair, decimals: u8) {
    let space =
        ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::ConfidentialMintBurn])
            .unwrap();
    let mut data = vec![0u8; space];
    {
        let mut state = StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut data).unwrap();
        state.init_extension::<ConfidentialMintBurn>(true).unwrap();
        state.base = Mint {
            decimals,
            is_initialized: true,
            ..Default::default()
        };
        state.pack_base();
        state.init_account_type().unwrap();
    }

    let rent = svm.minimum_balance_for_rent_exemption(space);
    svm.set_account(
        mint.pubkey(),
        Account {
            lamports: rent,
            data,
            owner: spl_token_2022::id(),
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

pub fn approve_request_args(
    svm: &LiteSVM,
    request: &Pubkey,
) -> (Pubkey, RequestType, u64, i64, u64) {
    let account = svm
        .get_account(request)
        .expect("request account should exist");
    let req = Request::from_bytes(account.data()).unwrap();
    (
        req.owner,
        req.request_type,
        req.amount,
        req.created_at,
        req.nav_update_version,
    )
}

/// gets the amount of a token account, depending on the account owner
pub fn get_token_account_amount(account: &Account) -> u64 {
    if account.owner == token_2022::ID {
        StateWithExtensions::<TokenAccount2022>::unpack(account.data())
            .unwrap()
            .base
            .amount
    } else {
        TokenAccount::unpack(account.data()).unwrap().amount
    }
}

/// gets the supply of a token mint, depending on the account owner
pub fn get_mint_supply(account: &Account) -> u64 {
    if account.owner == token_2022::ID {
        let state = StateWithExtensions::<Token2022Mint>::unpack(account.data())
            .expect("unpack token-2022 mint");
        state.base.supply
    } else {
        spl_token::state::Mint::unpack(account.data())
            .expect("unpack token-keg mint")
            .supply
    }
}

pub fn get_mint_authority(account: &Account) -> Option<Pubkey> {
    let data = account.data();
    assert!(data.len() >= 36, "mint account data too short");
    match u32::from_le_bytes(data[0..4].try_into().unwrap()) {
        0 => None,
        1 => Some(Pubkey::new_from_array(data[4..36].try_into().unwrap())),
        tag => panic!("invalid mint authority option tag: {tag}"),
    }
}

pub fn set_up_async_vault_v2(
    svm: &mut LiteSVM,
    asset_token_program: Pubkey,
    asset_mint_transfer_fee_bps: Option<u16>,
    share_token_program: Pubkey,
    user_amount: u64,
) -> (
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let authority = Keypair::new();
    let payer = Keypair::new();
    let mint_authority = Keypair::new();
    let asset_mint = Keypair::new();
    let share_mint = Keypair::new();
    let user = Keypair::new();
    let operator = Keypair::new();
    let fee_recipient = Keypair::new();

    svm.airdrop(&authority.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&fee_recipient.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&mint_authority.pubkey(), 1_000_000_000)
        .unwrap();
    svm.airdrop(&user.pubkey(), 1_000_000_000).unwrap();
    svm.airdrop(&operator.pubkey(), 1_000_000_000).unwrap();

    if asset_token_program == token_2022::ID && asset_mint_transfer_fee_bps.is_some() {
        // Initialize with TransferFee extension enabled
        create_mint_with_transfer_fee(
            svm,
            &mint_authority,
            &asset_mint,
            asset_mint_transfer_fee_bps.unwrap(),
            u64::MAX,
        );
    } else {
        create_mint(svm, &mint_authority, &asset_mint, &asset_token_program);
    }
    create_mint(svm, &mint_authority, &share_mint, &share_token_program);

    let (reserve_pubkey, _) = Pubkey::find_program_address(
        &[RESERVE_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );
    let (pending_vault_pubkey, _) = Pubkey::find_program_address(
        &[PENDING_VAULT_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );

    let (vault_pubkey, _) = Pubkey::find_program_address(
        &[VAULT_CONFIG_SEED, share_mint.pubkey().as_ref()],
        &program_id(),
    );

    CreateAsyncVaultBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .fee_recipient(fee_recipient.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .reserve(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .vault(vault_pubkey)
        .asset_token_program(asset_token_program)
        .share_token_program(share_token_program)
        .authority(authority.pubkey())
        .instruction()
        .send_transaction(svm, &payer.pubkey(), &[&payer, &mint_authority])
        .expect("vault creation should succeed");

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault_pubkey)
        .require_fresh_nav(false)
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[&authority])
        .expect("upstream-parity vault config should succeed");

    let user_token_account = create_ata(svm, &user, &asset_mint.pubkey(), &asset_token_program);
    let fee_recipient_ata = create_ata(
        svm,
        &fee_recipient,
        &asset_mint.pubkey(),
        &asset_token_program,
    );

    helper_mint_to(
        svm,
        &asset_mint.pubkey(),
        &user_token_account,
        &mint_authority,
        user_amount,
        &asset_token_program,
    );

    let user_share_account = create_ata(svm, &user, &share_mint.pubkey(), &share_token_program);

    return (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        operator,
        fee_recipient,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        fee_recipient_ata,
        user_share_account,
    );
}

pub fn set_share_balance(
    svm: &mut LiteSVM,
    user_share_account: &Pubkey,
    share_mint: &Pubkey,
    amount: u64,
) {
    let mut acct = svm.get_account(user_share_account).unwrap();
    let mut token_state = spl_token::state::Account::unpack(&acct.data).unwrap();
    token_state.amount = amount;
    spl_token::state::Account::pack(token_state, &mut acct.data).unwrap();
    svm.set_account(*user_share_account, acct).unwrap();

    let mut mint_acct = svm.get_account(share_mint).unwrap();
    let mut mint_state = spl_token::state::Mint::unpack(&mint_acct.data).unwrap();
    mint_state.supply = amount;
    spl_token::state::Mint::pack(mint_state, &mut mint_acct.data).unwrap();
    svm.set_account(*share_mint, mint_acct).unwrap();
}

/// Update Vault's `total_asset_balance`
pub fn set_vault_total_asset_balance(svm: &mut LiteSVM, vault: Pubkey, amount: u64) {
    let mut account = svm.get_account(&vault).unwrap();
    let mut vault_state = AsyncVault::from_bytes(account.data()).unwrap();
    vault_state.total_asset_balance = amount;
    let mut buf = Vec::new();
    vault_state.serialize(&mut buf).unwrap();
    let tlv_bytes = account.data()[buf.len()..].to_vec();
    buf.extend_from_slice(&tlv_bytes);
    account.data = buf;
    svm.set_account(vault, account).unwrap();
}

pub fn set_vault_pending_async_requests(svm: &mut LiteSVM, vault: Pubkey, count: u16) {
    let mut account = svm.get_account(&vault).unwrap();
    let mut vault_state = AsyncVault::from_bytes(account.data()).unwrap();
    vault_state.pending_async_requests = count;
    let mut buf = Vec::new();
    vault_state.serialize(&mut buf).unwrap();
    let tlv_bytes = account.data()[buf.len()..].to_vec();
    buf.extend_from_slice(&tlv_bytes);
    account.data = buf;
    svm.set_account(vault, account).unwrap();
}
