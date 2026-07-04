use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    instructions::{
        queue_vault_update::PendingVaultUpdate,
        update_vault::{apply_vault_update, validate_queued_vault_update},
    },
    state::Vault,
};

#[derive(Accounts)]
pub struct ExecuteVaultUpdate<'info> {
    #[account(mut)]
    pub executor: Signer<'info>,

    #[account(
        mut,
        close = executor,
        constraint = pending_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_update: Account<'info, PendingVaultUpdate>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<ExecuteVaultUpdate>) -> Result<()> {
    let pending_update = &ctx.accounts.pending_update;
    let current_slot = Clock::get()?.slot;
    require!(
        current_slot >= pending_update.eta_slot,
        AsyncVaultError::TimelockNotReady
    );
    require_keys_eq!(
        pending_update.queued_by,
        ctx.accounts.vault.curator,
        AsyncVaultError::StaleTimelockAuthority
    );

    let args = pending_update.args.clone();
    validate_queued_vault_update(&ctx.accounts.vault, &args)?;
    apply_vault_update(&mut ctx.accounts.vault, &args)
}
