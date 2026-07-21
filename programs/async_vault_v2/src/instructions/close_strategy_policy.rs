use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{StrategyPolicy, Vault, STRATEGY_POLICY_SEED},
};

#[derive(Accounts)]
pub struct CloseStrategyPolicy<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
        mut,
        close = authority,
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

pub fn handler(ctx: Context<CloseStrategyPolicy>) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    ctx.accounts.strategy_policy.assert_not_executing()
}
