use solana_address::Address;
use solana_sha256_hasher::hash;
use thiserror::Error;

use crate::{PolicyOperator, TokenBalanceAdapterAction, ASYNC_VAULT_V2_ID};

pub const STRATEGY_LEAF_DOMAIN: &[u8] = b"async-vault-v2/strategy-policy-leaf/v1";
pub const STRATEGY_NODE_DOMAIN: &[u8] = b"async-vault-v2/strategy-policy-node/v1";
pub const TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN: &[u8] =
    b"async-vault-v2/token-balance-adapter-leaf/v1";
pub const MAX_CPI_ACCOUNTS: usize = 64;
pub const MAX_INSTRUCTION_DATA_LEN: usize = 1024;
pub const MAX_POLICY_OPERATORS: usize = 32;
pub const MAX_INGESTED_INSTRUCTION_BYTES: usize = 2_048;
pub const MAX_POLICY_LEAVES: usize = 1 << 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrategyPolicyAccountMeta {
    pub address: Address,
    pub is_signer: bool,
    pub is_writable: bool,
}

pub struct StrategyPolicyLeaf<'a> {
    pub program_address: Address,
    pub vault: Address,
    pub strategist: Address,
    pub policy_version: u64,
    pub target_program: Address,
    pub instruction_data: &'a [u8],
    /// These must be the effective CPI privileges. Use [`normalize_policy_accounts`].
    pub accounts: &'a [StrategyPolicyAccountMeta],
    pub operators: &'a [PolicyOperator],
}

impl<'a> StrategyPolicyLeaf<'a> {
    pub fn for_async_vault(
        vault: Address,
        strategist: Address,
        policy_version: u64,
        target_program: Address,
        instruction_data: &'a [u8],
        accounts: &'a [StrategyPolicyAccountMeta],
        operators: &'a [PolicyOperator],
    ) -> Self {
        Self {
            program_address: ASYNC_VAULT_V2_ID,
            vault,
            strategist,
            policy_version,
            target_program,
            instruction_data,
            accounts,
            operators,
        }
    }
}

pub struct TokenBalanceAdapterLeaf {
    pub program_address: Address,
    pub vault: Address,
    pub strategist: Address,
    pub policy_version: u64,
    pub venue_entry: Address,
    pub vault_venue: Address,
    pub target_program: Address,
    pub action: TokenBalanceAdapterAction,
    pub asset_mint: Address,
    pub vault_token_account: Address,
    pub position: Address,
    pub position_token_account: Address,
    pub policy_max_amount: u64,
}

impl TokenBalanceAdapterLeaf {
    #[allow(clippy::too_many_arguments)]
    pub fn for_async_vault(
        vault: Address,
        strategist: Address,
        policy_version: u64,
        venue_entry: Address,
        vault_venue: Address,
        target_program: Address,
        action: TokenBalanceAdapterAction,
        asset_mint: Address,
        vault_token_account: Address,
        position: Address,
        position_token_account: Address,
        policy_max_amount: u64,
    ) -> Self {
        Self {
            program_address: ASYNC_VAULT_V2_ID,
            vault,
            strategist,
            policy_version,
            venue_entry,
            vault_venue,
            target_program,
            action,
            asset_mint,
            vault_token_account,
            position,
            position_token_account,
            policy_max_amount,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrategyPolicyMerkleTree {
    /// Lexicographically sorted leaves. Proof indexes refer to this order.
    pub leaves: Vec<[u8; 32]>,
    /// Level zero contains `leaves`; the last level contains the root.
    pub levels: Vec<Vec<[u8; 32]>>,
    pub root: [u8; 32],
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StrategyPolicyHashError {
    #[error("instruction data must contain 8..=1024 bytes")]
    InvalidInstructionData,
    #[error("managed CPI has too many accounts")]
    TooManyAccounts,
    #[error("policy has too many operators")]
    TooManyOperators,
    #[error("policy operator is out of bounds")]
    OperatorOutOfBounds,
    #[error("instruction ingestion length must be nonzero")]
    EmptyInstructionRange,
    #[error("only one manager-limit amount operator is allowed")]
    DuplicateManagerLimitAmount,
    #[error("policy instruction ingestion exceeds the 2048-byte limit")]
    IngestionLimitExceeded,
    #[error("at least one policy leaf is required")]
    EmptyTree,
    #[error("policy tree exceeds the 4096-leaf proof-depth limit")]
    TreeTooLarge,
    #[error("sorted leaf index is out of bounds")]
    LeafIndexOutOfBounds,
    #[error("token-balance adapter maximum amount must be nonzero")]
    InvalidAdapterMaxAmount,
}

pub fn normalize_policy_accounts(
    vault: Address,
    accounts: &[StrategyPolicyAccountMeta],
) -> Vec<StrategyPolicyAccountMeta> {
    accounts
        .iter()
        .map(|account| StrategyPolicyAccountMeta {
            address: account.address,
            is_signer: account.is_signer || account.address == vault,
            is_writable: account.is_writable && account.address != vault,
        })
        .collect()
}

pub fn hash_strategy_policy_leaf(
    input: StrategyPolicyLeaf<'_>,
) -> Result<[u8; 32], StrategyPolicyHashError> {
    if !(8..=MAX_INSTRUCTION_DATA_LEN).contains(&input.instruction_data.len()) {
        return Err(StrategyPolicyHashError::InvalidInstructionData);
    }
    if input.accounts.len() > MAX_CPI_ACCOUNTS {
        return Err(StrategyPolicyHashError::TooManyAccounts);
    }
    if input.operators.len() > MAX_POLICY_OPERATORS {
        return Err(StrategyPolicyHashError::TooManyOperators);
    }

    let mut bytes = Vec::with_capacity(256 + input.instruction_data.len());
    bytes.extend_from_slice(STRATEGY_LEAF_DOMAIN);
    bytes.extend_from_slice(&input.program_address.to_bytes());
    bytes.extend_from_slice(&input.vault.to_bytes());
    bytes.extend_from_slice(&input.strategist.to_bytes());
    bytes.extend_from_slice(&input.policy_version.to_le_bytes());
    bytes.extend_from_slice(&input.target_program.to_bytes());
    bytes.extend_from_slice(&input.instruction_data[..8]);
    bytes.extend_from_slice(&(input.instruction_data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(input.accounts.len() as u16).to_le_bytes());
    bytes.push(input.operators.len() as u8);

    let mut has_manager_limit_amount = false;
    let mut ingested_instruction_bytes = 0_usize;
    for operator in input.operators {
        match operator {
            PolicyOperator::IngestInstruction { offset, length } => {
                if *length == 0 {
                    return Err(StrategyPolicyHashError::EmptyInstructionRange);
                }
                let from = usize::from(*offset);
                let to = from
                    .checked_add(usize::from(*length))
                    .ok_or(StrategyPolicyHashError::OperatorOutOfBounds)?;
                let selected = input
                    .instruction_data
                    .get(from..to)
                    .ok_or(StrategyPolicyHashError::OperatorOutOfBounds)?;
                ingested_instruction_bytes = ingested_instruction_bytes
                    .checked_add(selected.len())
                    .ok_or(StrategyPolicyHashError::IngestionLimitExceeded)?;
                if ingested_instruction_bytes > MAX_INGESTED_INSTRUCTION_BYTES {
                    return Err(StrategyPolicyHashError::IngestionLimitExceeded);
                }
                bytes.push(0);
                bytes.extend_from_slice(&offset.to_le_bytes());
                bytes.extend_from_slice(&length.to_le_bytes());
                bytes.extend_from_slice(selected);
            }
            PolicyOperator::IngestAccount { index } => {
                let account = input
                    .accounts
                    .get(usize::from(*index))
                    .ok_or(StrategyPolicyHashError::OperatorOutOfBounds)?;
                bytes.push(1);
                bytes.push(*index);
                bytes.extend_from_slice(&account.address.to_bytes());
                bytes.push(u8::from(account.is_signer));
                bytes.push(u8::from(account.is_writable));
            }
            PolicyOperator::ManagerLimitAmount { offset } => {
                if has_manager_limit_amount {
                    return Err(StrategyPolicyHashError::DuplicateManagerLimitAmount);
                }
                let from = usize::from(*offset);
                let to = from
                    .checked_add(size_of::<u64>())
                    .ok_or(StrategyPolicyHashError::OperatorOutOfBounds)?;
                input
                    .instruction_data
                    .get(from..to)
                    .ok_or(StrategyPolicyHashError::OperatorOutOfBounds)?;
                has_manager_limit_amount = true;
                bytes.push(2);
                bytes.extend_from_slice(&offset.to_le_bytes());
            }
        }
    }

    Ok(hash(&bytes).to_bytes())
}

pub fn hash_token_balance_adapter_leaf(
    input: TokenBalanceAdapterLeaf,
) -> Result<[u8; 32], StrategyPolicyHashError> {
    if input.policy_max_amount == 0 {
        return Err(StrategyPolicyHashError::InvalidAdapterMaxAmount);
    }
    let action = match input.action {
        TokenBalanceAdapterAction::Deploy => 0,
        TokenBalanceAdapterAction::Pull => 1,
    };
    let mut bytes = Vec::with_capacity(TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN.len() + 32 * 10 + 17);
    bytes.extend_from_slice(TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN);
    bytes.extend_from_slice(&input.program_address.to_bytes());
    bytes.extend_from_slice(&input.vault.to_bytes());
    bytes.extend_from_slice(&input.strategist.to_bytes());
    bytes.extend_from_slice(&input.policy_version.to_le_bytes());
    bytes.extend_from_slice(&input.venue_entry.to_bytes());
    bytes.extend_from_slice(&input.vault_venue.to_bytes());
    bytes.extend_from_slice(&input.target_program.to_bytes());
    bytes.push(action);
    bytes.extend_from_slice(&input.asset_mint.to_bytes());
    bytes.extend_from_slice(&input.vault_token_account.to_bytes());
    bytes.extend_from_slice(&input.position.to_bytes());
    bytes.extend_from_slice(&input.position_token_account.to_bytes());
    bytes.extend_from_slice(&input.policy_max_amount.to_le_bytes());
    Ok(hash(&bytes).to_bytes())
}

pub fn hash_strategy_policy_node(first: [u8; 32], second: [u8; 32]) -> [u8; 32] {
    let (lower, upper) = if first <= second {
        (first, second)
    } else {
        (second, first)
    };
    let mut bytes = Vec::with_capacity(STRATEGY_NODE_DOMAIN.len() + 64);
    bytes.extend_from_slice(STRATEGY_NODE_DOMAIN);
    bytes.extend_from_slice(&lower);
    bytes.extend_from_slice(&upper);
    hash(&bytes).to_bytes()
}

pub fn build_strategy_policy_merkle_tree(
    mut leaves: Vec<[u8; 32]>,
) -> Result<StrategyPolicyMerkleTree, StrategyPolicyHashError> {
    if leaves.is_empty() {
        return Err(StrategyPolicyHashError::EmptyTree);
    }
    if leaves.len() > MAX_POLICY_LEAVES {
        return Err(StrategyPolicyHashError::TreeTooLarge);
    }
    leaves.sort_unstable();
    let mut levels = vec![leaves.clone()];
    let mut current = leaves.clone();
    while current.len() > 1 {
        let mut next = Vec::with_capacity(current.len().div_ceil(2));
        for pair in current.chunks(2) {
            next.push(hash_strategy_policy_node(
                pair[0],
                pair.get(1).copied().unwrap_or(pair[0]),
            ));
        }
        levels.push(next.clone());
        current = next;
    }
    Ok(StrategyPolicyMerkleTree {
        leaves,
        levels,
        root: current[0],
    })
}

pub fn strategy_policy_proof(
    tree: &StrategyPolicyMerkleTree,
    sorted_leaf_index: usize,
) -> Result<Vec<[u8; 32]>, StrategyPolicyHashError> {
    if sorted_leaf_index >= tree.leaves.len() {
        return Err(StrategyPolicyHashError::LeafIndexOutOfBounds);
    }
    let mut proof = Vec::with_capacity(tree.levels.len().saturating_sub(1));
    let mut index = sorted_leaf_index;
    for level in tree.levels.iter().take(tree.levels.len().saturating_sub(1)) {
        let sibling = if index.is_multiple_of(2) {
            level.get(index + 1).copied().unwrap_or(level[index])
        } else {
            level[index - 1]
        };
        proof.push(sibling);
        index /= 2;
    }
    Ok(proof)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(value: [u8; 32]) -> String {
        value.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn proof_reconstructs_a_three_leaf_tree() {
        let tree = build_strategy_policy_merkle_tree(vec![[3; 32], [1; 32], [2; 32]]).unwrap();
        for (index, leaf) in tree.leaves.iter().copied().enumerate() {
            let computed = strategy_policy_proof(&tree, index)
                .unwrap()
                .into_iter()
                .fold(leaf, hash_strategy_policy_node);
            assert_eq!(computed, tree.root);
        }
    }

    #[test]
    fn cross_language_hash_and_proof_vector_is_stable() {
        let vault = Address::from_str_const("11111111111111111111111111111111");
        let strategist = Address::from_str_const("SysvarC1ock11111111111111111111111111111111");
        let target = Address::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        let accounts = normalize_policy_accounts(
            vault,
            &[
                StrategyPolicyAccountMeta {
                    address: vault,
                    is_signer: false,
                    is_writable: true,
                },
                StrategyPolicyAccountMeta {
                    address: target,
                    is_signer: false,
                    is_writable: false,
                },
            ],
        );
        let instruction_data: Vec<u8> = (1..=16).collect();
        let operators = [
            PolicyOperator::IngestInstruction {
                offset: 8,
                length: 4,
            },
            PolicyOperator::IngestAccount { index: 0 },
            PolicyOperator::IngestAccount { index: 1 },
            PolicyOperator::ManagerLimitAmount { offset: 8 },
        ];
        let leaf = hash_strategy_policy_leaf(StrategyPolicyLeaf::for_async_vault(
            vault,
            strategist,
            7,
            target,
            &instruction_data,
            &accounts,
            &operators,
        ))
        .unwrap();
        let tree = build_strategy_policy_merkle_tree(vec![leaf, [7; 32], [9; 32]]).unwrap();
        let index = tree
            .leaves
            .iter()
            .position(|candidate| *candidate == leaf)
            .unwrap();
        let proof = strategy_policy_proof(&tree, index).unwrap();

        assert_eq!(
            hex(leaf),
            "d925c25360e7d800278163511eb9742bedbb5fdf9640323afa9814c3840d77f0"
        );
        assert_eq!(
            hex(tree.root),
            "8b27c6b7da89dfc5deb2a320214be3e77371e19ea5abae2bd8d585d1d09fe12a"
        );
        assert_eq!(
            proof.into_iter().map(hex).collect::<Vec<_>>(),
            [
                "d925c25360e7d800278163511eb9742bedbb5fdf9640323afa9814c3840d77f0",
                "8b88aadfa6a0dc942d584d37d30895e891fb6e34900b5d30ebd679b33ef282e5",
            ]
        );
    }

    #[test]
    fn token_balance_adapter_hash_vector_is_stable() {
        let system = Address::from_str_const("11111111111111111111111111111111");
        let clock = Address::from_str_const("SysvarC1ock11111111111111111111111111111111");
        let token = Address::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        let associated = Address::from_str_const("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
        let loader = Address::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");
        let vote = Address::from_str_const("Vote111111111111111111111111111111111111111");
        let stake = Address::from_str_const("Stake11111111111111111111111111111111111111");
        let compute = Address::from_str_const("ComputeBudget111111111111111111111111111111");
        let leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf::for_async_vault(
            system,
            clock,
            7,
            associated,
            loader,
            token,
            TokenBalanceAdapterAction::Deploy,
            vote,
            stake,
            compute,
            clock,
            250,
        ))
        .unwrap();

        assert_eq!(
            hex(leaf),
            "4c0920146de6af2cfc03030a24a4cf10d835a76bdafccc86394602928c4f066e"
        );
    }

    #[test]
    fn token_balance_adapter_leaf_binds_action_accounts_version_and_maximum() {
        let address = |byte: u8| Address::new_from_array([byte; 32]);
        let baseline = || {
            TokenBalanceAdapterLeaf::for_async_vault(
                address(1),
                address(2),
                1,
                address(3),
                address(4),
                address(5),
                TokenBalanceAdapterAction::Deploy,
                address(6),
                address(7),
                address(8),
                address(9),
                100,
            )
        };
        let baseline_hash = hash_token_balance_adapter_leaf(baseline()).unwrap();

        let mut changed = baseline();
        changed.action = TokenBalanceAdapterAction::Pull;
        assert_ne!(
            hash_token_balance_adapter_leaf(changed).unwrap(),
            baseline_hash
        );
        let mut changed = baseline();
        changed.position_token_account = address(10);
        assert_ne!(
            hash_token_balance_adapter_leaf(changed).unwrap(),
            baseline_hash
        );
        let mut changed = baseline();
        changed.policy_version = 2;
        assert_ne!(
            hash_token_balance_adapter_leaf(changed).unwrap(),
            baseline_hash
        );
        let mut changed = baseline();
        changed.policy_max_amount = 101;
        assert_ne!(
            hash_token_balance_adapter_leaf(changed).unwrap(),
            baseline_hash
        );
    }

    #[test]
    fn token_balance_adapter_client_rejects_zero_maximum() {
        let address = |byte: u8| Address::new_from_array([byte; 32]);
        let input = TokenBalanceAdapterLeaf::for_async_vault(
            address(1),
            address(2),
            1,
            address(3),
            address(4),
            address(5),
            TokenBalanceAdapterAction::Deploy,
            address(6),
            address(7),
            address(8),
            address(9),
            0,
        );
        assert_eq!(
            hash_token_balance_adapter_leaf(input),
            Err(StrategyPolicyHashError::InvalidAdapterMaxAmount)
        );
    }
}
