use crate::error::AsyncVaultError;
use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Burn, Mint, TokenAccount, TokenInterface};

use crate::{
    extensions::{
        self,
        fifo_queue::{has_fifo_queue, next_queue_request_id},
        redemption_queue::processor::{RedemptionQueue, RedemptionQueueRequest},
        request_extensions::{compute_request_extension_space, init_request_extension},
    },
    state::{Request, RequestState, RequestType, Vault, VaultAsset, VAULT_CONFIG_SEED},
    utils::{
        enforce_tranche_request_limits, next_tranche_queue_request_id, validate_request_share_mint,
    },
};

use super::create_deposit_request::RequestArgs;

#[derive(Accounts)]
pub struct CreateRedeemRequest<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut, seeds = [VAULT_CONFIG_SEED, vault.share_mint.as_ref()], bump = vault.bump)]
    pub vault: Box<Account<'info, Vault>>,

    pub vault_asset: Option<Box<Account<'info, VaultAsset>>>,

    // Space is extended conditionally: if RedemptionQueue is active on the vault,
    // extra bytes are reserved for the RedemptionQueueRequest TLV extension.
    #[account(
        init,
        space = 8 + Request::INIT_SPACE + compute_request_extension_space(&vault.to_account_info()),
        payer = user,
    )]
    pub request: Account<'info, Request>,

    #[account(
        mut,
        token::mint = share_mint.key(),
        token::authority = user
    )]
    pub user_share_account: InterfaceAccount<'info, TokenAccount>,

    pub share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> CreateRedeemRequest<'info> {
    /// Burn User shares
    pub fn burn_shares(&self, amount: u64) -> Result<()> {
        let cpi_accounts = Burn {
            mint: self.share_mint.to_account_info(),
            from: self.user_share_account.to_account_info(),
            authority: self.user.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(self.share_token_program.key(), cpi_accounts);
        token_interface::burn(cpi_ctx, amount)
    }
}

pub fn handler(ctx: Context<CreateRedeemRequest>, args: RequestArgs) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    let vault_key = ctx.accounts.vault.key();
    let asset_mint_key = ctx.accounts.asset_mint.key();
    let share_mint_key = ctx.accounts.share_mint.key();
    let share_mint_context = validate_request_share_mint(
        &ctx.accounts.vault,
        vault_key,
        share_mint_key,
        ctx.remaining_accounts,
    )?;
    if !ctx.accounts.vault.is_primary_asset(asset_mint_key) {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_ref()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.assert_matches(vault_key, asset_mint_key)?;
    }

    // Extension: PausableRedemption handling
    extensions::pausable_redemptions::check_redemptions_paused(
        &ctx.accounts.vault.to_account_info(),
    )?;

    extensions::min_redemption::check_min_redemption_amount(
        &ctx.accounts.vault.to_account_info(),
        args.amount,
    )?;

    require!(args.amount > 0, AsyncVaultError::InsufficientRedeemAmount);
    enforce_tranche_request_limits(
        &share_mint_context,
        share_mint_key,
        RequestType::Redeem,
        args.amount,
    )?;

    ctx.accounts.burn_shares(args.amount)?;

    let current_timestamp = Clock::get()?.unix_timestamp;
    ctx.accounts.request.set_inner(Request {
        vault: ctx.accounts.vault.key(),
        request_type: RequestType::Redeem,
        request_state: RequestState::Pending,
        owner: ctx.accounts.user.key(),
        amount: args.amount,
        price: share_mint_context
            .tranche_nav
            .filter(|nav| *nav > 0)
            .unwrap_or(ctx.accounts.vault.nav),
        asset_mint_address: asset_mint_key,
        share_mint_address: share_mint_key,
        created_at: current_timestamp,
        nav_update_version: ctx.accounts.vault.nav_version,
        operator: args.operator,
    });

    ctx.accounts.vault.pending_async_requests = ctx
        .accounts
        .vault
        .pending_async_requests
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    // Extension: RedemptionQueue — increment counter and tag the request with its ID.
    let queue_id = if share_mint_context.tranche_info.is_some() {
        if has_fifo_queue::<RedemptionQueue>(&ctx.accounts.vault.to_account_info())? {
            next_tranche_queue_request_id(
                &share_mint_context,
                share_mint_key,
                RequestType::Redeem,
                ctx.remaining_accounts,
            )?
        } else {
            None
        }
    } else {
        next_queue_request_id::<RedemptionQueue>(&ctx.accounts.vault.to_account_info())?
    };
    if let Some(id) = queue_id {
        init_request_extension(
            &ctx.accounts.request.to_account_info(),
            &RedemptionQueueRequest { id },
        )?;
    }

    Ok(())
}
