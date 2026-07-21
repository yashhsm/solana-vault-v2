use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    instructions::queue_strategy_policy_update::PendingStrategyPolicyUpdate, state::Vault,
};

#[derive(Accounts)]
pub struct CancelStrategyPolicyUpdate<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
        mut,
        close = authority,
        constraint = pending_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_update: Account<'info, PendingStrategyPolicyUpdate>,
}

pub fn handler(ctx: Context<CancelStrategyPolicyUpdate>) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())
}
