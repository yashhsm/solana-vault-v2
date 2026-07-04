use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError, instructions::queue_fee_update::PendingFeeUpdate, state::Vault,
};

#[derive(Accounts)]
pub struct CancelFeeUpdate<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(
        mut,
        close = authority,
        constraint = pending_fee_update.vault == vault.key() @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_fee_update: Account<'info, PendingFeeUpdate>,

    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<CancelFeeUpdate>) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    Ok(())
}
