use anchor_lang::prelude::*;
use solana_sha256_hasher::hash;

use crate::{
    error::AsyncVaultError,
    state::{
        TokenBalanceAdapterAction, MAX_MANAGE_CPI_ACCOUNTS, MAX_MANAGE_IX_DATA_LEN,
        MAX_MERKLE_PROOF_DEPTH, MAX_POLICY_INGESTED_INSTRUCTION_BYTES, MAX_POLICY_OPERATORS,
    },
};

pub const STRATEGY_LEAF_DOMAIN: &[u8] = b"async-vault-v2/strategy-policy-leaf/v1";
pub const STRATEGY_NODE_DOMAIN: &[u8] = b"async-vault-v2/strategy-policy-node/v1";
pub const TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN: &[u8] =
    b"async-vault-v2/token-balance-adapter-leaf/v1";

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum PolicyOperator {
    /// Commit a checked instruction-data range to the leaf.
    IngestInstruction { offset: u16, length: u16 },
    /// Commit an ordered account key and its effective CPI privileges.
    IngestAccount { index: u8 },
    /// Parse a dynamic little-endian u64 for the vault manager rolling limit.
    ManagerLimitAmount { offset: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyAccountMeta {
    pub key: Pubkey,
    pub is_signer: bool,
    pub is_writable: bool,
}

pub struct StrategyLeaf<'a> {
    pub vault: Pubkey,
    pub strategist: Pubkey,
    pub policy_version: u64,
    pub target_program: Pubkey,
    pub instruction_data: &'a [u8],
    pub accounts: &'a [PolicyAccountMeta],
    pub operators: &'a [PolicyOperator],
}

pub struct TokenBalanceAdapterLeaf {
    pub vault: Pubkey,
    pub strategist: Pubkey,
    pub policy_version: u64,
    pub venue_entry: Pubkey,
    pub vault_venue: Pubkey,
    pub target_program: Pubkey,
    pub action: TokenBalanceAdapterAction,
    pub asset_mint: Pubkey,
    pub vault_token_account: Pubkey,
    pub position: Pubkey,
    pub position_token_account: Pubkey,
    pub policy_max_amount: u64,
}

pub fn hash_token_balance_adapter_leaf(input: TokenBalanceAdapterLeaf) -> [u8; 32] {
    let action = match input.action {
        TokenBalanceAdapterAction::Deploy => 0,
        TokenBalanceAdapterAction::Pull => 1,
    };
    let mut bytes = Vec::with_capacity(TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN.len() + 32 * 10 + 17);
    bytes.extend_from_slice(TOKEN_BALANCE_ADAPTER_LEAF_DOMAIN);
    bytes.extend_from_slice(crate::ID.as_ref());
    bytes.extend_from_slice(input.vault.as_ref());
    bytes.extend_from_slice(input.strategist.as_ref());
    bytes.extend_from_slice(&input.policy_version.to_le_bytes());
    bytes.extend_from_slice(input.venue_entry.as_ref());
    bytes.extend_from_slice(input.vault_venue.as_ref());
    bytes.extend_from_slice(input.target_program.as_ref());
    bytes.push(action);
    bytes.extend_from_slice(input.asset_mint.as_ref());
    bytes.extend_from_slice(input.vault_token_account.as_ref());
    bytes.extend_from_slice(input.position.as_ref());
    bytes.extend_from_slice(input.position_token_account.as_ref());
    bytes.extend_from_slice(&input.policy_max_amount.to_le_bytes());
    hash(&bytes).to_bytes()
}

pub fn hash_strategy_leaf(input: StrategyLeaf<'_>) -> Result<([u8; 32], Option<u64>)> {
    require!(
        input.instruction_data.len() >= 8,
        AsyncVaultError::InvalidVenueInstruction
    );
    require!(
        input.instruction_data.len() <= MAX_MANAGE_IX_DATA_LEN,
        AsyncVaultError::InstructionDataTooLarge
    );
    require!(
        input.accounts.len() <= MAX_MANAGE_CPI_ACCOUNTS,
        AsyncVaultError::TooManyCpiAccounts
    );
    require!(
        input.operators.len() <= MAX_POLICY_OPERATORS,
        AsyncVaultError::InvalidPolicyOperator
    );

    let instruction_len = u32::try_from(input.instruction_data.len())
        .map_err(|_| AsyncVaultError::InstructionDataTooLarge)?;
    let account_count =
        u16::try_from(input.accounts.len()).map_err(|_| AsyncVaultError::TooManyCpiAccounts)?;
    let operator_count =
        u8::try_from(input.operators.len()).map_err(|_| AsyncVaultError::InvalidPolicyOperator)?;

    let mut bytes = Vec::with_capacity(
        STRATEGY_LEAF_DOMAIN.len() + 32 * 5 + 8 + 8 + 4 + 2 + 1 + input.instruction_data.len(),
    );
    bytes.extend_from_slice(STRATEGY_LEAF_DOMAIN);
    bytes.extend_from_slice(crate::ID.as_ref());
    bytes.extend_from_slice(input.vault.as_ref());
    bytes.extend_from_slice(input.strategist.as_ref());
    bytes.extend_from_slice(&input.policy_version.to_le_bytes());
    bytes.extend_from_slice(input.target_program.as_ref());
    bytes.extend_from_slice(&input.instruction_data[..8]);
    bytes.extend_from_slice(&instruction_len.to_le_bytes());
    bytes.extend_from_slice(&account_count.to_le_bytes());
    bytes.push(operator_count);

    let mut manager_limit_amount = None;
    let mut ingested_instruction_bytes = 0_usize;
    for operator in input.operators {
        match operator {
            PolicyOperator::IngestInstruction { offset, length } => {
                require!(*length > 0, AsyncVaultError::InvalidPolicyOperator);
                let from = usize::from(*offset);
                let to = from
                    .checked_add(usize::from(*length))
                    .ok_or(AsyncVaultError::PolicyOperatorOutOfBounds)?;
                let selected = input
                    .instruction_data
                    .get(from..to)
                    .ok_or(AsyncVaultError::PolicyOperatorOutOfBounds)?;
                ingested_instruction_bytes = ingested_instruction_bytes
                    .checked_add(selected.len())
                    .ok_or(AsyncVaultError::PolicyIngestionLimitExceeded)?;
                require!(
                    ingested_instruction_bytes <= MAX_POLICY_INGESTED_INSTRUCTION_BYTES,
                    AsyncVaultError::PolicyIngestionLimitExceeded
                );
                bytes.push(0);
                bytes.extend_from_slice(&offset.to_le_bytes());
                bytes.extend_from_slice(&length.to_le_bytes());
                bytes.extend_from_slice(selected);
            }
            PolicyOperator::IngestAccount { index } => {
                let account = input
                    .accounts
                    .get(usize::from(*index))
                    .ok_or(AsyncVaultError::PolicyOperatorOutOfBounds)?;
                bytes.push(1);
                bytes.push(*index);
                bytes.extend_from_slice(account.key.as_ref());
                bytes.push(u8::from(account.is_signer));
                bytes.push(u8::from(account.is_writable));
            }
            PolicyOperator::ManagerLimitAmount { offset } => {
                require!(
                    manager_limit_amount.is_none(),
                    AsyncVaultError::DuplicateManagerLimitOperator
                );
                let from = usize::from(*offset);
                let to = from
                    .checked_add(size_of::<u64>())
                    .ok_or(AsyncVaultError::PolicyOperatorOutOfBounds)?;
                let raw: [u8; 8] = input
                    .instruction_data
                    .get(from..to)
                    .ok_or(AsyncVaultError::PolicyOperatorOutOfBounds)?
                    .try_into()
                    .map_err(|_| AsyncVaultError::PolicyOperatorOutOfBounds)?;
                bytes.push(2);
                bytes.extend_from_slice(&offset.to_le_bytes());
                manager_limit_amount = Some(u64::from_le_bytes(raw));
            }
        }
    }

    Ok((hash(&bytes).to_bytes(), manager_limit_amount))
}

pub fn hash_strategy_node(left: [u8; 32], right: [u8; 32]) -> [u8; 32] {
    let (left, right) = if left <= right {
        (left, right)
    } else {
        (right, left)
    };
    let mut bytes = Vec::with_capacity(STRATEGY_NODE_DOMAIN.len() + 64);
    bytes.extend_from_slice(STRATEGY_NODE_DOMAIN);
    bytes.extend_from_slice(&left);
    bytes.extend_from_slice(&right);
    hash(&bytes).to_bytes()
}

pub fn verify_strategy_proof(leaf: [u8; 32], proof: &[[u8; 32]], root: [u8; 32]) -> Result<()> {
    require!(
        proof.len() <= MAX_MERKLE_PROOF_DEPTH,
        AsyncVaultError::MerkleProofTooDeep
    );
    let computed = proof.iter().copied().fold(leaf, hash_strategy_node);
    require!(computed == root, AsyncVaultError::MerkleProofInvalid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(byte: u8, is_signer: bool, is_writable: bool) -> PolicyAccountMeta {
        PolicyAccountMeta {
            key: Pubkey::new_from_array([byte; 32]),
            is_signer,
            is_writable,
        }
    }

    fn leaf(
        version: u64,
        data: &[u8],
        accounts: &[PolicyAccountMeta],
        operators: &[PolicyOperator],
    ) -> Result<([u8; 32], Option<u64>)> {
        hash_strategy_leaf(StrategyLeaf {
            vault: Pubkey::new_from_array([1; 32]),
            strategist: Pubkey::new_from_array([2; 32]),
            policy_version: version,
            target_program: Pubkey::new_from_array([3; 32]),
            instruction_data: data,
            accounts,
            operators,
        })
    }

    #[test]
    fn selected_fields_and_privileges_change_the_leaf() {
        let data = [7_u8; 16];
        let operators = [
            PolicyOperator::IngestInstruction {
                offset: 8,
                length: 4,
            },
            PolicyOperator::IngestAccount { index: 0 },
        ];
        let base = leaf(1, &data, &[account(4, false, false)], &operators)
            .unwrap()
            .0;
        let writable = leaf(1, &data, &[account(4, false, true)], &operators)
            .unwrap()
            .0;
        let other_version = leaf(2, &data, &[account(4, false, false)], &operators)
            .unwrap()
            .0;
        assert_ne!(base, writable);
        assert_ne!(base, other_version);
    }

    #[test]
    fn dynamic_amount_is_limited_without_being_committed() {
        let mut first = [0_u8; 16];
        first[..8].copy_from_slice(&[9; 8]);
        first[8..].copy_from_slice(&42_u64.to_le_bytes());
        let mut second = first;
        second[8..].copy_from_slice(&99_u64.to_le_bytes());
        let operators = [PolicyOperator::ManagerLimitAmount { offset: 8 }];

        let (first_leaf, first_amount) = leaf(1, &first, &[], &operators).unwrap();
        let (second_leaf, second_amount) = leaf(1, &second, &[], &operators).unwrap();
        assert_eq!(first_leaf, second_leaf);
        assert_eq!(first_amount, Some(42));
        assert_eq!(second_amount, Some(99));
    }

    #[test]
    fn invalid_operator_ranges_fail_instead_of_panicking() {
        let data = [1_u8; 8];
        let err = leaf(
            1,
            &data,
            &[],
            &[PolicyOperator::IngestInstruction {
                offset: u16::MAX,
                length: 1,
            }],
        )
        .unwrap_err();
        assert_eq!(err, AsyncVaultError::PolicyOperatorOutOfBounds.into());
    }

    #[test]
    fn sorted_pair_proof_supports_single_and_two_leaf_trees() {
        let a = [1_u8; 32];
        let b = [2_u8; 32];
        let root = hash_strategy_node(a, b);
        assert!(verify_strategy_proof(a, &[b], root).is_ok());
        assert!(verify_strategy_proof(b, &[a], root).is_ok());
        assert!(verify_strategy_proof(a, &[], a).is_ok());
        assert!(verify_strategy_proof(a, &[b], [3; 32]).is_err());
    }

    #[test]
    fn proof_depth_is_bounded() {
        let proof = vec![[2_u8; 32]; MAX_MERKLE_PROOF_DEPTH + 1];
        assert!(matches!(
            verify_strategy_proof([1; 32], &proof, [3; 32]),
            Err(error) if error == AsyncVaultError::MerkleProofTooDeep.into()
        ));
    }

    #[test]
    fn cumulative_instruction_ingestion_is_bounded() {
        let data = [7_u8; 1024];
        let operators = vec![
            PolicyOperator::IngestInstruction {
                offset: 0,
                length: 1024,
            };
            3
        ];
        let err = leaf(1, &data, &[], &operators).unwrap_err();
        assert_eq!(err, AsyncVaultError::PolicyIngestionLimitExceeded.into());
    }

    #[test]
    fn token_balance_adapter_hash_matches_client_vector() {
        let leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf {
            vault: Pubkey::default(),
            strategist: pubkey!("SysvarC1ock11111111111111111111111111111111"),
            policy_version: 7,
            venue_entry: pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
            vault_venue: pubkey!("BPFLoaderUpgradeab1e11111111111111111111111"),
            target_program: pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
            action: TokenBalanceAdapterAction::Deploy,
            asset_mint: pubkey!("Vote111111111111111111111111111111111111111"),
            vault_token_account: pubkey!("Stake11111111111111111111111111111111111111"),
            position: pubkey!("ComputeBudget111111111111111111111111111111"),
            position_token_account: pubkey!("SysvarC1ock11111111111111111111111111111111"),
            policy_max_amount: 250,
        });
        let encoded = leaf
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            encoded,
            "4c0920146de6af2cfc03030a24a4cf10d835a76bdafccc86394602928c4f066e"
        );
    }

    #[test]
    fn token_balance_adapter_leaf_binds_action_accounts_version_and_maximum() {
        let baseline = || TokenBalanceAdapterLeaf {
            vault: Pubkey::new_from_array([1; 32]),
            strategist: Pubkey::new_from_array([2; 32]),
            policy_version: 1,
            venue_entry: Pubkey::new_from_array([3; 32]),
            vault_venue: Pubkey::new_from_array([4; 32]),
            target_program: Pubkey::new_from_array([5; 32]),
            action: TokenBalanceAdapterAction::Deploy,
            asset_mint: Pubkey::new_from_array([6; 32]),
            vault_token_account: Pubkey::new_from_array([7; 32]),
            position: Pubkey::new_from_array([8; 32]),
            position_token_account: Pubkey::new_from_array([9; 32]),
            policy_max_amount: 100,
        };
        let baseline_hash = hash_token_balance_adapter_leaf(baseline());

        let mut changed = baseline();
        changed.action = TokenBalanceAdapterAction::Pull;
        assert_ne!(hash_token_balance_adapter_leaf(changed), baseline_hash);
        let mut changed = baseline();
        changed.position_token_account = Pubkey::new_from_array([10; 32]);
        assert_ne!(hash_token_balance_adapter_leaf(changed), baseline_hash);
        let mut changed = baseline();
        changed.policy_version = 2;
        assert_ne!(hash_token_balance_adapter_leaf(changed), baseline_hash);
        let mut changed = baseline();
        changed.policy_max_amount = 101;
        assert_ne!(hash_token_balance_adapter_leaf(changed), baseline_hash);
    }
}
