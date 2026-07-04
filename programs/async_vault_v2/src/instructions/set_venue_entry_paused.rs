use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{VenueEntry, VENUE_ENTRY_SEED},
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct SetVenueEntryPausedArgs {
    pub paused: bool,
}

#[derive(Accounts)]
pub struct SetVenueEntryPaused<'info> {
    pub registry_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [
            VENUE_ENTRY_SEED,
            venue_entry.registry_authority.as_ref(),
            venue_entry.venue_id.as_ref(),
        ],
        bump = venue_entry.bump,
        constraint = venue_entry.registry_authority == registry_authority.key() @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub venue_entry: Account<'info, VenueEntry>,
}

pub fn handler(ctx: Context<SetVenueEntryPaused>, args: SetVenueEntryPausedArgs) -> Result<()> {
    ctx.accounts
        .venue_entry
        .assert_authority(ctx.accounts.registry_authority.key())?;
    ctx.accounts.venue_entry.paused = args.paused;
    Ok(())
}
