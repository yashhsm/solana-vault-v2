use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    instructions::queue_extension_update::{apply_extension_update, PendingExtensionUpdate},
    state::Vault,
};

#[derive(Accounts)]
pub struct ExecuteExtensionUpdate<'info> {
    #[account(mut)]
    pub executor: Signer<'info>,

    #[account(
        mut,
        close = executor,
        constraint = pending_extension_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_extension_update: Account<'info, PendingExtensionUpdate>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<ExecuteExtensionUpdate>) -> Result<()> {
    let pending_extension_update = &ctx.accounts.pending_extension_update;
    let current_slot = Clock::get()?.slot;
    require!(
        current_slot >= pending_extension_update.eta_slot,
        AsyncVaultError::TimelockNotReady
    );
    require_keys_eq!(
        pending_extension_update.queued_by,
        ctx.accounts.vault.curator,
        AsyncVaultError::StaleTimelockAuthority
    );

    apply_extension_update(
        &ctx.accounts.vault.to_account_info(),
        pending_extension_update.args,
    )
}
