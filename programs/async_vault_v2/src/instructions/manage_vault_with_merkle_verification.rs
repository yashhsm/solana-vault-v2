use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
};
use anchor_spl::token_interface::Mint;

use crate::{
    error::AsyncVaultError,
    events::StrategyActionExecuted,
    state::{
        StrategyPolicy, Vault, VaultVenue, VenueEntry, MAX_MANAGE_CPI_ACCOUNTS,
        MAX_MANAGE_IX_DATA_LEN, STRATEGY_POLICY_SEED, VAULT_CONFIG_SEED, VAULT_VENUE_SEED,
    },
    utils::assert_generic_strategy_has_no_vault_token_writes,
    utils::merkle::{
        hash_strategy_leaf, verify_strategy_proof, PolicyAccountMeta, PolicyOperator, StrategyLeaf,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ManageVaultWithMerkleVerificationArgs {
    pub policy_version: u64,
    pub instruction_data: Vec<u8>,
    pub operators: Vec<PolicyOperator>,
    pub proof: Vec<[u8; 32]>,
}

#[derive(Accounts)]
pub struct ManageVaultWithMerkleVerification<'info> {
    pub strategist: Signer<'info>,

    #[account(
        mut,
        constraint = share_mint.key() == vault.share_mint @ AsyncVaultError::InvalidShareMint,
    )]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Box<Account<'info, Vault>>,

    pub venue_entry: Box<Account<'info, VenueEntry>>,

    #[account(
        seeds = [VAULT_VENUE_SEED, vault.key().as_ref(), venue_entry.key().as_ref()],
        bump = vault_venue.bump,
        constraint = vault_venue.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = vault_venue.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
    )]
    pub vault_venue: Box<Account<'info, VaultVenue>>,

    #[account(
        mut,
        seeds = [
            STRATEGY_POLICY_SEED,
            vault.key().as_ref(),
            strategist.key().as_ref(),
        ],
        bump = strategy_policy.bump,
        constraint = strategy_policy.vault == vault.key() @ AsyncVaultError::InvalidStrategyPolicy,
        constraint = strategy_policy.strategist == strategist.key() @ AsyncVaultError::InvalidStrategyPolicy,
    )]
    pub strategy_policy: Box<Account<'info, StrategyPolicy>>,

    /// CHECK: The key, executable bit, and venue binding are validated before CPI.
    pub target_program: UncheckedAccount<'info>,
}

pub fn handler<'info>(
    ctx: Context<'info, ManageVaultWithMerkleVerification<'info>>,
    args: ManageVaultWithMerkleVerificationArgs,
) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    ctx.accounts
        .strategy_policy
        .assert_bound(ctx.accounts.vault.key(), ctx.accounts.strategist.key())?;
    ctx.accounts.strategy_policy.assert_active()?;
    require!(
        args.policy_version == ctx.accounts.strategy_policy.version,
        AsyncVaultError::StaleStrategyPolicyVersion
    );

    ctx.accounts.venue_entry.assert_active()?;
    ctx.accounts.vault_venue.assert_active()?;
    ctx.accounts
        .vault_venue
        .assert_manager_authorized(&ctx.accounts.vault, ctx.accounts.strategist.key())?;

    let target_program = ctx.accounts.target_program.key();
    require_keys_eq!(
        target_program,
        ctx.accounts.venue_entry.target_program,
        AsyncVaultError::TargetProgramMismatch
    );
    require_keys_eq!(
        target_program,
        ctx.accounts.vault_venue.target_program,
        AsyncVaultError::TargetProgramMismatch
    );
    require!(
        ctx.accounts.target_program.executable,
        AsyncVaultError::TargetProgramNotExecutable
    );
    require_keys_neq!(
        target_program,
        crate::ID,
        AsyncVaultError::UnsafeTargetProgram
    );
    require!(
        args.instruction_data.len() <= MAX_MANAGE_IX_DATA_LEN,
        AsyncVaultError::InstructionDataTooLarge
    );
    require!(
        ctx.remaining_accounts.len() <= MAX_MANAGE_CPI_ACCOUNTS,
        AsyncVaultError::TooManyCpiAccounts
    );
    ctx.accounts
        .venue_entry
        .assert_instruction_allowed(&args.instruction_data)?;

    let vault_key = ctx.accounts.vault.key();
    let policy_accounts: Vec<PolicyAccountMeta> = ctx
        .remaining_accounts
        .iter()
        .map(|account| PolicyAccountMeta {
            key: account.key(),
            is_signer: account.is_signer || account.key() == vault_key,
            // The vault config is required mutably by this instruction for rolling-limit
            // accounting, but external venues receive it only as a read-only authority.
            is_writable: account.is_writable && account.key() != vault_key,
        })
        .collect();

    let (leaf, manager_limit_amount) = hash_strategy_leaf(StrategyLeaf {
        vault: vault_key,
        strategist: ctx.accounts.strategist.key(),
        policy_version: args.policy_version,
        target_program,
        instruction_data: &args.instruction_data,
        accounts: &policy_accounts,
        operators: &args.operators,
    })?;
    verify_strategy_proof(leaf, &args.proof, ctx.accounts.strategy_policy.merkle_root)?;

    // The generic path proves capability shape only. Any writable token account
    // controlled by the vault must use a typed adapter with mandatory balance and
    // ledger reconciliation.
    assert_generic_strategy_has_no_vault_token_writes(vault_key, ctx.remaining_accounts)?;

    if let Some(amount) = manager_limit_amount {
        ctx.accounts
            .vault
            .consume_manager_rolling_limit(amount, Clock::get()?.slot)?;
    }

    let share_supply_before = ctx.accounts.share_mint.supply;
    let cpi_metas = policy_accounts
        .iter()
        .map(|account| {
            if account.is_writable {
                AccountMeta::new(account.key, account.is_signer)
            } else {
                AccountMeta::new_readonly(account.key, account.is_signer)
            }
        })
        .collect();
    let instruction = Instruction {
        program_id: target_program,
        accounts: cpi_metas,
        data: args.instruction_data,
    };
    let mut account_infos: Vec<AccountInfo<'info>> = ctx.remaining_accounts.to_vec();
    account_infos.push(ctx.accounts.target_program.to_account_info());

    ctx.accounts.strategy_policy.executing = true;
    let share_mint_key = ctx.accounts.share_mint.key();
    let vault_bump = [ctx.accounts.vault.bump];
    let vault_signer_seeds: &[&[u8]] = &[
        VAULT_CONFIG_SEED,
        share_mint_key.as_ref(),
        vault_bump.as_ref(),
    ];
    // Anchor serializes accounts at instruction exit. Flush both guards before CPI so a
    // callback observes the executing flag and the consumed rolling-limit amount.
    ctx.accounts.strategy_policy.exit(ctx.program_id)?;
    ctx.accounts.vault.exit(ctx.program_id)?;
    invoke_signed(&instruction, &account_infos, &[vault_signer_seeds])?;
    ctx.accounts.strategy_policy.executing = false;

    ctx.accounts.share_mint.reload()?;
    require!(
        ctx.accounts.share_mint.supply == share_supply_before,
        AsyncVaultError::ShareSupplyChanged
    );

    emit!(StrategyActionExecuted {
        vault: vault_key,
        strategy_policy: ctx.accounts.strategy_policy.key(),
        strategist: ctx.accounts.strategist.key(),
        venue_entry: ctx.accounts.venue_entry.key(),
        target_program,
        policy_version: args.policy_version,
        leaf,
        cpi_account_count: u16::try_from(policy_accounts.len())
            .map_err(|_| AsyncVaultError::TooManyCpiAccounts)?,
        manager_limit_amount,
    });
    Ok(())
}
