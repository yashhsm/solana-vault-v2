use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::state::{Vault, VAULT_CONFIG_SEED};

#[derive(Accounts)]
pub struct PauseVault<'info> {
    pub breaker: Signer<'info>,

    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<PauseVault>) -> Result<()> {
    ctx.accounts
        .vault
        .assert_breaker(ctx.accounts.breaker.key())?;
    ctx.accounts.vault.paused = true;
    Ok(())
}
