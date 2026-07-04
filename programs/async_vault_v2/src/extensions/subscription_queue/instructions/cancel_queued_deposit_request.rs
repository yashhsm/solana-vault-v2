use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    error::AsyncVaultError,
    extensions::{
        request_extensions::has_request_extension,
        subscription_queue::processor::SubscriptionQueueRequest,
    },
    state::{Request, RequestState, RequestType, Vault, VaultAsset, VAULT_CONFIG_SEED},
    utils::validate_request_share_mint,
};

#[derive(Accounts)]
pub struct CancelQueuedDepositRequest<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, vault.share_mint.as_ref()],
        bump = vault.bump
    )]
    pub vault: Box<Account<'info, Vault>>,

    #[account(mut)]
    pub vault_asset: Option<Box<Account<'info, VaultAsset>>>,

    #[account(
        mut,
        constraint = request.owner == user.key() @ AsyncVaultError::UnauthorizedSigner,
        constraint = request.request_type == RequestType::Deposit @ AsyncVaultError::InvalidRequestType,
        has_one = vault,
    )]
    pub request: Box<Account<'info, Request>>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = user
    )]
    pub user_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub asset_pending_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> CancelQueuedDepositRequest<'info> {
    /// Transfers deposited assets from the pending vault back to the user's token account,
    /// using the vault's PDA authority to sign the CPI transfer.
    pub fn transfer_assets_to_user(&self, amount: u64) -> Result<()> {
        let cpi_accounts = TransferChecked {
            from: self.asset_pending_vault.to_account_info(),
            mint: self.asset_mint.to_account_info(),
            to: self.user_token_account.to_account_info(),
            authority: self.vault.to_account_info(),
        };

        let base_share_mint = self.vault.share_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            base_share_mint.as_ref(),
            &[self.vault.bump],
        ]];
        let cpi_ctx =
            CpiContext::new_with_signer(self.asset_token_program.key(), cpi_accounts, seeds);
        token_interface::transfer_checked(cpi_ctx, amount, self.asset_mint.decimals)
    }

    /// Validate that the SubscriptionQueue Request Extension is present.
    pub fn validate_has_subscription_queue_extension(&self) -> Result<()> {
        let request_info = self.request.to_account_info();
        let request_data = request_info
            .data
            .try_borrow()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        let has_queue_ext = has_request_extension::<SubscriptionQueueRequest>(&request_data);
        require!(has_queue_ext, AsyncVaultError::UninitializedExtension);
        Ok(())
    }
}

/// Cancels a pending queued deposit request. Assets are refunded immediately. The request
/// account remains open as a tombstone so the subscription queue can advance past it via
/// `skip_canceled_queue_request`.
pub fn handler(ctx: Context<CancelQueuedDepositRequest>) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    require!(
        ctx.accounts.request.request_state == RequestState::Pending,
        AsyncVaultError::RequestIsNotPending,
    );
    ctx.accounts.validate_has_subscription_queue_extension()?;
    let asset_mint_key = ctx.accounts.asset_mint.key();
    let share_mint_key = ctx.accounts.share_mint.key();
    require_keys_eq!(
        ctx.accounts.request.asset_mint_address,
        asset_mint_key,
        AsyncVaultError::InvalidAssetMint
    );
    require_keys_eq!(
        ctx.accounts.request.share_mint_address,
        share_mint_key,
        AsyncVaultError::InvalidShareMint
    );
    validate_request_share_mint(
        &ctx.accounts.vault,
        ctx.accounts.vault.key(),
        share_mint_key,
        ctx.remaining_accounts,
    )?;
    if ctx.accounts.vault.is_primary_asset(asset_mint_key) {
        require_keys_eq!(
            ctx.accounts.vault.pending_vault,
            ctx.accounts.asset_pending_vault.key(),
            AsyncVaultError::InvalidPendingVault
        );
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_ref()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.assert_matches(ctx.accounts.vault.key(), asset_mint_key)?;
        vault_asset.assert_pending_vault(ctx.accounts.asset_pending_vault.key())?;
    }

    let refund_amount = ctx.accounts.request.amount;
    ctx.accounts.transfer_assets_to_user(refund_amount)?;

    ctx.accounts.request.request_state = RequestState::Canceled;
    if ctx.accounts.vault.is_primary_asset(asset_mint_key) {
        ctx.accounts.vault.pending_deposit_amount = ctx
            .accounts
            .vault
            .pending_deposit_amount
            .checked_sub(refund_amount)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_mut()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.pending_deposit_amount = vault_asset
            .pending_deposit_amount
            .checked_sub(refund_amount)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    }
    ctx.accounts.vault.pending_async_requests = ctx
        .accounts
        .vault
        .pending_async_requests
        .checked_sub(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
