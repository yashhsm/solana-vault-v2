use anchor_lang::prelude::*;
use anchor_spl::{
    token, token_2022,
    token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked},
};

use crate::{
    error::AsyncVaultError,
    extensions::{
        fee::processor::{get_deposit_fee_and_net, get_withdrawal_fee_and_net},
        fifo_queue::{check_and_advance_queue, has_fifo_queue},
        redemption_queue::processor::{RedemptionQueue, RedemptionQueueRequest},
        subscription_queue::processor::{SubscriptionQueue, SubscriptionQueueRequest},
    },
    state::{
        Request, RequestState, RequestType, TrancheConfig, Vault, VaultAsset, VAULT_CONFIG_SEED,
    },
    utils::{
        calculate_assets, calculate_shares, check_and_advance_tranche_queue,
        read_mint_supply_and_decimals, resolve_protocol_fee_recipient, split_protocol_fee,
        validate_asset_mint_extensions_from_acct_info, validate_request_share_mint,
        validate_token_account_owner, TrancheRequestInfo,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct ApproveRequestArgs {
    pub owner: Pubkey,
    pub request_type: RequestType,
    pub amount: u64,
    pub created_at: i64,
    pub nav_update_version: u64,
}

#[derive(Accounts)]
pub struct ApproveRequest<'info> {
    pub authority: Signer<'info>,

    pub asset_mint: InterfaceAccount<'info, Mint>,
    pub share_mint: InterfaceAccount<'info, Mint>,

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
        has_one = vault @ AsyncVaultError::InvalidRequest,
    )]
    pub request: Box<Account<'info, Request>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub vault_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub pending_vault: InterfaceAccount<'info, TokenAccount>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

// TODO [SYSTEM DESIGN]: As this is currently written, the fee_recipient_token_account is only
// required if the DepositFee|WithrawFee Extension is enabled AND produces a fee > 0.
// This creates an inconsistent API at the expense of a very minor optimization.

impl<'info> ApproveRequest<'info> {
    /// Transfers assets from the pending vault (aka escrow) to the supplied
    /// TokenAccount.
    fn transfer_asset_from_pending_vault(
        &self,
        to: AccountInfo<'info>,
        amount: u64,
        seeds: &[&[&[u8]]],
    ) -> Result<()> {
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: self.pending_vault.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to,
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }

    /// Transfers assets from the vault to the supplied TokenAccount.
    fn transfer_asset_from_vault(
        &self,
        to: AccountInfo<'info>,
        amount: u64,
        seeds: &[&[&[u8]]],
    ) -> Result<()> {
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from: self.vault_token_account.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to,
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }

    /// Transfers assets from pending vault to vault, enabling the Authority to withdraw
    /// in a future transaction.
    fn settle_deposit(&self, seeds: &[&[&[u8]]], amount: u64) -> Result<()> {
        self.transfer_asset_from_pending_vault(
            self.vault_token_account.to_account_info(),
            amount,
            seeds,
        )
    }

    /// Transfers assets from vault to pending vault, removing them from the supply
    /// that the Authority may withdraw from.
    fn settle_redeem(&self, seeds: &[&[&[u8]]], assets: u64) -> Result<()> {
        self.transfer_asset_from_vault(self.pending_vault.to_account_info(), assets, seeds)
    }
}

fn assets_from_supply(supply: u64, decimals: u8, nav: u128) -> Result<u128> {
    if supply == 0 || nav == 0 {
        return Ok(0);
    }
    let precision = 10u128
        .checked_pow(decimals as u32)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    u128::from(supply)
        .checked_mul(nav)
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(precision)
        .ok_or(AsyncVaultError::ArithmeticError.into())
}

fn require_token_mint_account(info: &AccountInfo) -> Result<()> {
    require!(
        *info.owner == token::ID || *info.owner == token_2022::ID,
        AsyncVaultError::InvalidShareMint
    );
    Ok(())
}

fn post_approval_supply(
    cached_supply: u64,
    live_supply_after_request: u64,
    request_type: RequestType,
    share_delta: u64,
) -> Result<u64> {
    if matches!(request_type, RequestType::Deposit) {
        return cached_supply
            .max(live_supply_after_request)
            .checked_add(share_delta)
            .ok_or(AsyncVaultError::ArithmeticError.into());
    }

    let live_supply_before_request = live_supply_after_request
        .checked_add(share_delta)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    cached_supply
        .max(live_supply_before_request)
        .checked_sub(share_delta)
        .ok_or(AsyncVaultError::ArithmeticError.into())
}

fn maybe_enforce_junior_ratio_floor<'info, I>(
    vault: &Vault,
    selected_share_mint: Pubkey,
    request_type: RequestType,
    share_delta: u64,
    tranche_info: Option<TrancheRequestInfo>,
    remaining: &mut I,
) -> Result<()>
where
    I: Iterator<Item = &'info AccountInfo<'info>>,
{
    let Some(tranche_info) = tranche_info else {
        return Ok(());
    };
    if tranche_info.min_junior_ratio_bps == 0 {
        return Ok(());
    }

    let is_senior_deposit = matches!(request_type, RequestType::Deposit)
        && selected_share_mint == tranche_info.senior_share_mint;
    let is_junior_redeem = matches!(request_type, RequestType::Redeem)
        && selected_share_mint == tranche_info.junior_share_mint;
    if !is_senior_deposit && !is_junior_redeem {
        return Ok(());
    }

    let senior_share_mint_info = remaining
        .next()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;
    let junior_share_mint_info = remaining
        .next()
        .ok_or(AsyncVaultError::MissingRequiredAccount)?;

    require_keys_eq!(
        *senior_share_mint_info.key,
        tranche_info.senior_share_mint,
        AsyncVaultError::InvalidShareMint
    );
    require_keys_eq!(
        *junior_share_mint_info.key,
        tranche_info.junior_share_mint,
        AsyncVaultError::InvalidShareMint
    );
    require_token_mint_account(senior_share_mint_info)?;
    require_token_mint_account(junior_share_mint_info)?;

    let (senior_live_supply, senior_decimals) =
        read_mint_supply_and_decimals(senior_share_mint_info)?;
    let (junior_live_supply, junior_decimals) =
        read_mint_supply_and_decimals(junior_share_mint_info)?;

    let senior_supply = if selected_share_mint == tranche_info.senior_share_mint {
        post_approval_supply(
            tranche_info.senior_supply,
            senior_live_supply,
            request_type,
            share_delta,
        )?
    } else {
        senior_live_supply.max(tranche_info.senior_supply)
    };
    let junior_supply = if selected_share_mint == tranche_info.junior_share_mint {
        post_approval_supply(
            tranche_info.junior_supply,
            junior_live_supply,
            request_type,
            share_delta,
        )?
    } else {
        junior_live_supply.max(tranche_info.junior_supply)
    };

    let senior_nav = if tranche_info.senior_nav > 0 {
        tranche_info.senior_nav
    } else {
        vault.nav
    };
    let junior_nav = if tranche_info.junior_nav > 0 {
        tranche_info.junior_nav
    } else {
        vault.nav
    };
    let senior_assets = assets_from_supply(senior_supply, senior_decimals, senior_nav)?;
    let junior_assets = assets_from_supply(junior_supply, junior_decimals, junior_nav)?;
    let total_assets = senior_assets
        .checked_add(junior_assets)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    if total_assets == 0 {
        return Ok(());
    }

    let junior_ratio_bps = junior_assets
        .checked_mul(u128::from(vault_common::MAX_BPS))
        .ok_or(AsyncVaultError::ArithmeticError)?
        .checked_div(total_assets)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    require!(
        junior_ratio_bps >= u128::from(tranche_info.min_junior_ratio_bps),
        AsyncVaultError::JuniorRatioBelowMinimum
    );

    Ok(())
}

fn update_tranche_supply_cache<'info>(
    selected_share_mint: Pubkey,
    request_type: RequestType,
    share_delta: u64,
    selected_live_supply_after_request: u64,
    tranche_info: Option<TrancheRequestInfo>,
    tranche_config_info: Option<&'info AccountInfo<'info>>,
) -> Result<()> {
    let Some(tranche_info) = tranche_info else {
        return Ok(());
    };

    let tranche_config_info = tranche_config_info.ok_or(AsyncVaultError::MissingRequiredAccount)?;
    require!(
        tranche_config_info.is_writable,
        AsyncVaultError::MissingRequiredAccount
    );
    let mut tranche_config: Account<TrancheConfig> = Account::try_from(tranche_config_info)?;

    if selected_share_mint == tranche_info.senior_share_mint {
        tranche_config.senior_supply = post_approval_supply(
            tranche_info.senior_supply,
            selected_live_supply_after_request,
            request_type,
            share_delta,
        )?;
    } else if selected_share_mint == tranche_info.junior_share_mint {
        tranche_config.junior_supply = post_approval_supply(
            tranche_info.junior_supply,
            selected_live_supply_after_request,
            request_type,
            share_delta,
        )?;
    }
    tranche_config.exit(&crate::ID)?;

    Ok(())
}

pub fn handler<'info>(
    ctx: Context<'info, ApproveRequest<'info>>,
    args: ApproveRequestArgs,
) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    ctx.accounts
        .vault
        .assert_curator_or_fulfiller(ctx.accounts.authority.key())?;

    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;

    require!(
        matches!(ctx.accounts.request.request_state, RequestState::Pending),
        AsyncVaultError::RequestNotPending
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
    let request_share_mint_key = ctx.accounts.share_mint.key();
    require_keys_eq!(
        request.share_mint_address,
        request_share_mint_key,
        AsyncVaultError::InvalidShareMint
    );
    let share_mint_context = validate_request_share_mint(
        &ctx.accounts.vault,
        ctx.accounts.vault.key(),
        request_share_mint_key,
        ctx.remaining_accounts,
    )?;

    let vault_key = ctx.accounts.vault.key();
    let is_primary_asset = ctx.accounts.vault.is_primary_asset(asset_mint_key);
    if is_primary_asset {
        require_keys_eq!(
            ctx.accounts.vault.vault_token_account,
            ctx.accounts.vault_token_account.key(),
            AsyncVaultError::InvalidVault
        );
        require_keys_eq!(
            ctx.accounts.vault.pending_vault,
            ctx.accounts.pending_vault.key(),
            AsyncVaultError::InvalidPendingVault
        );
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_ref()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.assert_matches(vault_key, asset_mint_key)?;
        vault_asset.assert_reserve(ctx.accounts.vault_token_account.key())?;
        vault_asset.assert_pending_vault(ctx.accounts.pending_vault.key())?;
        return err!(AsyncVaultError::UnsupportedPhaseConfig);
    }

    require!(ctx.accounts.vault.nav > 0, AsyncVaultError::NavIsNotSet);
    ctx.accounts
        .vault
        .assert_fresh_nav_for_request(ctx.accounts.request.nav_update_version)?;
    let current_slot = Clock::get()?.slot;
    ctx.accounts.vault.assert_nav_not_stale(current_slot)?;

    let nav = share_mint_context
        .tranche_nav
        .filter(|nav| *nav > 0)
        .unwrap_or(ctx.accounts.vault.nav);
    let decimals = ctx.accounts.share_mint.decimals;
    let base_share_mint_key = ctx.accounts.vault.share_mint;
    let vault_bump = ctx.accounts.vault.bump;
    let seeds: &[&[&[u8]]] = &[&[
        VAULT_CONFIG_SEED,
        base_share_mint_key.as_ref(),
        &[vault_bump],
    ]];

    let is_deposit = matches!(ctx.accounts.request.request_type, RequestType::Deposit);
    let original_amount = ctx.accounts.request.amount;

    // Extension: SubscriptionQueue/RedemptionQueue — enforce FIFO ordering.
    if is_deposit {
        if share_mint_context.tranche_info.is_some() {
            if has_fifo_queue::<SubscriptionQueue>(&ctx.accounts.vault.to_account_info())? {
                check_and_advance_tranche_queue::<SubscriptionQueueRequest>(
                    &share_mint_context,
                    request_share_mint_key,
                    RequestType::Deposit,
                    &ctx.accounts.request.to_account_info(),
                    ctx.remaining_accounts,
                )?;
            }
        } else {
            check_and_advance_queue::<SubscriptionQueue, SubscriptionQueueRequest>(
                &ctx.accounts.vault.to_account_info(),
                &ctx.accounts.request.to_account_info(),
            )?;
        }
    } else if share_mint_context.tranche_info.is_some() {
        if has_fifo_queue::<RedemptionQueue>(&ctx.accounts.vault.to_account_info())? {
            check_and_advance_tranche_queue::<RedemptionQueueRequest>(
                &share_mint_context,
                request_share_mint_key,
                RequestType::Redeem,
                &ctx.accounts.request.to_account_info(),
                ctx.remaining_accounts,
            )?;
        }
    } else {
        check_and_advance_queue::<RedemptionQueue, RedemptionQueueRequest>(
            &ctx.accounts.vault.to_account_info(),
            &ctx.accounts.request.to_account_info(),
        )?;
    }

    let mut remaining = ctx
        .remaining_accounts
        .iter()
        .skip(share_mint_context.consumed_accounts)
        .peekable();
    let tranche_config_info = ctx.remaining_accounts.first();

    // Transfer assets between Vault and Pending Vault (aka escrow)
    let (claimable_amount, balance_delta) = if is_deposit {
        // Check for DepositFee Extension and calculate fee owed
        let (deposit_fee, net_deposit) =
            get_deposit_fee_and_net(&ctx.accounts.vault.to_account_info(), original_amount)?;
        require!(net_deposit > 0, AsyncVaultError::InsufficientDepositAmount);
        // Shares to be minted, floored (protocol favorable)
        let shares = calculate_shares(nav, decimals, net_deposit)?;
        require!(shares > 0, AsyncVaultError::InsufficientDepositAmount);
        maybe_enforce_junior_ratio_floor(
            &ctx.accounts.vault,
            request_share_mint_key,
            ctx.accounts.request.request_type,
            shares,
            share_mint_context.tranche_info,
            &mut remaining,
        )?;
        update_tranche_supply_cache(
            request_share_mint_key,
            ctx.accounts.request.request_type,
            shares,
            ctx.accounts.share_mint.supply,
            share_mint_context.tranche_info,
            tranche_config_info,
        )?;
        let (protocol_fee, fee_recipient_fee) =
            split_protocol_fee(deposit_fee, ctx.accounts.vault.protocol_fee_bps)?;
        let fee_recipient_token_account_info = if fee_recipient_fee > 0 {
            let fee_recipient_token_account_info = remaining
                .next()
                .ok_or(AsyncVaultError::MissingFeeRecipient)?;
            validate_token_account_owner(
                fee_recipient_token_account_info,
                &ctx.accounts.vault.fee_recipient,
            )?;
            Some(fee_recipient_token_account_info)
        } else {
            None
        };
        let protocol_fee_recipient_token_account_info = if protocol_fee > 0 {
            let (protocol_fee_recipient, consumed_protocol_fee_config) =
                resolve_protocol_fee_recipient(&ctx.accounts.vault, remaining.peek().copied())?;
            if consumed_protocol_fee_config {
                remaining.next();
            }
            let protocol_fee_recipient_token_account_info = remaining
                .next()
                .ok_or(AsyncVaultError::MissingFeeRecipient)?;
            validate_token_account_owner(
                protocol_fee_recipient_token_account_info,
                &protocol_fee_recipient,
            )?;
            Some(protocol_fee_recipient_token_account_info)
        } else {
            None
        };
        if let Some(fee_recipient_token_account_info) = fee_recipient_token_account_info {
            ctx.accounts.transfer_asset_from_pending_vault(
                fee_recipient_token_account_info.to_account_info(),
                fee_recipient_fee,
                seeds,
            )?;
        }
        if let Some(protocol_fee_recipient_token_account_info) =
            protocol_fee_recipient_token_account_info
        {
            ctx.accounts.transfer_asset_from_pending_vault(
                protocol_fee_recipient_token_account_info.to_account_info(),
                protocol_fee,
                seeds,
            )?;
        }
        ctx.accounts.settle_deposit(seeds, net_deposit)?;
        (shares, net_deposit)
    } else {
        // Assets to be transferred, floored (protocol favorable)
        let assets = calculate_assets(nav, decimals, original_amount)?;
        maybe_enforce_junior_ratio_floor(
            &ctx.accounts.vault,
            request_share_mint_key,
            ctx.accounts.request.request_type,
            original_amount,
            share_mint_context.tranche_info,
            &mut remaining,
        )?;
        update_tranche_supply_cache(
            request_share_mint_key,
            ctx.accounts.request.request_type,
            original_amount,
            ctx.accounts.share_mint.supply,
            share_mint_context.tranche_info,
            tranche_config_info,
        )?;

        // Check for WithdrawFee Extension and calculate fee owed
        let (withdraw_fee, net_assets) =
            get_withdrawal_fee_and_net(&ctx.accounts.vault.to_account_info(), assets)?;
        ctx.accounts
            .vault
            .consume_redemption_rolling_limit(assets, current_slot)?;
        let (protocol_fee, fee_recipient_fee) =
            split_protocol_fee(withdraw_fee, ctx.accounts.vault.protocol_fee_bps)?;
        let fee_recipient_token_account_info = if fee_recipient_fee > 0 {
            let fee_recipient_token_account_info = remaining
                .next()
                .ok_or(AsyncVaultError::MissingFeeRecipient)?;
            validate_token_account_owner(
                fee_recipient_token_account_info,
                &ctx.accounts.vault.fee_recipient,
            )?;
            Some(fee_recipient_token_account_info)
        } else {
            None
        };
        let protocol_fee_recipient_token_account_info = if protocol_fee > 0 {
            let (protocol_fee_recipient, consumed_protocol_fee_config) =
                resolve_protocol_fee_recipient(&ctx.accounts.vault, remaining.peek().copied())?;
            if consumed_protocol_fee_config {
                remaining.next();
            }
            let protocol_fee_recipient_token_account_info = remaining
                .next()
                .ok_or(AsyncVaultError::MissingFeeRecipient)?;
            validate_token_account_owner(
                protocol_fee_recipient_token_account_info,
                &protocol_fee_recipient,
            )?;
            Some(protocol_fee_recipient_token_account_info)
        } else {
            None
        };
        if let Some(fee_recipient_token_account_info) = fee_recipient_token_account_info {
            ctx.accounts.transfer_asset_from_vault(
                fee_recipient_token_account_info.to_account_info(),
                fee_recipient_fee,
                seeds,
            )?;
        }
        if let Some(protocol_fee_recipient_token_account_info) =
            protocol_fee_recipient_token_account_info
        {
            ctx.accounts.transfer_asset_from_vault(
                protocol_fee_recipient_token_account_info.to_account_info(),
                protocol_fee,
                seeds,
            )?;
        }
        ctx.accounts.settle_redeem(seeds, net_assets)?;
        (net_assets, assets)
    };

    let vault = &mut ctx.accounts.vault;
    let request = &mut ctx.accounts.request;

    // Update primary vault aggregate accounting or the secondary asset ledger.
    if is_deposit {
        if is_primary_asset {
            if vault.deposit_cap > 0 {
                let capped_total = vault
                    .total_asset_balance
                    .checked_add(balance_delta)
                    .ok_or(AsyncVaultError::ArithmeticError)?;
                require!(
                    capped_total <= vault.deposit_cap,
                    AsyncVaultError::DepositCapExceeded
                );
            }
            vault.total_asset_balance = vault
                .total_asset_balance
                .checked_add(balance_delta)
                .ok_or(AsyncVaultError::ArithmeticError)?;
            vault.pending_deposit_amount = vault
                .pending_deposit_amount
                .checked_sub(original_amount)
                .ok_or(AsyncVaultError::ArithmeticError)?;
        } else {
            let vault_asset = ctx
                .accounts
                .vault_asset
                .as_mut()
                .ok_or(AsyncVaultError::InvalidAssetMint)?;
            vault_asset.idle_balance = vault_asset
                .idle_balance
                .checked_add(balance_delta)
                .ok_or(AsyncVaultError::ArithmeticError)?;
            vault_asset.pending_deposit_amount = vault_asset
                .pending_deposit_amount
                .checked_sub(original_amount)
                .ok_or(AsyncVaultError::ArithmeticError)?;
        }
    } else if is_primary_asset {
        vault.total_asset_balance = vault
            .total_asset_balance
            .checked_sub(balance_delta)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_mut()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.idle_balance = vault_asset
            .idle_balance
            .checked_sub(balance_delta)
            .ok_or(AsyncVaultError::ArithmeticError)?;
    }

    // Update Request's amount with the claimable amount
    request.amount = claimable_amount;
    request.price = nav;
    request.request_state = RequestState::Claimable;

    // Decrement Vault's pending Requests
    vault.pending_async_requests = vault
        .pending_async_requests
        .checked_sub(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
