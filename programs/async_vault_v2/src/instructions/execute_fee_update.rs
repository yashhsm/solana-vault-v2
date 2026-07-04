use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    instructions::queue_fee_update::{apply_fee_update, PendingFeeUpdate},
    state::Vault,
};

#[derive(Accounts)]
pub struct ExecuteFeeUpdate<'info> {
    #[account(mut)]
    pub executor: Signer<'info>,

    #[account(
        mut,
        close = executor,
        constraint = pending_fee_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_fee_update: Account<'info, PendingFeeUpdate>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<ExecuteFeeUpdate>) -> Result<()> {
    let pending_fee_update = &ctx.accounts.pending_fee_update;
    let current_slot = Clock::get()?.slot;
    require!(
        current_slot >= pending_fee_update.eta_slot,
        AsyncVaultError::TimelockNotReady
    );
    require_keys_eq!(
        pending_fee_update.queued_by,
        ctx.accounts.vault.curator,
        AsyncVaultError::StaleTimelockAuthority
    );

    apply_fee_update(
        &ctx.accounts.vault.to_account_info(),
        pending_fee_update.args,
    )
}
