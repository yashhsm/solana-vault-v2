use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    error::AsyncVaultError,
    extensions::assert_externally_managed_withdrawals_enabled,
    state::{Vault, VaultVenue, VenueEntry, VAULT_CONFIG_SEED, VAULT_VENUE_SEED},
};

#[derive(Accounts)]
pub struct WithdrawAssets<'info> {
    pub authority: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        has_one = asset_mint @ AsyncVaultError::InvalidAssetMint,
        constraint = authority.key() == vault.curator @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub vault: Box<Account<'info, Vault>>,

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
        token::mint = asset_mint.key(),
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = vault.vault_token_account == vault_token_account.key(),
    )]
    pub vault_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        constraint = recipient_token_account.owner == vault_venue.recipient_authority
            @ AsyncVaultError::InvalidVenueRecipient,
    )]
    pub recipient_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

impl<'info> WithdrawAssets<'info> {
    pub fn transfer_assets_to_approved_recipient(&mut self, amount: u64) -> Result<()> {
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            self.vault.share_mint.as_ref(),
            &[self.vault.bump],
        ]];

        let cpi_accounts = TransferChecked {
            from: self.vault_token_account.to_account_info(),
            mint: self.asset_mint.to_account_info(),
            to: self.recipient_token_account.to_account_info(),
            authority: self.vault.to_account_info(),
        };

        let cpi_ctx =
            CpiContext::new_with_signer(self.asset_token_program.key(), cpi_accounts, seeds);

        token_interface::transfer_checked(cpi_ctx, amount, self.asset_mint.decimals)
    }
}

pub fn handler(ctx: Context<WithdrawAssets>, amount: u64) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    assert_externally_managed_withdrawals_enabled(&ctx.accounts.vault.to_account_info())?;
    ctx.accounts.venue_entry.assert_active()?;
    ctx.accounts.vault_venue.assert_active()?;
    ctx.accounts
        .vault
        .consume_external_withdraw_rolling_limit(amount, Clock::get()?.slot)?;
    ctx.accounts.transfer_assets_to_approved_recipient(amount)?;
    Ok(())
}
