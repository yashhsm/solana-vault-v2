use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::StrategyPolicyPaused,
    state::{StrategyPolicy, Vault, STRATEGY_POLICY_SEED},
};

#[derive(Accounts)]
pub struct PauseStrategyPolicy<'info> {
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

pub fn handler(ctx: Context<PauseStrategyPolicy>) -> Result<()> {
    require!(
        ctx.accounts.authority.key() == ctx.accounts.vault.curator
            || ctx.accounts.authority.key() == ctx.accounts.vault.breaker,
        AsyncVaultError::UnauthorizedSigner
    );
    ctx.accounts.strategy_policy.assert_not_executing()?;
    ctx.accounts.strategy_policy.paused = true;
    // Invalidate every proof and queued update created before the emergency stop.
    ctx.accounts.strategy_policy.version = ctx
        .accounts
        .strategy_policy
        .version
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    emit!(StrategyPolicyPaused {
        vault: ctx.accounts.vault.key(),
        strategy_policy: ctx.accounts.strategy_policy.key(),
        strategist: ctx.accounts.strategy_policy.strategist,
        authority: ctx.accounts.authority.key(),
        version: ctx.accounts.strategy_policy.version,
    });
    Ok(())
}
