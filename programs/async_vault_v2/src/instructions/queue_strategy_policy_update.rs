use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::StrategyPolicyUpdateQueued,
    instructions::update_strategy_policy::{
        validate_strategy_policy_update, StrategyPolicyUpdateArgs,
    },
    state::{StrategyPolicy, Vault, STRATEGY_POLICY_SEED},
};

#[account]
#[derive(InitSpace)]
pub struct PendingStrategyPolicyUpdate {
    pub vault: Pubkey,
    pub strategy_policy: Pubkey,
    pub queued_by: Pubkey,
    pub created_slot: u64,
    pub eta_slot: u64,
    pub expected_version: u64,
    pub args: StrategyPolicyUpdateArgs,
}

#[derive(Accounts)]
pub struct QueueStrategyPolicyUpdate<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
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
        init,
        payer = payer,
        space = 8 + PendingStrategyPolicyUpdate::INIT_SPACE,
    )]
    pub pending_update: Account<'info, PendingStrategyPolicyUpdate>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<QueueStrategyPolicyUpdate>,
    args: StrategyPolicyUpdateArgs,
) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    require!(
        ctx.accounts.vault.timelock_delay_slots > 0,
        AsyncVaultError::TimelockNotConfigured
    );
    validate_strategy_policy_update(&args)?;

    let current_slot = Clock::get()?.slot;
    let eta_slot = current_slot
        .checked_add(ctx.accounts.vault.timelock_delay_slots)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let expected_version = ctx.accounts.strategy_policy.version;

    ctx.accounts
        .pending_update
        .set_inner(PendingStrategyPolicyUpdate {
            vault: ctx.accounts.vault.key(),
            strategy_policy: ctx.accounts.strategy_policy.key(),
            queued_by: ctx.accounts.authority.key(),
            created_slot: current_slot,
            eta_slot,
            expected_version,
            args,
        });

    emit!(StrategyPolicyUpdateQueued {
        vault: ctx.accounts.vault.key(),
        strategy_policy: ctx.accounts.strategy_policy.key(),
        pending_update: ctx.accounts.pending_update.key(),
        expected_version,
        eta_slot,
        merkle_root: args.merkle_root,
        paused: args.paused,
    });
    Ok(())
}
