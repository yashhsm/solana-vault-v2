use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    error::AsyncVaultError,
    extensions::{
        redemption_queue::processor::RedemptionQueueRequest,
        request_extensions::has_request_extension,
        subscription_queue::processor::SubscriptionQueueRequest,
    },
    state::{Request, RequestState, RequestType, Vault, VaultAsset, VAULT_CONFIG_SEED},
    utils::validate_request_share_mint,
};

#[derive(Accounts)]
pub struct CancelRequest<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
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
        close = user,
        constraint = request.owner == user.key() @ AsyncVaultError::UnauthorizedSigner,
        has_one = vault,
    )]
    pub request: Box<Account<'info, Request>>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = user
    )]
    pub user_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(
        mut,
        token::mint = asset_mint.key(),
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub asset_pending_vault: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(
        mut,
        token::mint = share_mint.key(),
        token::authority = user,
        token::token_program = share_token_program,
    )]
    pub user_share_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    pub share_token_program: Option<Interface<'info, TokenInterface>>,
    pub asset_token_program: Option<Interface<'info, TokenInterface>>,
    pub system_program: Program<'info, System>,
}

impl<'info> CancelRequest<'info> {
    /// Transfers deposited assets from the pending vault back to the user's token account,
    /// using the vault's PDA authority to sign the CPI transfer.
    #[inline(never)]
    pub fn transfer_assets_to_user(&self, amount: u64) -> Result<()> {
        let asset_pending_vault = self
            .asset_pending_vault
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let user_token_account = self
            .user_token_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let asset_token_program = self
            .asset_token_program
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;

        let cpi_accounts = TransferChecked {
            from: asset_pending_vault.to_account_info(),
            mint: self.asset_mint.to_account_info(),
            to: user_token_account.to_account_info(),
            authority: self.vault.to_account_info(),
        };

        let base_share_mint = self.vault.share_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            base_share_mint.as_ref(),
            &[self.vault.bump],
        ]];
        let cpi_ctx = CpiContext::new_with_signer(asset_token_program.key(), cpi_accounts, seeds);
        token_interface::transfer_checked(cpi_ctx, amount, self.asset_mint.decimals)
    }

    /// Mints share tokens back to the user's share account to reverse a pending redeem request,
    /// using the vault's PDA authority to sign the CPI mint.
    #[inline(never)]
    pub fn mint_shares(&self, amount: u64) -> Result<()> {
        let user_share_account = self
            .user_share_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let share_token_program = self
            .share_token_program
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;

        let cpi_accounts = MintTo {
            mint: self.share_mint.to_account_info(),
            to: user_share_account.to_account_info(),
            authority: self.vault.to_account_info(),
        };

        let base_share_mint = self.vault.share_mint;
        let seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            base_share_mint.as_ref(),
            &[self.vault.bump],
        ]];
        let cpi_ctx = CpiContext::new_with_signer(share_token_program.key(), cpi_accounts, seeds);
        token_interface::mint_to(cpi_ctx, amount)
    }

    /// Validates that the request is not a queued deposit or queued redeem that requires its own
    /// cancel instruction. Extracted into a separate, non-inlined function to keep the main
    /// handler's BPF stack frame within the 4096-byte limit.
    #[inline(never)]
    pub fn validate_queue_extension_constraints(&self) -> Result<()> {
        let request_info = self.request.to_account_info();
        let request_data = request_info
            .data
            .try_borrow()
            .map_err(|_| ProgramError::AccountBorrowFailed)?;
        if self.request.request_type == RequestType::Deposit {
            let has_queue_ext = has_request_extension::<SubscriptionQueueRequest>(&request_data);
            require!(
                !has_queue_ext,
                AsyncVaultError::MustUseCancelQueuedDepositRequest,
            );
        }
        if self.request.request_type == RequestType::Redeem {
            let has_queue_ext = has_request_extension::<RedemptionQueueRequest>(&request_data);
            require!(
                !has_queue_ext,
                AsyncVaultError::MustUseCancelQueuedRedemptionRequest,
            );
        }
        Ok(())
    }
}

pub fn handler(ctx: Context<CancelRequest>) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    require!(
        ctx.accounts.request.request_state == RequestState::Pending,
        AsyncVaultError::RequestIsNotPending,
    );
    let asset_mint_key = ctx.accounts.asset_mint.key();
    require_keys_eq!(
        ctx.accounts.request.asset_mint_address,
        asset_mint_key,
        AsyncVaultError::InvalidAssetMint
    );
    let share_mint_key = ctx.accounts.share_mint.key();
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
    ctx.accounts.validate_queue_extension_constraints()?;
    match ctx.accounts.request.request_type {
        RequestType::Deposit => {
            let refund_amount = ctx.accounts.request.amount;
            if ctx.accounts.vault.is_primary_asset(asset_mint_key) {
                let pending_vault = ctx
                    .accounts
                    .asset_pending_vault
                    .as_ref()
                    .ok_or(AsyncVaultError::MissingRequiredAccount)?;
                require_keys_eq!(
                    ctx.accounts.vault.pending_vault,
                    pending_vault.key(),
                    AsyncVaultError::InvalidPendingVault
                );
            } else {
                let vault_asset = ctx
                    .accounts
                    .vault_asset
                    .as_ref()
                    .ok_or(AsyncVaultError::InvalidAssetMint)?;
                let pending_vault = ctx
                    .accounts
                    .asset_pending_vault
                    .as_ref()
                    .ok_or(AsyncVaultError::MissingRequiredAccount)?;
                vault_asset.assert_matches(ctx.accounts.vault.key(), asset_mint_key)?;
                vault_asset.assert_pending_vault(pending_vault.key())?;
            }
            ctx.accounts.transfer_assets_to_user(refund_amount)?;
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
        }
        RequestType::Redeem => {
            let shares = ctx.accounts.request.amount;
            ctx.accounts.mint_shares(shares)?;
        }
    }
    ctx.accounts.vault.pending_async_requests = ctx
        .accounts
        .vault
        .pending_async_requests
        .checked_sub(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
