use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{Vault, VaultVenue, VenueEntry, VAULT_VENUE_SEED},
};

#[derive(Accounts)]
pub struct ApproveVaultVenue<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Account<'info, Vault>,

    pub venue_entry: Account<'info, VenueEntry>,

    #[account(
        init,
        payer = payer,
        space = 8 + VaultVenue::INIT_SPACE,
        seeds = [VAULT_VENUE_SEED, vault.key().as_ref(), venue_entry.key().as_ref()],
        bump,
    )]
    pub vault_venue: Account<'info, VaultVenue>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<ApproveVaultVenue>) -> Result<()> {
    let vault = &ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots == 0,
        AsyncVaultError::TimelockRequired
    );
    ctx.accounts.venue_entry.assert_active()?;

    ctx.accounts.vault_venue.set_inner(VaultVenue {
        vault: vault.key(),
        venue_entry: ctx.accounts.venue_entry.key(),
        target_program: ctx.accounts.venue_entry.target_program,
        routine_safe: ctx.accounts.venue_entry.routine_safe,
        paused: false,
        position_count: 0,
        approved_at_slot: Clock::get()?.slot,
        bump: ctx.bumps.vault_venue,
    });

    Ok(())
}
