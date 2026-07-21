use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    instructions::{
        queue_strategy_policy_update::PendingStrategyPolicyUpdate,
        update_strategy_policy::apply_strategy_policy_update,
    },
    state::{StrategyPolicy, Vault, STRATEGY_POLICY_SEED},
};

#[derive(Accounts)]
pub struct ExecuteStrategyPolicyUpdate<'info> {
    #[account(mut)]
    pub executor: Signer<'info>,

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

    #[account(
        mut,
        close = executor,
        constraint = pending_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
        constraint = pending_update.strategy_policy == strategy_policy.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_update: Account<'info, PendingStrategyPolicyUpdate>,
}

pub fn handler(ctx: Context<ExecuteStrategyPolicyUpdate>) -> Result<()> {
    let pending = &ctx.accounts.pending_update;
    require!(
        Clock::get()?.slot >= pending.eta_slot,
        AsyncVaultError::TimelockNotReady
    );
    require_keys_eq!(
        pending.queued_by,
        ctx.accounts.vault.curator,
        AsyncVaultError::StaleTimelockAuthority
    );
    require!(
        ctx.accounts.strategy_policy.version == pending.expected_version,
        AsyncVaultError::StaleStrategyPolicyVersion
    );
    let args = pending.args;
    apply_strategy_policy_update(&mut ctx.accounts.strategy_policy, args)
}
