use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{
    error::AsyncVaultError,
    state::{
        Position, Vault, VaultAsset, VaultVenue, VenueEntry, POSITION_SEED, POSITION_TOKEN_SEED,
        VAULT_VENUE_SEED,
    },
};

#[derive(Accounts)]
pub struct CreateVenuePosition<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    pub vault_asset: Option<Box<Account<'info, VaultAsset>>>,

    pub venue_entry: Box<Account<'info, VenueEntry>>,

    #[account(
        mut,
        seeds = [VAULT_VENUE_SEED, vault.key().as_ref(), venue_entry.key().as_ref()],
        bump = vault_venue.bump,
        constraint = vault_venue.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = vault_venue.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
    )]
    pub vault_venue: Box<Account<'info, VaultVenue>>,

    #[account(
        init,
        payer = payer,
        space = 8 + Position::INIT_SPACE,
        seeds = [
            POSITION_SEED,
            vault.key().as_ref(),
            venue_entry.key().as_ref(),
            asset_mint.key().as_ref(),
        ],
        bump,
    )]
    pub position: Box<Account<'info, Position>>,

    #[account(
        init,
        token::authority = vault,
        token::mint = asset_mint,
        token::token_program = asset_token_program,
        payer = payer,
        seeds = [
            POSITION_TOKEN_SEED,
            vault.key().as_ref(),
            venue_entry.key().as_ref(),
            asset_mint.key().as_ref(),
        ],
        bump,
    )]
    pub position_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<CreateVenuePosition>) -> Result<()> {
    let vault = &ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots == 0,
        AsyncVaultError::TimelockRequired
    );
    if !vault.is_primary_asset(ctx.accounts.asset_mint.key()) {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_ref()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.assert_matches(vault.key(), ctx.accounts.asset_mint.key())?;
    }
    ctx.accounts.venue_entry.assert_active()?;
    ctx.accounts.vault_venue.assert_active()?;

    ctx.accounts.position.set_inner(Position {
        vault: vault.key(),
        venue_entry: ctx.accounts.venue_entry.key(),
        vault_venue: ctx.accounts.vault_venue.key(),
        asset_mint: ctx.accounts.asset_mint.key(),
        token_account: ctx.accounts.position_token_account.key(),
        amount: 0,
        token_account_bump: ctx.bumps.position_token_account,
        bump: ctx.bumps.position,
    });

    ctx.accounts.vault_venue.position_count = ctx
        .accounts
        .vault_venue
        .position_count
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
