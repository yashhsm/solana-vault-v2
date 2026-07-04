use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    error::AsyncVaultError,
    state::{
        Position, PositionLedger, Vault, VaultAsset, VaultVenue, VenueEntry, POSITION_SEED,
        POSITION_TOKEN_SEED, VAULT_CONFIG_SEED, VAULT_VENUE_SEED,
    },
};

#[derive(Accounts)]
pub struct PullVenuePosition<'info> {
    pub authority: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    #[account(mut)]
    pub vault_asset: Option<Box<Account<'info, VaultAsset>>>,

    pub venue_entry: Box<Account<'info, VenueEntry>>,

    #[account(
        seeds = [VAULT_VENUE_SEED, vault.key().as_ref(), venue_entry.key().as_ref()],
        bump = vault_venue.bump,
        constraint = vault_venue.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = vault_venue.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
    )]
    pub vault_venue: Box<Account<'info, VaultVenue>>,

    #[account(
        mut,
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
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub vault_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

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

impl<'info> PullVenuePosition<'info> {
    fn transfer_to_vault(&self, amount: u64) -> Result<()> {
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            self.vault.share_mint.as_ref(),
            &[self.vault.bump],
        ]];
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: self.position_token_account.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to: self.vault_token_account.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }
}

pub fn handler(ctx: Context<PullVenuePosition>, amount: u64) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    ctx.accounts
        .vault_venue
        .assert_manager_authorized(&ctx.accounts.vault, ctx.accounts.authority.key())?;
    ctx.accounts.venue_entry.assert_active()?;
    ctx.accounts.vault_venue.assert_active()?;
    require!(
        ctx.accounts.position.amount >= amount,
        AsyncVaultError::InsufficientPositionBalance
    );
    let is_primary_asset = ctx
        .accounts
        .vault
        .is_primary_asset(ctx.accounts.asset_mint.key());
    let current_slot = Clock::get()?.slot;
    let secondary_ledger = if is_primary_asset {
        require_keys_eq!(
            ctx.accounts.vault.vault_token_account,
            ctx.accounts.vault_token_account.key(),
            AsyncVaultError::InvalidVault
        );
        ctx.accounts
            .vault
            .consume_manager_rolling_limit(amount, current_slot)?;
        None
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_mut()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.assert_matches(ctx.accounts.vault.key(), ctx.accounts.asset_mint.key())?;
        vault_asset.assert_reserve(ctx.accounts.vault_token_account.key())?;
        require!(
            vault_asset.deployed_balance >= amount,
            AsyncVaultError::InsufficientPositionBalance
        );
        vault_asset.consume_manager_rolling_limit(
            amount,
            current_slot,
            ctx.accounts.vault.manager_rolling_limit,
            ctx.accounts.vault.rolling_limit_window_slots,
        )?;
        Some(
            PositionLedger::new(
                ctx.accounts.position.amount,
                vault_asset.idle_balance,
                vault_asset.deployed_balance,
            )
            .pull(amount)?,
        )
    };

    let vault_before = ctx.accounts.vault_token_account.amount;
    let position_before = ctx.accounts.position_token_account.amount;
    ctx.accounts.transfer_to_vault(amount)?;
    ctx.accounts.vault_token_account.reload()?;
    ctx.accounts.position_token_account.reload()?;

    let expected_vault_after = vault_before
        .checked_add(amount)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let expected_position_after = position_before
        .checked_sub(amount)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    require!(
        ctx.accounts.vault_token_account.amount == expected_vault_after
            && ctx.accounts.position_token_account.amount == expected_position_after,
        AsyncVaultError::InvalidBalanceDelta
    );

    if let Some(ledger) = secondary_ledger {
        ctx.accounts.position.amount = ledger.position_amount;
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_mut()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.idle_balance = ledger.idle_balance;
        vault_asset.deployed_balance = ledger.deployed_balance;
    } else {
        ctx.accounts.position.amount = ctx
            .accounts
            .position
            .amount
            .checked_sub(amount)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    }

    Ok(())
}
