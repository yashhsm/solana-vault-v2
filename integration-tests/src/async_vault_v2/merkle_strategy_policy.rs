use anchor_spl::token;
use async_vault_v2_client::{
    lite::SendTransaction,
    merkle_strategy_policy::{
        build_strategy_policy_merkle_tree, hash_strategy_policy_leaf,
        hash_token_balance_adapter_leaf, normalize_policy_accounts, strategy_policy_proof,
        StrategyPolicyAccountMeta, StrategyPolicyLeaf, StrategyPolicyMerkleTree,
        TokenBalanceAdapterLeaf,
    },
    sdk::program_id,
    AddVaultAssetBuilder, ApproveVaultVenueBuilder, CancelStrategyPolicyUpdateBuilder,
    CloseStrategyPolicyBuilder, CreateVenuePositionBuilder, ExecuteStrategyPolicyUpdateBuilder,
    InitializeStrategyPolicyBuilder, InitializeVaultBuilder,
    ManageVaultWithMerkleVerificationBuilder, ManageVaultWithTokenBalanceAdapterBuilder,
    PauseStrategyPolicyBuilder, PendingStrategyPolicyUpdate, PolicyOperator, Position,
    QueueStrategyPolicyUpdateBuilder, RegisterVenueBuilder, StrategyPolicy,
    StrategyPolicyUpdateArgs, TokenBalanceAdapterAction, UpdateStrategyPolicyBuilder,
    UpdateVaultBuilder, Vault, VaultAsset, VenueType,
};
use litesvm::LiteSVM;
use solana_sdk::{
    account::ReadableAccount,
    clock::Clock,
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
};

use crate::{
    async_helper_functions::{
        assert_error_code, create_ata, create_mint, get_token_account_amount, helper_mint_to,
        set_up_async_vault_v2,
    },
    async_vault_v2::constants::{
        INVALID_STRATEGY_ADAPTER, MERKLE_PROOF_INVALID, POSITION_ACCOUNTING_MISMATCH,
        REENTRANT_STRATEGY_CALL, ROLLING_LIMIT_EXCEEDED, SHARE_SUPPLY_CHANGED,
        STALE_STRATEGY_POLICY_VERSION, STRATEGY_ADAPTER_AMOUNT_EXCEEDED, STRATEGY_ADAPTER_REQUIRED,
        STRATEGY_POLICY_PAUSED, TIMELOCK_NOT_READY, TIMELOCK_REQUIRED,
    },
};

const STRATEGY_POLICY_SEED: &[u8] = b"strategy_policy";
const VENUE_ENTRY_SEED: &[u8] = b"venue";
const VAULT_VENUE_SEED: &[u8] = b"vault_venue";
const POSITION_SEED: &[u8] = b"position";
const POSITION_TOKEN_SEED: &[u8] = b"position_token";
const ASSET_CONFIG_SEED: &[u8] = b"asset";
const ASSET_RESERVE_SEED: &[u8] = b"asset_reserve";
const ASSET_PENDING_SEED: &[u8] = b"asset_pending";
const FIRST_AMOUNT: u64 = 250;
const SECOND_AMOUNT: u64 = 100;
const DEPLOY_POLICY_MAX_AMOUNT: u64 = 300;
const PULL_POLICY_MAX_AMOUNT: u64 = 200;

struct PolicyFixture {
    svm: LiteSVM,
    authority: Keypair,
    payer: Keypair,
    asset_mint: Keypair,
    share_mint: Keypair,
    reserve: Pubkey,
    vault: Pubkey,
    recipient_token_account: Pubkey,
    user_share_account: Pubkey,
    venue_entry: Pubkey,
    vault_venue: Pubkey,
    strategy_policy: Pubkey,
    position: Pubkey,
    position_token_account: Pubkey,
    operators: Vec<PolicyOperator>,
    first_transfer: Instruction,
    second_transfer: Instruction,
    mint_shares: Instruction,
    mint_operators: Vec<PolicyOperator>,
    tree: StrategyPolicyMerkleTree,
    first_leaf: [u8; 32],
    second_leaf: [u8; 32],
    mint_leaf: [u8; 32],
    deploy_leaf: [u8; 32],
    pull_leaf: [u8; 32],
}

fn setup_policy_fixture() -> PolicyFixture {
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
        reserve,
        vault,
        _pending_vault,
        _fee_recipient_ata,
        user_share_account,
    ) = set_up_async_vault_v2(&mut svm, token::ID, None, token::ID, 0);

    UpdateVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault)
        .rolling_limit_window_slots(1_000)
        .manager_rolling_limit(1_000)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("manager rolling limit should be configured");
    InitializeVaultBuilder::new()
        .authority(authority.pubkey())
        .share_mint(share_mint.pubkey())
        .vault(vault)
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("vault initialization should succeed");
    helper_mint_to(
        &mut svm,
        &asset_mint.pubkey(),
        &reserve,
        &mint_authority,
        1_000,
        &token::ID,
    );

    let recipient = Keypair::new();
    svm.airdrop(&recipient.pubkey(), 1_000_000_000).unwrap();
    let recipient_token_account =
        create_ata(&mut svm, &recipient, &asset_mint.pubkey(), &token::ID);
    let first_transfer = transfer_instruction(
        reserve,
        asset_mint.pubkey(),
        recipient_token_account,
        vault,
        FIRST_AMOUNT,
    );
    let second_transfer = transfer_instruction(
        reserve,
        asset_mint.pubkey(),
        recipient_token_account,
        vault,
        SECOND_AMOUNT,
    );
    let mint_shares = anchor_spl::token::spl_token::instruction::mint_to(
        &token::ID,
        &share_mint.pubkey(),
        &user_share_account,
        &vault,
        &[],
        10,
    )
    .unwrap();

    let venue_id = [42_u8; 32];
    let venue_entry = Pubkey::find_program_address(
        &[
            VENUE_ENTRY_SEED,
            authority.pubkey().as_ref(),
            venue_id.as_ref(),
        ],
        &program_id(),
    )
    .0;
    let mut allowed_discriminators = [0_u8; 64];
    allowed_discriminators[..8].copy_from_slice(&first_transfer.data[..8]);
    allowed_discriminators[8..16].copy_from_slice(&second_transfer.data[..8]);
    allowed_discriminators[16..24].copy_from_slice(&mint_shares.data[..8]);
    RegisterVenueBuilder::new()
        .payer(payer.pubkey())
        .registry_authority(authority.pubkey())
        .venue_entry(venue_entry)
        .venue_id(venue_id)
        .target_program(token::ID)
        .allowed_discriminator_count(3)
        .allowed_discriminators(allowed_discriminators)
        .risk_class(1)
        .venue_type(VenueType::ExternalProtocol)
        .routine_safe(true)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("venue registration should succeed");

    let vault_venue = Pubkey::find_program_address(
        &[VAULT_VENUE_SEED, vault.as_ref(), venue_entry.as_ref()],
        &program_id(),
    )
    .0;
    ApproveVaultVenueBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .recipient_authority(authority.pubkey())
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("vault venue approval should succeed");

    let position = Pubkey::find_program_address(
        &[
            POSITION_SEED,
            vault.as_ref(),
            venue_entry.as_ref(),
            asset_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    let position_token_account = Pubkey::find_program_address(
        &[
            POSITION_TOKEN_SEED,
            vault.as_ref(),
            venue_entry.as_ref(),
            asset_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    CreateVenuePositionBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .asset_mint(asset_mint.pubkey())
        .vault_asset(None)
        .venue_entry(venue_entry)
        .vault_venue(vault_venue)
        .position(position)
        .position_token_account(position_token_account)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("canonical strategy position should be created");

    let strategy_policy = Pubkey::find_program_address(
        &[
            STRATEGY_POLICY_SEED,
            vault.as_ref(),
            authority.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    InitializeStrategyPolicyBuilder::new()
        .payer(payer.pubkey())
        .authority(authority.pubkey())
        .vault(vault)
        .strategy_policy(strategy_policy)
        .strategist(authority.pubkey())
        .instruction()
        .send_transaction(&mut svm, &payer.pubkey(), &[&payer, &authority])
        .expect("strategy policy initialization should succeed");

    let operators = vec![
        PolicyOperator::IngestInstruction {
            offset: 8,
            length: 2,
        },
        PolicyOperator::IngestAccount { index: 0 },
        PolicyOperator::IngestAccount { index: 1 },
        PolicyOperator::IngestAccount { index: 2 },
        PolicyOperator::IngestAccount { index: 3 },
        PolicyOperator::ManagerLimitAmount { offset: 1 },
    ];
    let first_leaf = policy_leaf(vault, authority.pubkey(), &first_transfer, &operators);
    let second_leaf = policy_leaf(vault, authority.pubkey(), &second_transfer, &operators);
    let mint_operators = vec![
        PolicyOperator::IngestInstruction {
            offset: 8,
            length: 1,
        },
        PolicyOperator::IngestAccount { index: 0 },
        PolicyOperator::IngestAccount { index: 1 },
        PolicyOperator::IngestAccount { index: 2 },
    ];
    let mint_leaf = policy_leaf(vault, authority.pubkey(), &mint_shares, &mint_operators);
    let deploy_leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf::for_async_vault(
        vault,
        authority.pubkey(),
        1,
        venue_entry,
        vault_venue,
        token::ID,
        TokenBalanceAdapterAction::Deploy,
        asset_mint.pubkey(),
        reserve,
        position,
        position_token_account,
        DEPLOY_POLICY_MAX_AMOUNT,
    ))
    .unwrap();
    let pull_leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf::for_async_vault(
        vault,
        authority.pubkey(),
        1,
        venue_entry,
        vault_venue,
        token::ID,
        TokenBalanceAdapterAction::Pull,
        asset_mint.pubkey(),
        reserve,
        position,
        position_token_account,
        PULL_POLICY_MAX_AMOUNT,
    ))
    .unwrap();
    let tree = build_strategy_policy_merkle_tree(vec![
        first_leaf,
        second_leaf,
        mint_leaf,
        deploy_leaf,
        pull_leaf,
    ])
    .unwrap();
    UpdateStrategyPolicyBuilder::new()
        .authority(authority.pubkey())
        .vault(vault)
        .strategy_policy(strategy_policy)
        .args(StrategyPolicyUpdateArgs {
            merkle_root: tree.root,
            paused: false,
        })
        .instruction()
        .send_transaction(&mut svm, &authority.pubkey(), &[&authority])
        .expect("strategy root activation should succeed");

    PolicyFixture {
        svm,
        authority,
        payer,
        asset_mint,
        share_mint,
        reserve,
        vault,
        recipient_token_account,
        user_share_account,
        venue_entry,
        vault_venue,
        strategy_policy,
        position,
        position_token_account,
        operators,
        first_transfer,
        second_transfer,
        mint_shares,
        mint_operators,
        tree,
        first_leaf,
        second_leaf,
        mint_leaf,
        deploy_leaf,
        pull_leaf,
    }
}

fn transfer_instruction(
    source: Pubkey,
    mint: Pubkey,
    destination: Pubkey,
    authority: Pubkey,
    amount: u64,
) -> Instruction {
    anchor_spl::token::spl_token::instruction::transfer_checked(
        &token::ID,
        &source,
        &mint,
        &destination,
        &authority,
        &[],
        amount,
        9,
    )
    .unwrap()
}

fn outer_account_metas(instruction: &Instruction, vault: Pubkey) -> Vec<AccountMeta> {
    instruction
        .accounts
        .iter()
        .map(|account| {
            if account.pubkey == vault {
                AccountMeta::new_readonly(vault, false)
            } else {
                account.clone()
            }
        })
        .collect()
}

fn policy_leaf(
    vault: Pubkey,
    strategist: Pubkey,
    instruction: &Instruction,
    operators: &[PolicyOperator],
) -> [u8; 32] {
    let outer_accounts = outer_account_metas(instruction, vault);
    let accounts: Vec<StrategyPolicyAccountMeta> = outer_accounts
        .iter()
        .map(|account| StrategyPolicyAccountMeta {
            address: account.pubkey,
            is_signer: account.is_signer,
            is_writable: account.is_writable,
        })
        .collect();
    let accounts = normalize_policy_accounts(vault, &accounts);
    hash_strategy_policy_leaf(StrategyPolicyLeaf::for_async_vault(
        vault,
        strategist,
        1,
        token::ID,
        &instruction.data,
        &accounts,
        operators,
    ))
    .unwrap()
}

fn proof_for(tree: &StrategyPolicyMerkleTree, leaf: [u8; 32]) -> Vec<[u8; 32]> {
    let index = tree
        .leaves
        .iter()
        .position(|candidate| *candidate == leaf)
        .unwrap();
    strategy_policy_proof(tree, index).unwrap()
}

fn manage_transfer(
    fixture: &mut PolicyFixture,
    transfer: &Instruction,
    proof: Vec<[u8; 32]>,
) -> litesvm::types::TransactionResult {
    let operators = fixture.operators.clone();
    manage_instruction(fixture, transfer, operators, proof)
}

fn manage_instruction(
    fixture: &mut PolicyFixture,
    instruction: &Instruction,
    operators: Vec<PolicyOperator>,
    proof: Vec<[u8; 32]>,
) -> litesvm::types::TransactionResult {
    let remaining_accounts = outer_account_metas(instruction, fixture.vault);
    ManageVaultWithMerkleVerificationBuilder::new()
        .strategist(fixture.authority.pubkey())
        .share_mint(fixture.share_mint.pubkey())
        .vault(fixture.vault)
        .venue_entry(fixture.venue_entry)
        .vault_venue(fixture.vault_venue)
        .strategy_policy(fixture.strategy_policy)
        .target_program(token::ID)
        .policy_version(1)
        .instruction_data(instruction.data.clone())
        .operators(operators)
        .proof(proof)
        .add_remaining_accounts(&remaining_accounts)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
}

fn manage_token_balance_adapter(
    fixture: &mut PolicyFixture,
    action: TokenBalanceAdapterAction,
    amount: u64,
    policy_max_amount: u64,
    proof: Vec<[u8; 32]>,
) -> litesvm::types::TransactionResult {
    ManageVaultWithTokenBalanceAdapterBuilder::new()
        .strategist(fixture.authority.pubkey())
        .share_mint(fixture.share_mint.pubkey())
        .vault(fixture.vault)
        .asset_mint(fixture.asset_mint.pubkey())
        .vault_asset(None)
        .venue_entry(fixture.venue_entry)
        .vault_venue(fixture.vault_venue)
        .strategy_policy(fixture.strategy_policy)
        .position(fixture.position)
        .vault_token_account(fixture.reserve)
        .position_token_account(fixture.position_token_account)
        .asset_token_program(token::ID)
        .policy_version(1)
        .action(action)
        .amount(amount)
        .policy_max_amount(policy_max_amount)
        .proof(proof)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
}

fn read_position(fixture: &PolicyFixture) -> Position {
    Position::from_bytes(
        fixture
            .svm
            .get_account(&fixture.position)
            .expect("position should exist")
            .data(),
    )
    .unwrap()
}

#[test]
fn generic_policy_cannot_write_vault_owned_token_accounts() {
    let mut fixture = setup_policy_fixture();
    let first_transfer = fixture.first_transfer.clone();
    let proof = proof_for(&fixture.tree, fixture.first_leaf);
    let err = manage_transfer(&mut fixture, &first_transfer, proof).unwrap_err();
    assert_error_code(&err, STRATEGY_ADAPTER_REQUIRED, "StrategyAdapterRequired");

    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        1_000
    );
    assert_eq!(
        get_token_account_amount(
            &fixture
                .svm
                .get_account(&fixture.recipient_token_account)
                .unwrap()
        ),
        0
    );
    let vault = Vault::from_bytes(fixture.svm.get_account(&fixture.vault).unwrap().data()).unwrap();
    assert_eq!(vault.manager_window_amount, 0);
    let policy = StrategyPolicy::from_bytes(
        fixture
            .svm
            .get_account(&fixture.strategy_policy)
            .unwrap()
            .data(),
    )
    .unwrap();
    assert!(!policy.executing);
}

#[test]
fn changed_cpi_account_is_rejected_by_the_merkle_proof() {
    let mut fixture = setup_policy_fixture();
    let other_recipient = Keypair::new();
    fixture
        .svm
        .airdrop(&other_recipient.pubkey(), 1_000_000_000)
        .unwrap();
    let other_token_account = create_ata(
        &mut fixture.svm,
        &other_recipient,
        &fixture.asset_mint.pubkey(),
        &token::ID,
    );
    let changed_transfer = transfer_instruction(
        fixture.reserve,
        fixture.asset_mint.pubkey(),
        other_token_account,
        fixture.vault,
        FIRST_AMOUNT,
    );
    let proof = proof_for(&fixture.tree, fixture.first_leaf);
    let err = manage_transfer(&mut fixture, &changed_transfer, proof).unwrap_err();
    assert_error_code(&err, MERKLE_PROOF_INVALID, "MerkleProofInvalid");
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        1_000
    );
}

#[test]
fn paused_policy_blocks_an_otherwise_valid_proof() {
    let mut fixture = setup_policy_fixture();
    PauseStrategyPolicyBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .expect("breaker-compatible pause should succeed");

    let first_transfer = fixture.first_transfer.clone();
    let proof = proof_for(&fixture.tree, fixture.first_leaf);
    let err = manage_transfer(&mut fixture, &first_transfer, proof).unwrap_err();
    assert_error_code(&err, STRATEGY_POLICY_PAUSED, "StrategyPolicyPaused");
}

#[test]
fn every_generic_transfer_leaf_requires_a_typed_adapter() {
    let mut fixture = setup_policy_fixture();
    let second_transfer = fixture.second_transfer.clone();
    let second_proof = proof_for(&fixture.tree, fixture.second_leaf);
    let err = manage_transfer(&mut fixture, &second_transfer, second_proof).unwrap_err();
    assert_error_code(&err, STRATEGY_ADAPTER_REQUIRED, "StrategyAdapterRequired");
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        1_000
    );
    let vault = Vault::from_bytes(fixture.svm.get_account(&fixture.vault).unwrap().data()).unwrap();
    assert_eq!(vault.manager_window_amount, 0);
}

#[test]
fn token_balance_adapter_deploys_and_pulls_with_exact_accounting() {
    let mut fixture = setup_policy_fixture();
    let deploy_proof = proof_for(&fixture.tree, fixture.deploy_leaf);
    manage_token_balance_adapter(
        &mut fixture,
        TokenBalanceAdapterAction::Deploy,
        FIRST_AMOUNT,
        DEPLOY_POLICY_MAX_AMOUNT,
        deploy_proof,
    )
    .expect("authorized deploy should succeed");

    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        750
    );
    assert_eq!(
        get_token_account_amount(
            &fixture
                .svm
                .get_account(&fixture.position_token_account)
                .unwrap()
        ),
        FIRST_AMOUNT
    );
    assert_eq!(read_position(&fixture).amount, FIRST_AMOUNT);

    fixture.svm.expire_blockhash();
    let pull_proof = proof_for(&fixture.tree, fixture.pull_leaf);
    manage_token_balance_adapter(
        &mut fixture,
        TokenBalanceAdapterAction::Pull,
        SECOND_AMOUNT,
        PULL_POLICY_MAX_AMOUNT,
        pull_proof,
    )
    .expect("authorized pull should succeed");

    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        850
    );
    assert_eq!(
        get_token_account_amount(
            &fixture
                .svm
                .get_account(&fixture.position_token_account)
                .unwrap()
        ),
        150
    );
    assert_eq!(read_position(&fixture).amount, 150);
    let vault = Vault::from_bytes(fixture.svm.get_account(&fixture.vault).unwrap().data()).unwrap();
    assert_eq!(vault.manager_window_amount, FIRST_AMOUNT + SECOND_AMOUNT);
}

#[test]
fn token_balance_adapter_reconciles_secondary_asset_ledgers() {
    let mut fixture = setup_policy_fixture();
    let secondary_mint = Keypair::new();
    create_mint(
        &mut fixture.svm,
        &fixture.authority,
        &secondary_mint,
        &token::ID,
    );
    let vault_asset = Pubkey::find_program_address(
        &[
            ASSET_CONFIG_SEED,
            fixture.vault.as_ref(),
            secondary_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    let reserve = Pubkey::find_program_address(
        &[
            ASSET_RESERVE_SEED,
            fixture.vault.as_ref(),
            secondary_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    let pending_vault = Pubkey::find_program_address(
        &[
            ASSET_PENDING_SEED,
            fixture.vault.as_ref(),
            secondary_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    AddVaultAssetBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .asset_mint(secondary_mint.pubkey())
        .vault_asset(vault_asset)
        .reserve(reserve)
        .pending_vault(pending_vault)
        .asset_token_program(token::ID)
        .deposit_cap(1_000)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority],
        )
        .expect("secondary asset should be approved");
    helper_mint_to(
        &mut fixture.svm,
        &secondary_mint.pubkey(),
        &reserve,
        &fixture.authority,
        500,
        &token::ID,
    );
    let mut vault_asset_account = fixture.svm.get_account(&vault_asset).unwrap();
    let mut vault_asset_state = VaultAsset::from_bytes(vault_asset_account.data()).unwrap();
    vault_asset_state.idle_balance = 500;
    vault_asset_account.data = borsh::to_vec(&vault_asset_state).unwrap();
    fixture
        .svm
        .set_account(vault_asset, vault_asset_account)
        .unwrap();

    let primary_proof = proof_for(&fixture.tree, fixture.deploy_leaf);
    let err = ManageVaultWithTokenBalanceAdapterBuilder::new()
        .strategist(fixture.authority.pubkey())
        .share_mint(fixture.share_mint.pubkey())
        .vault(fixture.vault)
        .asset_mint(fixture.asset_mint.pubkey())
        .vault_asset(Some(vault_asset))
        .venue_entry(fixture.venue_entry)
        .vault_venue(fixture.vault_venue)
        .strategy_policy(fixture.strategy_policy)
        .position(fixture.position)
        .vault_token_account(fixture.reserve)
        .position_token_account(fixture.position_token_account)
        .asset_token_program(token::ID)
        .policy_version(1)
        .action(TokenBalanceAdapterAction::Deploy)
        .amount(1)
        .policy_max_amount(DEPLOY_POLICY_MAX_AMOUNT)
        .proof(primary_proof)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap_err();
    assert_error_code(&err, INVALID_STRATEGY_ADAPTER, "InvalidStrategyAdapter");

    let position = Pubkey::find_program_address(
        &[
            POSITION_SEED,
            fixture.vault.as_ref(),
            fixture.venue_entry.as_ref(),
            secondary_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    let position_token_account = Pubkey::find_program_address(
        &[
            POSITION_TOKEN_SEED,
            fixture.vault.as_ref(),
            fixture.venue_entry.as_ref(),
            secondary_mint.pubkey().as_ref(),
        ],
        &program_id(),
    )
    .0;
    CreateVenuePositionBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .asset_mint(secondary_mint.pubkey())
        .vault_asset(Some(vault_asset))
        .venue_entry(fixture.venue_entry)
        .vault_venue(fixture.vault_venue)
        .position(position)
        .position_token_account(position_token_account)
        .asset_token_program(token::ID)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority],
        )
        .expect("secondary position should be created");

    let deploy_leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf::for_async_vault(
        fixture.vault,
        fixture.authority.pubkey(),
        2,
        fixture.venue_entry,
        fixture.vault_venue,
        token::ID,
        TokenBalanceAdapterAction::Deploy,
        secondary_mint.pubkey(),
        reserve,
        position,
        position_token_account,
        300,
    ))
    .unwrap();
    let pull_leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf::for_async_vault(
        fixture.vault,
        fixture.authority.pubkey(),
        2,
        fixture.venue_entry,
        fixture.vault_venue,
        token::ID,
        TokenBalanceAdapterAction::Pull,
        secondary_mint.pubkey(),
        reserve,
        position,
        position_token_account,
        100,
    ))
    .unwrap();
    let tree = build_strategy_policy_merkle_tree(vec![deploy_leaf, pull_leaf]).unwrap();
    fixture.svm.expire_blockhash();
    UpdateStrategyPolicyBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .args(StrategyPolicyUpdateArgs {
            merkle_root: tree.root,
            paused: false,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .expect("secondary adapter policy should activate");

    let call = |fixture: &mut PolicyFixture,
                action: TokenBalanceAdapterAction,
                amount: u64,
                max: u64,
                proof: Vec<[u8; 32]>| {
        ManageVaultWithTokenBalanceAdapterBuilder::new()
            .strategist(fixture.authority.pubkey())
            .share_mint(fixture.share_mint.pubkey())
            .vault(fixture.vault)
            .asset_mint(secondary_mint.pubkey())
            .vault_asset(Some(vault_asset))
            .venue_entry(fixture.venue_entry)
            .vault_venue(fixture.vault_venue)
            .strategy_policy(fixture.strategy_policy)
            .position(position)
            .vault_token_account(reserve)
            .position_token_account(position_token_account)
            .asset_token_program(token::ID)
            .policy_version(2)
            .action(action)
            .amount(amount)
            .policy_max_amount(max)
            .proof(proof)
            .instruction()
            .send_transaction(
                &mut fixture.svm,
                &fixture.authority.pubkey(),
                &[&fixture.authority],
            )
    };
    call(
        &mut fixture,
        TokenBalanceAdapterAction::Deploy,
        200,
        300,
        proof_for(&tree, deploy_leaf),
    )
    .expect("secondary deploy should succeed");
    fixture.svm.expire_blockhash();
    call(
        &mut fixture,
        TokenBalanceAdapterAction::Pull,
        50,
        100,
        proof_for(&tree, pull_leaf),
    )
    .expect("secondary pull should succeed");

    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&reserve).unwrap()),
        350
    );
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&position_token_account).unwrap()),
        150
    );
    let position_state =
        Position::from_bytes(fixture.svm.get_account(&position).unwrap().data()).unwrap();
    assert_eq!(position_state.amount, 150);
    let asset_state =
        VaultAsset::from_bytes(fixture.svm.get_account(&vault_asset).unwrap().data()).unwrap();
    assert_eq!(asset_state.idle_balance, 350);
    assert_eq!(asset_state.deployed_balance, 150);
    assert_eq!(asset_state.manager_window_amount, 250);
    let vault_state =
        Vault::from_bytes(fixture.svm.get_account(&fixture.vault).unwrap().data()).unwrap();
    assert_eq!(vault_state.manager_window_amount, 0);
}

#[test]
fn token_balance_adapter_rejects_amount_above_leaf_maximum_without_side_effects() {
    let mut fixture = setup_policy_fixture();
    let proof = proof_for(&fixture.tree, fixture.deploy_leaf);
    let err = manage_token_balance_adapter(
        &mut fixture,
        TokenBalanceAdapterAction::Deploy,
        DEPLOY_POLICY_MAX_AMOUNT + 1,
        DEPLOY_POLICY_MAX_AMOUNT,
        proof,
    )
    .unwrap_err();
    assert_error_code(
        &err,
        STRATEGY_ADAPTER_AMOUNT_EXCEEDED,
        "StrategyAdapterAmountExceeded",
    );
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        1_000
    );
    assert_eq!(read_position(&fixture).amount, 0);
    let vault = Vault::from_bytes(fixture.svm.get_account(&fixture.vault).unwrap().data()).unwrap();
    assert_eq!(vault.manager_window_amount, 0);
}

#[test]
fn token_balance_adapter_proof_binds_the_declared_maximum() {
    let mut fixture = setup_policy_fixture();
    let proof = proof_for(&fixture.tree, fixture.deploy_leaf);
    let err = manage_token_balance_adapter(
        &mut fixture,
        TokenBalanceAdapterAction::Deploy,
        100,
        DEPLOY_POLICY_MAX_AMOUNT - 1,
        proof,
    )
    .unwrap_err();
    assert_error_code(&err, MERKLE_PROOF_INVALID, "MerkleProofInvalid");
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        1_000
    );
    assert_eq!(read_position(&fixture).amount, 0);
}

#[test]
fn token_balance_adapter_fails_closed_on_position_ledger_drift() {
    let mut fixture = setup_policy_fixture();
    let mut position_account = fixture.svm.get_account(&fixture.position).unwrap();
    let mut position = Position::from_bytes(position_account.data()).unwrap();
    position.amount = 1;
    position_account.data = borsh::to_vec(&position).unwrap();
    fixture
        .svm
        .set_account(fixture.position, position_account)
        .unwrap();

    let proof = proof_for(&fixture.tree, fixture.deploy_leaf);
    let err = manage_token_balance_adapter(
        &mut fixture,
        TokenBalanceAdapterAction::Deploy,
        100,
        DEPLOY_POLICY_MAX_AMOUNT,
        proof,
    )
    .unwrap_err();
    assert_error_code(
        &err,
        POSITION_ACCOUNTING_MISMATCH,
        "PositionAccountingMismatch",
    );
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        1_000
    );
    assert_eq!(
        get_token_account_amount(
            &fixture
                .svm
                .get_account(&fixture.position_token_account)
                .unwrap()
        ),
        0
    );
}

#[test]
fn token_balance_adapter_enforces_cumulative_manager_limit() {
    let mut fixture = setup_policy_fixture();
    let calls = [
        (TokenBalanceAdapterAction::Deploy, 300),
        (TokenBalanceAdapterAction::Pull, 200),
        (TokenBalanceAdapterAction::Deploy, 300),
        (TokenBalanceAdapterAction::Pull, 200),
    ];
    for (action, amount) in calls {
        let (leaf, max) = match action {
            TokenBalanceAdapterAction::Deploy => (fixture.deploy_leaf, DEPLOY_POLICY_MAX_AMOUNT),
            TokenBalanceAdapterAction::Pull => (fixture.pull_leaf, PULL_POLICY_MAX_AMOUNT),
        };
        let proof = proof_for(&fixture.tree, leaf);
        manage_token_balance_adapter(&mut fixture, action, amount, max, proof)
            .expect("operation within cumulative limit should succeed");
        fixture.svm.expire_blockhash();
    }

    let proof = proof_for(&fixture.tree, fixture.deploy_leaf);
    let err = manage_token_balance_adapter(
        &mut fixture,
        TokenBalanceAdapterAction::Deploy,
        1,
        DEPLOY_POLICY_MAX_AMOUNT,
        proof,
    )
    .unwrap_err();
    assert_error_code(&err, ROLLING_LIMIT_EXCEEDED, "RollingLimitExceeded");
    assert_eq!(
        get_token_account_amount(&fixture.svm.get_account(&fixture.reserve).unwrap()),
        800
    );
    assert_eq!(read_position(&fixture).amount, 200);
    let vault = Vault::from_bytes(fixture.svm.get_account(&fixture.vault).unwrap().data()).unwrap();
    assert_eq!(vault.manager_window_amount, 1_000);
}

#[test]
fn managed_cpi_that_changes_share_supply_is_rolled_back() {
    let mut fixture = setup_policy_fixture();
    let mint_shares = fixture.mint_shares.clone();
    let operators = fixture.mint_operators.clone();
    let proof = proof_for(&fixture.tree, fixture.mint_leaf);
    let err = manage_instruction(&mut fixture, &mint_shares, operators, proof).unwrap_err();
    assert_error_code(&err, SHARE_SUPPLY_CHANGED, "ShareSupplyChanged");
    assert_eq!(
        crate::async_helper_functions::get_mint_supply(
            &fixture
                .svm
                .get_account(&fixture.share_mint.pubkey())
                .unwrap()
        ),
        0
    );
    assert_eq!(
        get_token_account_amount(
            &fixture
                .svm
                .get_account(&fixture.user_share_account)
                .unwrap()
        ),
        0
    );
}

#[test]
fn in_progress_managed_call_blocks_reentrant_manage_and_policy_mutation() {
    let mut fixture = setup_policy_fixture();
    let mut policy_account = fixture.svm.get_account(&fixture.strategy_policy).unwrap();
    let mut policy = StrategyPolicy::from_bytes(policy_account.data()).unwrap();
    policy.executing = true;
    policy_account.data = borsh::to_vec(&policy).unwrap();
    fixture
        .svm
        .set_account(fixture.strategy_policy, policy_account)
        .unwrap();

    let err = PauseStrategyPolicyBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap_err();
    assert_error_code(&err, REENTRANT_STRATEGY_CALL, "ReentrantStrategyCall");

    fixture.svm.expire_blockhash();
    let first_transfer = fixture.first_transfer.clone();
    let proof = proof_for(&fixture.tree, fixture.first_leaf);
    let err = manage_transfer(&mut fixture, &first_transfer, proof).unwrap_err();
    assert_error_code(&err, REENTRANT_STRATEGY_CALL, "ReentrantStrategyCall");
}

#[test]
fn policy_root_updates_follow_the_existing_vault_timelock() {
    let mut fixture = setup_policy_fixture();
    UpdateVaultBuilder::new()
        .authority(fixture.authority.pubkey())
        .share_mint(fixture.share_mint.pubkey())
        .vault(fixture.vault)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .expect("timelock activation should succeed");

    let next_root = [9_u8; 32];
    fixture.svm.expire_blockhash();
    let err = UpdateStrategyPolicyBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .args(StrategyPolicyUpdateArgs {
            merkle_root: next_root,
            paused: false,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_REQUIRED, "TimelockRequired");

    let pending_update = Keypair::new();
    fixture.svm.expire_blockhash();
    QueueStrategyPolicyUpdateBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .pending_update(pending_update.pubkey())
        .args(StrategyPolicyUpdateArgs {
            merkle_root: next_root,
            paused: false,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority, &pending_update],
        )
        .expect("policy root update should queue");
    let pending = PendingStrategyPolicyUpdate::from_bytes(
        fixture
            .svm
            .get_account(&pending_update.pubkey())
            .unwrap()
            .data(),
    )
    .unwrap();
    assert_eq!(pending.expected_version, 1);

    fixture.svm.expire_blockhash();
    let err = ExecuteStrategyPolicyUpdateBuilder::new()
        .executor(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .pending_update(pending_update.pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap_err();
    assert_error_code(&err, TIMELOCK_NOT_READY, "TimelockNotReady");

    let mut clock = fixture.svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    fixture.svm.set_sysvar(&clock);
    fixture.svm.expire_blockhash();
    ExecuteStrategyPolicyUpdateBuilder::new()
        .executor(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .pending_update(pending_update.pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .expect("mature queued root should execute");

    let policy = StrategyPolicy::from_bytes(
        fixture
            .svm
            .get_account(&fixture.strategy_policy)
            .unwrap()
            .data(),
    )
    .unwrap();
    assert_eq!(policy.version, 2);
    assert_eq!(policy.merkle_root, next_root);
    assert!(
        fixture.svm.get_account(&pending_update.pubkey()).is_none(),
        "executed pending update should close"
    );
}

#[test]
fn emergency_pause_invalidates_a_previously_queued_unpause() {
    let mut fixture = setup_policy_fixture();
    UpdateVaultBuilder::new()
        .authority(fixture.authority.pubkey())
        .share_mint(fixture.share_mint.pubkey())
        .vault(fixture.vault)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap();

    let pending_update = Keypair::new();
    QueueStrategyPolicyUpdateBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .pending_update(pending_update.pubkey())
        .args(StrategyPolicyUpdateArgs {
            merkle_root: [7; 32],
            paused: false,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority, &pending_update],
        )
        .unwrap();
    let pending = PendingStrategyPolicyUpdate::from_bytes(
        fixture
            .svm
            .get_account(&pending_update.pubkey())
            .unwrap()
            .data(),
    )
    .unwrap();

    fixture.svm.expire_blockhash();
    PauseStrategyPolicyBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap();
    let paused = StrategyPolicy::from_bytes(
        fixture
            .svm
            .get_account(&fixture.strategy_policy)
            .unwrap()
            .data(),
    )
    .unwrap();
    assert!(paused.paused);
    assert_eq!(paused.version, 2);

    let mut clock = fixture.svm.get_sysvar::<Clock>();
    clock.slot = pending.eta_slot;
    fixture.svm.set_sysvar(&clock);
    fixture.svm.expire_blockhash();
    let err = ExecuteStrategyPolicyUpdateBuilder::new()
        .executor(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .pending_update(pending_update.pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap_err();
    assert_error_code(
        &err,
        STALE_STRATEGY_POLICY_VERSION,
        "StaleStrategyPolicyVersion",
    );
}

#[test]
fn pending_update_can_be_canceled_after_policy_is_closed() {
    let mut fixture = setup_policy_fixture();
    UpdateVaultBuilder::new()
        .authority(fixture.authority.pubkey())
        .share_mint(fixture.share_mint.pubkey())
        .vault(fixture.vault)
        .timelock_delay_slots(3)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap();
    let pending_update = Keypair::new();
    QueueStrategyPolicyUpdateBuilder::new()
        .payer(fixture.payer.pubkey())
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .pending_update(pending_update.pubkey())
        .args(StrategyPolicyUpdateArgs {
            merkle_root: [8; 32],
            paused: false,
        })
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.payer.pubkey(),
            &[&fixture.payer, &fixture.authority, &pending_update],
        )
        .unwrap();

    fixture.svm.expire_blockhash();
    CloseStrategyPolicyBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .strategy_policy(fixture.strategy_policy)
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap();
    assert!(fixture.svm.get_account(&fixture.strategy_policy).is_none());

    fixture.svm.expire_blockhash();
    CancelStrategyPolicyUpdateBuilder::new()
        .authority(fixture.authority.pubkey())
        .vault(fixture.vault)
        .pending_update(pending_update.pubkey())
        .instruction()
        .send_transaction(
            &mut fixture.svm,
            &fixture.authority.pubkey(),
            &[&fixture.authority],
        )
        .unwrap();
    assert!(fixture.svm.get_account(&pending_update.pubkey()).is_none());
}
