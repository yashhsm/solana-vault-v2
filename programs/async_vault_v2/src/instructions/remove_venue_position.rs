use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, CloseAccount, Mint, TokenAccount, TokenInterface};

use crate::{
    error::AsyncVaultError,
    state::{
        Position, Vault, VaultAsset, VaultVenue, VenueEntry, POSITION_SEED, POSITION_TOKEN_SEED,
        VAULT_CONFIG_SEED, VAULT_VENUE_SEED,
    },
};

#[derive(Accounts)]
pub struct RemoveVenuePosition<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    pub vault: Box<Account<'info, Vault>>,

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
        mut,
        close = authority,
        seeds = [
            POSITION_SEED,
            vault.key().as_ref(),
            venue_entry.key().as_ref(),
            asset_mint.key().as_ref(),
        ],
        bump = position.bump,
        constraint = position.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = position.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
        constraint = position.vault_venue == vault_venue.key() @ AsyncVaultError::InvalidVenueEntry,
        constraint = position.asset_mint == asset_mint.key() @ AsyncVaultError::InvalidAssetMint,
    )]
    pub position: Box<Account<'info, Position>>,

    #[account(
        mut,
        seeds = [
            POSITION_TOKEN_SEED,
            vault.key().as_ref(),
            venue_entry.key().as_ref(),
            asset_mint.key().as_ref(),
        ],
        bump = position.token_account_bump,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = position.token_account == position_token_account.key() @ AsyncVaultError::InvalidPosition,
    )]
    pub position_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

impl<'info> RemoveVenuePosition<'info> {
    fn close_position_token_account(&self) -> Result<()> {
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            self.vault.share_mint.as_ref(),
            &[self.vault.bump],
        ]];
        token_interface::close_account(CpiContext::new_with_signer(
            self.asset_token_program.key(),
            CloseAccount {
                account: self.position_token_account.to_account_info(),
                destination: self.authority.to_account_info(),
                authority: self.vault.to_account_info(),
            },
            seeds,
        ))
    }
}

pub fn handler(ctx: Context<RemoveVenuePosition>) -> Result<()> {
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
    ctx.accounts.position.assert_empty()?;
    require!(
        ctx.accounts.position_token_account.amount == 0,
        AsyncVaultError::PositionBalanceNonZero
    );

    ctx.accounts.vault_venue.position_count = ctx
        .accounts
        .vault_venue
        .position_count
        .checked_sub(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    ctx.accounts.close_position_token_account()
}
