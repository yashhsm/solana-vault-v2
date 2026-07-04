use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError, instructions::queue_extension_update::PendingExtensionUpdate,
    state::Vault,
};

#[derive(Accounts)]
pub struct CancelExtensionUpdate<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        close = authority,
        constraint = pending_extension_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_extension_update: Account<'info, PendingExtensionUpdate>,

    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<CancelExtensionUpdate>) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    Ok(())
}
