use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{Vault, VaultVenue, VenueEntry, VAULT_VENUE_SEED},
};

#[derive(Accounts)]
pub struct RemoveVaultVenue<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    pub venue_entry: Account<'info, VenueEntry>,

    #[account(
        mut,
        close = authority,
        seeds = [VAULT_VENUE_SEED, vault.key().as_ref(), venue_entry.key().as_ref()],
        bump = vault_venue.bump,
        constraint = vault_venue.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = vault_venue.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
    )]
    pub vault_venue: Account<'info, VaultVenue>,
}

pub fn handler(ctx: Context<RemoveVaultVenue>) -> Result<()> {
    let vault = &ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots == 0,
        AsyncVaultError::TimelockRequired
    );
    ctx.accounts.vault_venue.assert_empty()
}
