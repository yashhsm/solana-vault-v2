use anchor_spl::{associated_token::get_associated_token_address_with_program_id, token};
use async_vault_v2_client::{
    extensions::{redemption_queue, subscription_queue},
    lite::SendTransaction,
    sdk::program_id,
    ApproveRequestBuilder, CancelQueuedDepositRequestBuilder, ClaimBuilder,
    CreateDepositRequestBuilder, CreateRedeemRequestBuilder, InitializeRedemptionQueueBuilder,
    InitializeSubscriptionQueueBuilder, InitializeTranchesBuilder, InitializeVaultBuilder, Request,
    RequestArgs, RequestState, TrancheConfig, UpdateVaultNavBuilder, Vault,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount, clock::Clock, instruction::AccountMeta, pubkey::Pubkey,
    signature::Keypair, signer::Signer,
};

use crate::{
    async_helper_functions::{
        approve_request_args, assert_error_code, create_ata, create_mint, get_mint_authority,
        get_mint_supply, get_token_account_amount, helper_mint_to, set_share_balance,
        set_up_async_vault_v2, set_vault_total_asset_balance,
    },
    async_vault_v2::constants::{
        FEE_BPS_EXCEEDED, INVALID_SHARE_MINT, INVALID_TRANCHE_REQUEST_LIMIT_CONFIG, INVALID_VAULT,
        JUNIOR_RATIO_BELOW_MINIMUM, MISSING_REQUIRED_ACCOUNT, REDEMPTION_QUEUE_OUT_OF_ORDER,
        SHARE_MINT_SUPPLY_SHOULD_BE_ZERO, SUBSCRIPTION_QUEUE_OUT_OF_ORDER,
        TRANCHE_REQUEST_AMOUNT_ABOVE_MAXIMUM, TRANCHE_REQUEST_AMOUNT_BELOW_MINIMUM,
        UNAUTHORIZED_SIGNER, VAULT_ALREADY_INITIALIZED,
    },
};

const TRANCHE_CONFIG_SEED: &[u8] = b"tranches";
const NO_TRANCHE_REQUEST_LIMITS: [u64; 4] = [0; 4];

fn add_program(svm: &mut LiteSVM) {
    let program_bytes = include_bytes!("../../../target/deploy/async_vault_v2.so");
    svm.add_program(program_id(), program_bytes).unwrap();
}

fn tranche_config_address(vault: Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[TRANCHE_CONFIG_SEED, vault.as_ref()], &program_id()).0
}

fn tranche_remaining_accounts(
    tranche_config: Pubkey,
    senior_share_mint: Pubkey,
    junior_share_mint: Pubkey,
) -> [AccountMeta; 3] {
    [
        AccountMeta::new(tranche_config, false),
        AccountMeta::new_readonly(senior_share_mint, false),
        AccountMeta::new_readonly(junior_share_mint, false),
    ]
}

fn tranche_config_remaining_account(tranche_config: Pubkey) -> [AccountMeta; 1] {
    [AccountMeta::new_readonly(tranche_config, false)]
}

fn writable_tranche_config_remaining_account(tranche_config: Pubkey) -> [AccountMeta; 1] {
    [AccountMeta::new(tranche_config, false)]
}

#[allow(clippy::type_complexity)]
fn setup_tranche_queue_vault(
    with_subscription_queue: bool,
    with_redemption_queue: bool,
) -> (
    LiteSVM,
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
    Pubkey,
    Pubkey,
) {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

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
        pending_vault_pubkey,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000_000_000);

    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_user_share_account =
        create_ata(&mut svm, &user, &junior_share_mint.pubkey(), &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(0)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    if with_subscription_queue {
        InitializeSubscriptionQueueBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize_subscription_queue should succeed");
    }
    if with_redemption_queue {
        InitializeRedemptionQueueBuilder::new()
            .payer(authority.pubkey())
            .authority(authority.pubkey())
            .vault(vault_pubkey)
            .instruction()
            .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
            .expect("initialize_redemption_queue should succeed");
    }

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("tranche nav update should succeed");

    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    (
        svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        junior_share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        user_share_account,
        junior_user_share_account,
        tranche_config,
    )
}

fn create_tranche_deposit_request(
    svm: &mut LiteSVM,
    user: &Keypair,
    asset_mint: Pubkey,
    share_mint: Pubkey,
    vault_pubkey: Pubkey,
    user_token_account: Pubkey,
    pending_vault_pubkey: Pubkey,
    tranche_config: Pubkey,
    amount: u64,
) -> Keypair {
    let request = Keypair::new();
    svm.expire_blockhash();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .request(request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_token_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount,
            operator: None,
        })
        .add_remaining_accounts(&writable_tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(svm, &user.pubkey(), &[user, &request])
        .expect("create tranche deposit request should succeed");
    request
}

fn create_tranche_redeem_request(
    svm: &mut LiteSVM,
    user: &Keypair,
    asset_mint: Pubkey,
    share_mint: Pubkey,
    vault_pubkey: Pubkey,
    user_share_account: Pubkey,
    tranche_config: Pubkey,
    amount: u64,
) -> Keypair {
    let request = Keypair::new();
    svm.expire_blockhash();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .request(request.pubkey())
        .vault(vault_pubkey)
        .user_share_account(user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount,
            operator: None,
        })
        .add_remaining_accounts(&writable_tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(svm, &user.pubkey(), &[user, &request])
        .expect("create tranche redeem request should succeed");
    request
}

fn approve_tranche_request(
    svm: &mut LiteSVM,
    authority: &Keypair,
    asset_mint: Pubkey,
    share_mint: Pubkey,
    vault_pubkey: Pubkey,
    reserve_pubkey: Pubkey,
    pending_vault_pubkey: Pubkey,
    request: Pubkey,
    tranche_config: Pubkey,
) -> litesvm::types::TransactionResult {
    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(svm, &request);
    svm.expire_blockhash();
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(request)
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint)
        .share_mint(share_mint)
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&writable_tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(svm, &authority.pubkey(), &[authority])
}

#[test]
fn test_tranche_subscription_queue_lanes_advance_independently() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        share_mint,
        junior_share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        _user_share_account,
        _junior_user_share_account,
        tranche_config,
    ) = setup_tranche_queue_vault(true, false);

    let senior_request_1 = create_tranche_deposit_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        user_asset_account,
        pending_vault_pubkey,
        tranche_config,
        100,
    );
    let junior_request_1 = create_tranche_deposit_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        user_asset_account,
        pending_vault_pubkey,
        tranche_config,
        100,
    );
    let senior_request_2 = create_tranche_deposit_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        user_asset_account,
        pending_vault_pubkey,
        tranche_config,
        100,
    );

    let senior_id_1 = subscription_queue::get_request_state(
        svm.get_account(&senior_request_1.pubkey()).unwrap().data(),
    )
    .unwrap()
    .id;
    let junior_id_1 = subscription_queue::get_request_state(
        svm.get_account(&junior_request_1.pubkey()).unwrap().data(),
    )
    .unwrap()
    .id;
    let senior_id_2 = subscription_queue::get_request_state(
        svm.get_account(&senior_request_2.pubkey()).unwrap().data(),
    )
    .unwrap()
    .id;
    assert_eq!(senior_id_1, 1);
    assert_eq!(junior_id_1, 1);
    assert_eq!(senior_id_2, 2);

    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        junior_request_1.pubkey(),
        tranche_config,
    )
    .expect("junior request should not wait for senior lane");

    let err = approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        senior_request_2.pubkey(),
        tranche_config,
    )
    .unwrap_err();
    assert_error_code(
        &err,
        SUBSCRIPTION_QUEUE_OUT_OF_ORDER,
        "SubscriptionQueueOutOfOrder",
    );

    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        senior_request_1.pubkey(),
        tranche_config,
    )
    .expect("first senior request should advance senior lane");
    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        senior_request_2.pubkey(),
        tranche_config,
    )
    .expect("second senior request should succeed after senior lane advances");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_subscription_request_total, 2);
    assert_eq!(config.senior_subscription_request_last_processed, 2);
    assert_eq!(config.junior_subscription_request_total, 1);
    assert_eq!(config.junior_subscription_request_last_processed, 1);
}

#[test]
fn test_tranche_queued_deposit_cancel_and_skip_advance_selected_lane() {
    let (
        mut svm,
        authority,
        _mint_authority,
        asset_mint,
        _share_mint,
        junior_share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        user_asset_account,
        _user_share_account,
        _junior_user_share_account,
        tranche_config,
    ) = setup_tranche_queue_vault(true, false);

    let junior_request_1 = create_tranche_deposit_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        user_asset_account,
        pending_vault_pubkey,
        tranche_config,
        100,
    );
    let junior_request_2 = create_tranche_deposit_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        user_asset_account,
        pending_vault_pubkey,
        tranche_config,
        100,
    );

    svm.expire_blockhash();
    CancelQueuedDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .vault(vault_pubkey)
        .vault_asset(None)
        .request(junior_request_1.pubkey())
        .user_token_account(user_asset_account)
        .asset_pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("queued junior deposit cancellation should succeed");

    let canceled =
        Request::from_bytes(svm.get_account(&junior_request_1.pubkey()).unwrap().data()).unwrap();
    assert_eq!(canceled.request_state, RequestState::Canceled);

    svm.expire_blockhash();
    async_vault_v2_client::SkipCanceledQueueRequestBuilder::new()
        .vault(vault_pubkey)
        .request(junior_request_1.pubkey())
        .owner(user.pubkey())
        .add_remaining_accounts(&writable_tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("skip canceled junior request should advance junior lane");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.junior_subscription_request_last_processed, 1);
    assert_eq!(config.senior_subscription_request_last_processed, 0);

    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        junior_request_2.pubkey(),
        tranche_config,
    )
    .expect("second junior request should succeed after tombstone skip");
}

#[test]
fn test_tranche_redemption_queue_lanes_advance_independently() {
    let (
        mut svm,
        authority,
        mint_authority,
        asset_mint,
        share_mint,
        junior_share_mint,
        user,
        reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _user_asset_account,
        user_share_account,
        junior_user_share_account,
        tranche_config,
    ) = setup_tranche_queue_vault(false, true);

    set_share_balance(&mut svm, &user_share_account, &share_mint.pubkey(), 1_000);
    set_share_balance(
        &mut svm,
        &junior_user_share_account,
        &junior_share_mint.pubkey(),
        1_000,
    );
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        10_000,
        &token::ID,
    );
    set_vault_total_asset_balance(&mut svm, vault_pubkey, 10_000);

    let senior_request_1 = create_tranche_redeem_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        user_share_account,
        tranche_config,
        100,
    );
    let junior_request_1 = create_tranche_redeem_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        junior_user_share_account,
        tranche_config,
        100,
    );
    let senior_request_2 = create_tranche_redeem_request(
        &mut svm,
        &user,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        user_share_account,
        tranche_config,
        100,
    );

    let senior_id_1 = redemption_queue::get_request_state(
        svm.get_account(&senior_request_1.pubkey()).unwrap().data(),
    )
    .unwrap()
    .id;
    let junior_id_1 = redemption_queue::get_request_state(
        svm.get_account(&junior_request_1.pubkey()).unwrap().data(),
    )
    .unwrap()
    .id;
    let senior_id_2 = redemption_queue::get_request_state(
        svm.get_account(&senior_request_2.pubkey()).unwrap().data(),
    )
    .unwrap()
    .id;
    assert_eq!(senior_id_1, 1);
    assert_eq!(junior_id_1, 1);
    assert_eq!(senior_id_2, 2);

    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        junior_share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        junior_request_1.pubkey(),
        tranche_config,
    )
    .expect("junior redeem should not wait for senior lane");

    let err = approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        senior_request_2.pubkey(),
        tranche_config,
    )
    .unwrap_err();
    assert_error_code(
        &err,
        REDEMPTION_QUEUE_OUT_OF_ORDER,
        "RedemptionQueueOutOfOrder",
    );

    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        senior_request_1.pubkey(),
        tranche_config,
    )
    .expect("first senior redeem should advance senior lane");
    approve_tranche_request(
        &mut svm,
        &authority,
        asset_mint.pubkey(),
        share_mint.pubkey(),
        vault_pubkey,
        reserve_pubkey,
        pending_vault_pubkey,
        senior_request_2.pubkey(),
        tranche_config,
    )
    .expect("second senior redeem should succeed after senior lane advances");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_redemption_request_total, 2);
    assert_eq!(config.senior_redemption_request_last_processed, 2);
    assert_eq!(config.junior_redemption_request_total, 1);
    assert_eq!(config.junior_redemption_request_last_processed, 1);
}

#[test]
fn test_initialize_tranches_succeeds_before_vault_initialization() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    let tranche_config = tranche_config_address(vault_pubkey);
    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    let config_account = svm
        .get_account(&tranche_config)
        .expect("tranche config should exist");
    let config = TrancheConfig::from_bytes(config_account.data()).unwrap();
    assert_eq!(config.vault, vault_pubkey);
    assert_eq!(config.senior_share_mint, share_mint.pubkey());
    assert_eq!(config.junior_share_mint, junior_share_mint.pubkey());
    assert_eq!(config.senior_nav, 0);
    assert_eq!(config.junior_nav, 0);
    assert_eq!(config.senior_supply, 0);
    assert_eq!(config.junior_supply, 0);
    assert_eq!(config.senior_target_bps, 700);
    assert_eq!(config.min_junior_ratio_bps, 2_500);
    assert_eq!(config.min_request_amounts, NO_TRANCHE_REQUEST_LIMITS);
    assert_eq!(config.max_request_amounts, NO_TRANCHE_REQUEST_LIMITS);
    assert_eq!(config.senior_subscription_request_total, 0);
    assert_eq!(config.senior_subscription_request_last_processed, 0);
    assert_eq!(config.senior_redemption_request_total, 0);
    assert_eq!(config.senior_redemption_request_last_processed, 0);
    assert_eq!(config.junior_subscription_request_total, 0);
    assert_eq!(config.junior_subscription_request_last_processed, 0);
    assert_eq!(config.junior_redemption_request_total, 0);
    assert_eq!(config.junior_redemption_request_last_processed, 0);
    assert_eq!(config.last_waterfall_slot, 0);
    assert_eq!(config.last_waterfall_timestamp, 0);

    let senior_mint_account = svm.get_account(&share_mint.pubkey()).unwrap();
    let junior_mint_account = svm.get_account(&junior_share_mint.pubkey()).unwrap();
    assert_eq!(get_mint_authority(&senior_mint_account), Some(vault_pubkey));
    assert_eq!(get_mint_authority(&junior_mint_account), Some(vault_pubkey));

    let vault = Vault::from_bytes(svm.get_account(&vault_pubkey).unwrap().data()).unwrap();
    assert_eq!(vault.tranche_config, Some(tranche_config));
}

#[test]
fn test_initialize_tranches_rejects_non_curator() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        _authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000_000).unwrap();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(unauthorized.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &unauthorized],
        );

    assert_error_code(
        &result.unwrap_err(),
        UNAUTHORIZED_SIGNER,
        "UnauthorizedSigner",
    );
}

#[test]
fn test_initialize_tranches_after_vault_initialization_fails() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");
    svm.expire_blockhash();

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );

    assert_error_code(
        &result.unwrap_err(),
        VAULT_ALREADY_INITIALIZED,
        "VaultAlreadyInitialized",
    );
}

#[test]
fn test_initialize_tranches_rejects_invalid_mints() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
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

    let same_mints_result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );
    let same_mints_error = same_mints_result.unwrap_err();
    assert!(
        format!("{same_mints_error:?}").contains("ConstraintDuplicateMutableAccount"),
        "expected duplicate mutable account guard, got: {same_mints_error:?}"
    );

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(asset_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );
    assert_error_code(&result.unwrap_err(), INVALID_SHARE_MINT, "InvalidShareMint");
}

#[test]
fn test_initialize_tranches_requires_existing_share_mint_in_pair() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let senior_share_mint = Keypair::new();
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &senior_share_mint, &token::ID);
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(senior_share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );

    assert_error_code(&result.unwrap_err(), INVALID_SHARE_MINT, "InvalidShareMint");
}

#[test]
fn test_initialize_tranches_rejects_nonzero_tranche_mint_supply() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_holder = create_ata(&mut svm, &payer, &junior_share_mint.pubkey(), &token::ID);
    helper_mint_to(
        &mut svm,
        &junior_share_mint.pubkey(),
        &junior_holder,
        &mint_authority,
        1,
        &token::ID,
    );

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );

    assert_error_code(
        &result.unwrap_err(),
        SHARE_MINT_SUPPLY_SHOULD_BE_ZERO,
        "Share mint supply should be zero.",
    );
}

#[test]
fn test_initialize_tranches_rejects_invalid_bps_config() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(10_001)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );

    assert_error_code(&result.unwrap_err(), FEE_BPS_EXCEEDED, "FeeBpsExceeded");
}

#[test]
fn test_initialize_tranches_rejects_invalid_request_limit_config() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    let result = InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts([100, 0, 0, 0])
        .max_request_amounts([99, 0, 0, 0])
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        );

    assert_error_code(
        &result.unwrap_err(),
        INVALID_TRANCHE_REQUEST_LIMIT_CONFIG,
        "InvalidTrancheRequestLimitConfig",
    );
}

#[test]
fn test_senior_tranche_deposit_respects_request_minimum() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts([100, 0, 0, 0])
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );
    let below_min_request = Keypair::new();
    svm.expire_blockhash();
    let below_min = CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(below_min_request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 99,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &below_min_request]);
    assert_error_code(
        &below_min.unwrap_err(),
        TRANCHE_REQUEST_AMOUNT_BELOW_MINIMUM,
        "TrancheRequestAmountBelowMinimum",
    );

    let boundary_request = Keypair::new();
    svm.expire_blockhash();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(boundary_request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 100,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &boundary_request])
        .expect("senior deposit at tranche minimum should succeed");
}

#[test]
fn test_junior_tranche_redeem_respects_request_maximum() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        _pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_user_share_account =
        create_ata(&mut svm, &user, &junior_share_mint.pubkey(), &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(700)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts([0, 0, 0, 50])
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    set_share_balance(
        &mut svm,
        &junior_user_share_account,
        &junior_share_mint.pubkey(),
        60,
    );

    let above_max_request = Keypair::new();
    svm.expire_blockhash();
    let above_max = CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .request(above_max_request.pubkey())
        .vault(vault_pubkey)
        .user_share_account(junior_user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 51,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &above_max_request]);
    assert_error_code(
        &above_max.unwrap_err(),
        TRANCHE_REQUEST_AMOUNT_ABOVE_MAXIMUM,
        "TrancheRequestAmountAboveMaximum",
    );
    assert_eq!(
        get_token_account_amount(&svm.get_account(&junior_user_share_account).unwrap()),
        60,
        "failed redeem request should not burn shares"
    );

    let boundary_request = Keypair::new();
    svm.expire_blockhash();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .request(boundary_request.pubkey())
        .vault(vault_pubkey)
        .user_share_account(junior_user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 50,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &boundary_request])
        .expect("junior redeem at tranche maximum should succeed");
    assert_eq!(
        get_token_account_amount(&svm.get_account(&junior_user_share_account).unwrap()),
        10
    );
}

#[test]
fn test_tranche_enabled_nav_update_requires_tranche_accounts() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config_address(vault_pubkey))
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    svm.expire_blockhash();
    let err = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();

    assert_error_code(&err, MISSING_REQUIRED_ACCOUNT, "MissingRequiredAccount");
}

#[test]
fn test_tranche_nav_update_rejects_mismatched_remaining_accounts() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    let (
        other_authority,
        other_payer,
        other_mint_authority,
        _other_asset_mint,
        other_share_mint,
        _other_user,
        _other_operator,
        _other_fee_recipient,
        _other_reserve_pubkey,
        other_vault_pubkey,
        _other_pending_vault_pubkey,
        _other_fee_recipient_ata,
        _other_user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let other_junior_share_mint = Keypair::new();
    create_mint(
        &mut svm,
        &other_mint_authority,
        &other_junior_share_mint,
        &token::ID,
    );
    let other_tranche_config = tranche_config_address(other_vault_pubkey);
    InitializeTranchesBuilder::new()
        .payer(other_payer.pubkey())
        .mint_authority(other_mint_authority.pubkey())
        .authority(other_authority.pubkey())
        .vault(other_vault_pubkey)
        .senior_share_mint(other_share_mint.pubkey())
        .junior_share_mint(other_junior_share_mint.pubkey())
        .tranche_config(other_tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &other_payer.pubkey(),
            &[&other_payer, &other_mint_authority, &other_authority],
        )
        .expect("other tranche initialization should succeed");

    svm.expire_blockhash();
    let wrong_config_err = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            other_tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&wrong_config_err, INVALID_VAULT, "InvalidVault");

    svm.expire_blockhash();
    let swapped_mints_err = UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            junior_share_mint.pubkey(),
            share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&swapped_mints_err, INVALID_SHARE_MINT, "InvalidShareMint");
}

#[test]
fn test_tranche_waterfall_initializes_navs_and_supplies() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    let junior_holder = create_ata(&mut svm, &payer, &junior_share_mint.pubkey(), &token::ID);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );
    set_share_balance(
        &mut svm,
        &junior_holder,
        &junior_share_mint.pubkey(),
        1_000_000_000,
    );

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_nav, 1_000_000_000);
    assert_eq!(config.junior_nav, 1_000_000_000);
    assert_eq!(config.senior_supply, 1_000_000_000);
    assert_eq!(config.junior_supply, 1_000_000_000);
}

#[test]
fn test_tranche_waterfall_credits_senior_target_then_junior_residual() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    let junior_holder = create_ata(&mut svm, &payer, &junior_share_mint.pubkey(), &token::ID);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );
    set_share_balance(
        &mut svm,
        &junior_holder,
        &junior_share_mint.pubkey(),
        1_000_000_000,
    );

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = 10;
    clock.unix_timestamp = 1;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = 11;
    clock.unix_timestamp = 1 + 31_536_000;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_300_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("gain tranche nav update should succeed");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_nav, 1_100_000_000);
    assert_eq!(config.junior_nav, 1_500_000_000);
}

#[test]
fn test_tranche_waterfall_losses_hit_junior_before_senior() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        payer,
        mint_authority,
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
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    let junior_holder = create_ata(&mut svm, &payer, &junior_share_mint.pubkey(), &token::ID);
    set_share_balance(
        &mut svm,
        &user_share_account,
        &share_mint.pubkey(),
        1_000_000_000,
    );
    set_share_balance(
        &mut svm,
        &junior_holder,
        &junior_share_mint.pubkey(),
        1_000_000_000,
    );

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = 10;
    clock.unix_timestamp = 1;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let mut clock = svm.get_sysvar::<Clock>();
    clock.slot = 11;
    clock.unix_timestamp = 2;
    svm.set_sysvar(&clock);
    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(400_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("loss tranche nav update should succeed");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_nav, 800_000_000);
    assert_eq!(config.junior_nav, 0);
}

#[test]
fn test_junior_tranche_deposit_and_redeem_lifecycle_uses_junior_share_mint() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

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
        pending_vault_pubkey,
        _fee_recipient_ata,
        senior_user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_user_share_account =
        create_ata(&mut svm, &user, &junior_share_mint.pubkey(), &token::ID);
    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(1_000)
        .min_junior_ratio_bps(2_500)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let deposit_request = Keypair::new();
    svm.expire_blockhash();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .request(deposit_request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 100,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &deposit_request])
        .expect("junior deposit request should succeed");

    let request =
        Request::from_bytes(svm.get_account(&deposit_request.pubkey()).unwrap().data()).unwrap();
    assert_eq!(request.share_mint_address, junior_share_mint.pubkey());

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &deposit_request.pubkey());
    svm.expire_blockhash();
    let wrong_share_approval = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(deposit_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority]);
    assert_error_code(
        &wrong_share_approval.unwrap_err(),
        INVALID_SHARE_MINT,
        "InvalidShareMint",
    );

    svm.expire_blockhash();
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(deposit_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("junior deposit approval should succeed");

    let junior_supply_before =
        get_mint_supply(&svm.get_account(&junior_share_mint.pubkey()).unwrap());
    let senior_supply_before = get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap());
    svm.expire_blockhash();
    let wrong_share_claim = ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .vault(vault_pubkey)
        .request(deposit_request.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .pending_vault(None)
        .user_share_account(Some(senior_user_share_account))
        .user_asset_account(None)
        .asset_token_program(token::ID)
        .share_token_program(Some(token::ID))
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user]);
    assert_error_code(
        &wrong_share_claim.unwrap_err(),
        INVALID_SHARE_MINT,
        "InvalidShareMint",
    );

    svm.expire_blockhash();
    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .vault(vault_pubkey)
        .request(deposit_request.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .pending_vault(None)
        .user_share_account(Some(junior_user_share_account))
        .user_asset_account(None)
        .asset_token_program(token::ID)
        .share_token_program(Some(token::ID))
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("junior deposit claim should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&junior_user_share_account).unwrap()),
        100
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&junior_share_mint.pubkey()).unwrap()),
        junior_supply_before + 100
    );
    assert_eq!(
        get_mint_supply(&svm.get_account(&share_mint.pubkey()).unwrap()),
        senior_supply_before,
        "senior/base mint supply should not change for a junior deposit"
    );

    let redeem_request = Keypair::new();
    svm.expire_blockhash();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .request(redeem_request.pubkey())
        .vault(vault_pubkey)
        .user_share_account(junior_user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 40,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &redeem_request])
        .expect("junior redeem request should succeed");

    let request =
        Request::from_bytes(svm.get_account(&redeem_request.pubkey()).unwrap().data()).unwrap();
    assert_eq!(request.share_mint_address, junior_share_mint.pubkey());
    assert_eq!(
        get_token_account_amount(&svm.get_account(&junior_user_share_account).unwrap()),
        60,
        "redeem request should burn junior shares"
    );

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &redeem_request.pubkey());
    svm.expire_blockhash();
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(redeem_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("junior redeem approval should succeed");

    let user_assets_before =
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap());
    svm.expire_blockhash();
    ClaimBuilder::new()
        .user(user.pubkey())
        .owner(user.pubkey())
        .vault(vault_pubkey)
        .request(redeem_request.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .pending_vault(Some(pending_vault_pubkey))
        .user_share_account(None)
        .user_asset_account(Some(user_asset_account))
        .asset_token_program(token::ID)
        .share_token_program(None)
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user])
        .expect("junior redeem claim should succeed");

    assert_eq!(
        get_token_account_amount(&svm.get_account(&user_asset_account).unwrap()),
        user_assets_before + 40
    );
}

#[test]
fn test_senior_deposit_rejected_when_junior_ratio_floor_would_break() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

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
        pending_vault_pubkey,
        _fee_recipient_ata,
        senior_user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_holder = create_ata(&mut svm, &payer, &junior_share_mint.pubkey(), &token::ID);
    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(0)
        .min_junior_ratio_bps(5_000)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    set_share_balance(
        &mut svm,
        &senior_user_share_account,
        &share_mint.pubkey(),
        100,
    );
    set_share_balance(&mut svm, &junior_holder, &junior_share_mint.pubkey(), 100);

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let deposit_request = Keypair::new();
    svm.expire_blockhash();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(deposit_request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 100,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &deposit_request])
        .expect("senior deposit request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &deposit_request.pubkey());
    svm.expire_blockhash();
    let err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(deposit_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, JUNIOR_RATIO_BELOW_MINIMUM, "JuniorRatioBelowMinimum");
}

#[test]
fn test_senior_deposit_guard_counts_approved_unclaimed_senior_shares() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

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
        pending_vault_pubkey,
        _fee_recipient_ata,
        senior_user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 1_000);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_holder = create_ata(&mut svm, &payer, &junior_share_mint.pubkey(), &token::ID);
    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(0)
        .min_junior_ratio_bps(5_000)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    set_share_balance(
        &mut svm,
        &senior_user_share_account,
        &share_mint.pubkey(),
        100,
    );
    set_share_balance(&mut svm, &junior_holder, &junior_share_mint.pubkey(), 200);

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let first_request = Keypair::new();
    svm.expire_blockhash();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(first_request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 100,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &first_request])
        .expect("first senior deposit request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &first_request.pubkey());
    svm.expire_blockhash();
    ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(first_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("first senior approval should land exactly on the junior floor");

    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_supply, 200);

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("nav update should preserve committed senior supply");
    let config =
        TrancheConfig::from_bytes(svm.get_account(&tranche_config).unwrap().data()).unwrap();
    assert_eq!(config.senior_supply, 200);

    let second_request = Keypair::new();
    svm.expire_blockhash();
    CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .request(second_request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 100,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &second_request])
        .expect("second senior deposit request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &second_request.pubkey());
    svm.expire_blockhash();
    let err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(second_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, JUNIOR_RATIO_BELOW_MINIMUM, "JuniorRatioBelowMinimum");
}

#[test]
fn test_junior_redeem_rejected_when_junior_ratio_floor_would_break() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

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
        pending_vault_pubkey,
        _fee_recipient_ata,
        senior_user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);
    let junior_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &junior_share_mint, &token::ID);
    let junior_user_share_account =
        create_ata(&mut svm, &user, &junior_share_mint.pubkey(), &token::ID);
    let tranche_config = tranche_config_address(vault_pubkey);

    InitializeTranchesBuilder::new()
        .payer(payer.pubkey())
        .mint_authority(mint_authority.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .senior_share_mint(share_mint.pubkey())
        .junior_share_mint(junior_share_mint.pubkey())
        .tranche_config(tranche_config)
        .senior_share_token_program(token::ID)
        .junior_share_token_program(token::ID)
        .senior_target_bps(0)
        .min_junior_ratio_bps(5_000)
        .min_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .max_request_amounts(NO_TRANCHE_REQUEST_LIMITS)
        .instruction()
        .send_transaction(
            &mut svm,
            &payer.pubkey(),
            &[&payer, &mint_authority, &authority],
        )
        .expect("tranche initialization should succeed");

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    set_share_balance(
        &mut svm,
        &senior_user_share_account,
        &share_mint.pubkey(),
        100,
    );
    set_share_balance(
        &mut svm,
        &junior_user_share_account,
        &junior_share_mint.pubkey(),
        100,
    );
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve_pubkey,
        &mint_authority,
        200,
        &token::ID,
    );
    set_vault_total_asset_balance(&mut svm, vault_pubkey, 200);

    svm.expire_blockhash();
    UpdateVaultNavBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .updated_nav(1_000_000_000)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("initial tranche nav update should succeed");

    let redeem_request = Keypair::new();
    svm.expire_blockhash();
    CreateRedeemRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .request(redeem_request.pubkey())
        .vault(vault_pubkey)
        .user_share_account(junior_user_share_account)
        .share_token_program(token::ID)
        .args(RequestArgs {
            amount: 1,
            operator: None,
        })
        .add_remaining_accounts(&tranche_config_remaining_account(tranche_config))
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &redeem_request])
        .expect("junior redeem request should succeed");

    let (owner, request_type, amount, created_at, nav_update_version) =
        approve_request_args(&svm, &redeem_request.pubkey());
    svm.expire_blockhash();
    let err = ApproveRequestBuilder::new()
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .request(redeem_request.pubkey())
        .owner(owner)
        .request_type(request_type)
        .amount(amount)
        .created_at(created_at)
        .nav_update_version(nav_update_version)
        .asset_mint(asset_mint.pubkey())
        .share_mint(junior_share_mint.pubkey())
        .vault_token_account(reserve_pubkey)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .add_remaining_accounts(&tranche_remaining_accounts(
            tranche_config,
            share_mint.pubkey(),
            junior_share_mint.pubkey(),
        ))
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .unwrap_err();
    assert_error_code(&err, JUNIOR_RATIO_BELOW_MINIMUM, "JuniorRatioBelowMinimum");
}

#[test]
fn test_non_tranche_vault_rejects_non_base_share_mint_request() {
    let mut svm = LiteSVM::new();
    add_program(&mut svm);

    let (
        authority,
        _payer,
        mint_authority,
        asset_mint,
        share_mint,
        user,
        _operator,
        _fee_recipient,
        _reserve_pubkey,
        vault_pubkey,
        pending_vault_pubkey,
        _fee_recipient_ata,
        _user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 100);
    let stray_share_mint = Keypair::new();
    create_mint(&mut svm, &mint_authority, &stray_share_mint, &token::ID);
    let user_asset_account = get_associated_token_address_with_program_id(
        &user.pubkey(),
        &asset_mint.pubkey(),
        &token::ID,
    );

    InitializeVaultBuilder::new()
        .share_mint(share_mint.pubkey())
        .authority(authority.pubkey())
        .vault(vault_pubkey)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");

    let request = Keypair::new();
    let result = CreateDepositRequestBuilder::new()
        .user(user.pubkey())
        .asset_mint(asset_mint.pubkey())
        .share_mint(stray_share_mint.pubkey())
        .request(request.pubkey())
        .vault(vault_pubkey)
        .user_token_account(user_asset_account)
        .pending_vault(pending_vault_pubkey)
        .asset_token_program(token::ID)
        .args(RequestArgs {
            amount: 10,
            operator: None,
        })
        .instruction()
        .send_transaction(&mut svm, &user.pubkey(), &[&user, &request]);

    assert_error_code(&result.unwrap_err(), INVALID_SHARE_MINT, "InvalidShareMint");
}
