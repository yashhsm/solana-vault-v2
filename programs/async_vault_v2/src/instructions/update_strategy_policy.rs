use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::StrategyPolicyUpdated,
    state::{StrategyPolicy, Vault, STRATEGY_POLICY_SEED},
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, InitSpace)]
pub struct StrategyPolicyUpdateArgs {
    pub merkle_root: [u8; 32],
    pub paused: bool,
}

#[derive(Accounts)]
pub struct UpdateStrategyPolicy<'info> {
    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
        mut,
        seeds = [
            STRATEGY_POLICY_SEED,
            vault.key().as_ref(),
            strategy_policy.strategist.as_ref(),
        ],
        bump = strategy_policy.bump,
        constraint = strategy_policy.vault == vault.key() @ AsyncVaultError::InvalidStrategyPolicy,
    )]
    pub strategy_policy: Account<'info, StrategyPolicy>,
}

pub fn validate_strategy_policy_update(args: &StrategyPolicyUpdateArgs) -> Result<()> {
    if !args.paused {
        require!(
            args.merkle_root != [0_u8; 32],
            AsyncVaultError::EmptyMerkleRoot
        );
    }
    Ok(())
}

pub fn apply_strategy_policy_update(
    strategy_policy: &mut Account<StrategyPolicy>,
    args: StrategyPolicyUpdateArgs,
) -> Result<()> {
    strategy_policy.assert_not_executing()?;
    validate_strategy_policy_update(&args)?;
    let old_root = strategy_policy.merkle_root;
    strategy_policy.merkle_root = args.merkle_root;
    strategy_policy.paused = args.paused;
    strategy_policy.version = strategy_policy
        .version
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    emit!(StrategyPolicyUpdated {
        vault: strategy_policy.vault,
        strategy_policy: strategy_policy.key(),
        strategist: strategy_policy.strategist,
        old_root,
        new_root: strategy_policy.merkle_root,
        version: strategy_policy.version,
        paused: strategy_policy.paused,
    });
    Ok(())
}

pub fn handler(ctx: Context<UpdateStrategyPolicy>, args: StrategyPolicyUpdateArgs) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    require!(
        ctx.accounts.vault.timelock_delay_slots == 0,
        AsyncVaultError::TimelockRequired
    );
    apply_strategy_policy_update(&mut ctx.accounts.strategy_policy, args)
}
