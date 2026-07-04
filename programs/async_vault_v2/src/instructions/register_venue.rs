use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{
        VenueEntry, VenueType, MAX_VENUE_DISCRIMINATORS, MAX_VENUE_DISCRIMINATOR_BYTES,
        VENUE_ENTRY_SEED,
    },
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct RegisterVenueArgs {
    pub venue_id: [u8; 32],
    pub target_program: Pubkey,
    pub allowed_discriminator_count: u8,
    pub allowed_discriminators: [u8; MAX_VENUE_DISCRIMINATOR_BYTES],
    pub risk_class: u8,
    pub venue_type: VenueType,
    pub routine_safe: bool,
}

#[derive(Accounts)]
#[instruction(args: RegisterVenueArgs)]
pub struct RegisterVenue<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub registry_authority: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + VenueEntry::INIT_SPACE,
        seeds = [
            VENUE_ENTRY_SEED,
            registry_authority.key().as_ref(),
            args.venue_id.as_ref(),
        ],
        bump,
    )]
    pub venue_entry: Account<'info, VenueEntry>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<RegisterVenue>, args: RegisterVenueArgs) -> Result<()> {
    let discriminator_count = usize::from(args.allowed_discriminator_count);
    require!(
        discriminator_count > 0 && discriminator_count <= MAX_VENUE_DISCRIMINATORS,
        AsyncVaultError::InvalidVenueDiscriminatorCount
    );

    let mut allowed_discriminators = [0_u8; MAX_VENUE_DISCRIMINATOR_BYTES];
    let active_len = discriminator_count
        .checked_mul(8)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    allowed_discriminators[..active_len]
        .copy_from_slice(&args.allowed_discriminators[..active_len]);

    ctx.accounts.venue_entry.set_inner(VenueEntry {
        registry_authority: ctx.accounts.registry_authority.key(),
        venue_id: args.venue_id,
        target_program: args.target_program,
        allowed_discriminator_count: args.allowed_discriminator_count,
        allowed_discriminators,
        risk_class: args.risk_class,
        venue_type: args.venue_type,
        routine_safe: args.routine_safe,
        paused: false,
        bump: ctx.bumps.venue_entry,
    });

    Ok(())
}
