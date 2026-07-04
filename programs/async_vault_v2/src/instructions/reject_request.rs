use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    error::AsyncVaultError,
    extensions::{
        fifo_queue::{check_and_advance_queue, has_fifo_queue},
        redemption_queue::processor::{RedemptionQueue, RedemptionQueueRequest},
        subscription_queue::processor::{SubscriptionQueue, SubscriptionQueueRequest},
    },
    state::{Request, RequestState, RequestType, Vault, VaultAsset, VAULT_CONFIG_SEED},
    utils::{
        check_and_advance_tranche_queue, validate_request_share_mint, RequestShareMintContext,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct RejectRequestArgs {
    pub owner: Pubkey,
    pub request_type: RequestType,
    pub amount: u64,
    pub created_at: i64,
    pub nav_update_version: u64,
}

#[derive(Accounts)]
pub struct RejectRequest<'info> {
    pub authority: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, vault.share_mint.as_ref()],
        bump = vault.bump,
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

    /// CHECK: Validated against request.owner. Receives rent on account close.
    #[account(mut)]
    pub user: UncheckedAccount<'info>,

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

impl<'info> RejectRequest<'info> {
    #[inline(never)]
    pub fn transfer_assets_to_user(&self, amount: u64) -> Result<()> {
        let pending_vault = self
            .asset_pending_vault
            .as_ref()
            .ok_or(error!(AsyncVaultError::MissingRequiredAccount))?;
        let user_token_account = self
            .user_token_account
            .as_ref()
            .ok_or(error!(AsyncVaultError::MissingRequiredAccount))?;
        let asset_token_program = self
            .asset_token_program
            .as_ref()
            .ok_or(error!(AsyncVaultError::MissingRequiredAccount))?;

        let cpi_accounts = TransferChecked {
            from: pending_vault.to_account_info(),
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

    /// Enforces FIFO ordering for queued deposit and redeem requests.
    #[inline(never)]
    pub fn check_fifo_ordering<'a>(
        &self,
        share_mint_context: &RequestShareMintContext,
        remaining_accounts: &'a [AccountInfo<'a>],
    ) -> Result<()> {
        if matches!(self.request.request_type, RequestType::Deposit) {
            if share_mint_context.tranche_info.is_some() {
                if has_fifo_queue::<SubscriptionQueue>(&self.vault.to_account_info())? {
                    check_and_advance_tranche_queue::<SubscriptionQueueRequest>(
                        share_mint_context,
                        self.share_mint.key(),
                        RequestType::Deposit,
                        &self.request.to_account_info(),
                        remaining_accounts,
                    )?;
                }
            } else {
                check_and_advance_queue::<SubscriptionQueue, SubscriptionQueueRequest>(
                    &self.vault.to_account_info(),
                    &self.request.to_account_info(),
                )?;
            }
        }
        if matches!(self.request.request_type, RequestType::Redeem) {
            if share_mint_context.tranche_info.is_some() {
                if has_fifo_queue::<RedemptionQueue>(&self.vault.to_account_info())? {
                    check_and_advance_tranche_queue::<RedemptionQueueRequest>(
                        share_mint_context,
                        self.share_mint.key(),
                        RequestType::Redeem,
                        &self.request.to_account_info(),
                        remaining_accounts,
                    )?;
                }
            } else {
                check_and_advance_queue::<RedemptionQueue, RedemptionQueueRequest>(
                    &self.vault.to_account_info(),
                    &self.request.to_account_info(),
                )?;
            }
        }
        Ok(())
    }

    #[inline(never)]
    pub fn mint_shares(&self, amount: u64) -> Result<()> {
        let user_share_account = self
            .user_share_account
            .as_ref()
            .ok_or(error!(AsyncVaultError::MissingRequiredAccount))?;
        let share_token_program = self
            .share_token_program
            .as_ref()
            .ok_or(error!(AsyncVaultError::MissingRequiredAccount))?;

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
}

pub fn handler(ctx: Context<RejectRequest>, args: RejectRequestArgs) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator_or_fulfiller(ctx.accounts.authority.key())?;

    require!(
        ctx.accounts
            .request
            .request_state
            .eq(&RequestState::Pending),
        AsyncVaultError::RequestInvalidState
    );

    let request = &ctx.accounts.request;
    require!(
        request.owner == args.owner
            && request.request_type == args.request_type
            && request.amount == args.amount
            && request.created_at == args.created_at
            && request.nav_update_version == args.nav_update_version,
        AsyncVaultError::ApprovalRequestMismatch
    );
    let asset_mint_key = ctx.accounts.asset_mint.key();
    require_keys_eq!(
        request.asset_mint_address,
        asset_mint_key,
        AsyncVaultError::InvalidAssetMint
    );
    let share_mint_key = ctx.accounts.share_mint.key();
    require_keys_eq!(
        request.share_mint_address,
        share_mint_key,
        AsyncVaultError::InvalidShareMint
    );
    let share_mint_context = validate_request_share_mint(
        &ctx.accounts.vault,
        ctx.accounts.vault.key(),
        share_mint_key,
        ctx.remaining_accounts,
    )?;

    ctx.accounts
        .check_fifo_ordering(&share_mint_context, ctx.remaining_accounts)?;

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
