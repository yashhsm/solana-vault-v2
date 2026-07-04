use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError, instructions::queue_vault_update::PendingVaultUpdate, state::Vault,
};

#[derive(Accounts)]
pub struct CancelVaultUpdate<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        close = authority,
        constraint = pending_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_update: Account<'info, PendingVaultUpdate>,

    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<CancelVaultUpdate>) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    Ok(())
}
