use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
};
use vault_common::FeeType;

use crate::{
    error::AsyncVaultError,
    extensions::{
        fee::processor::get_withdrawal_fee,
        instant_settlement::{
            assert_instant_settlement_enabled, assert_instant_settlement_safety_guards,
        },
    },
    state::{Vault, INSTANT_USER_LIMIT_SEED, VAULT_CONFIG_SEED},
    utils::{
        calculate_assets, load_or_init_instant_settlement_user, resolve_protocol_fee_recipient,
        split_protocol_fee, validate_asset_mint_extensions_from_acct_info,
        validate_token_account_owner,
    },
};

#[derive(Accounts)]
pub struct InstantRedeem<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        has_one = asset_mint @ AsyncVaultError::InvalidAssetMint,
        has_one = share_mint @ AsyncVaultError::InvalidShareMint,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        mut,
        seeds = [INSTANT_USER_LIMIT_SEED, vault.key().as_ref(), user.key().as_ref()],
        bump,
    )]
    pub instant_user: Option<UncheckedAccount<'info>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = vault.vault_token_account == vault_token_account.key() @ AsyncVaultError::InvalidVault,
    )]
    pub vault_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = share_mint,
        token::authority = user,
        token::token_program = share_token_program,
    )]
    pub user_share_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = user,
        token::token_program = asset_token_program,
    )]
    pub user_asset_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault.fee_recipient,
        token::token_program = asset_token_program,
    )]
    pub fee_recipient_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::token_program = asset_token_program,
    )]
    pub protocol_fee_recipient_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> InstantRedeem<'info> {
    fn burn_user_shares(&self, amount: u64) -> Result<()> {
        token_interface::burn(
            CpiContext::new(
                self.share_token_program.key(),
                Burn {
                    mint: self.share_mint.to_account_info(),
                    from: self.user_share_account.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
        )
    }

    fn transfer_vault_assets_to(
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
}

pub fn handler<'info>(ctx: Context<'info, InstantRedeem<'info>>, shares: u64) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    require!(
        ctx.accounts.vault.tranche_config.is_none(),
        AsyncVaultError::UnsupportedPhaseConfig
    );
    let instant_settlement =
        assert_instant_settlement_enabled(&ctx.accounts.vault.to_account_info())?;
    assert_instant_settlement_safety_guards(
        ctx.accounts.vault.instant_redemption_fee_bps,
        ctx.accounts.vault.max_nav_staleness_slots,
    )?;
    instant_settlement.assert_redeem_shares(shares)?;
    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;
    require!(ctx.accounts.vault.nav > 0, AsyncVaultError::NavIsNotSet);
    let current_slot = Clock::get()?.slot;
    ctx.accounts.vault.assert_nav_not_stale(current_slot)?;
    if instant_settlement.max_user_redeem_shares != 0 {
        let instant_user_bump = ctx
            .bumps
            .instant_user
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let instant_user_account = ctx
            .accounts
            .instant_user
            .as_ref()
            .ok_or(AsyncVaultError::MissingRequiredAccount)?;
        let instant_user_info = instant_user_account.as_ref();
        let mut instant_user = load_or_init_instant_settlement_user(
            instant_user_info,
            ctx.accounts.user.to_account_info(),
            ctx.accounts.system_program.to_account_info(),
            ctx.accounts.vault.key(),
            ctx.accounts.user.key(),
            instant_user_bump,
        )?;
        instant_user.consume_redeem_limit(
            shares,
            current_slot,
            instant_settlement.max_user_redeem_shares,
            ctx.accounts.vault.rolling_limit_window_slots,
        )?;
        instant_user.exit(ctx.program_id)?;
    }

    let gross_assets = calculate_assets(
        ctx.accounts.vault.nav,
        ctx.accounts.share_mint.decimals,
        shares,
    )?;
    require!(
        ctx.accounts.vault_token_account.amount >= gross_assets,
        AsyncVaultError::InsufficientLiquidity
    );

    let withdrawal_fee = get_withdrawal_fee(&ctx.accounts.vault.to_account_info(), gross_assets)?;
    let instant_fee = if ctx.accounts.vault.instant_redemption_fee_bps == 0 {
        0
    } else {
        FeeType::Percentage {
            bps: ctx.accounts.vault.instant_redemption_fee_bps,
        }
        .get_fee(gross_assets)
        .map_err(AsyncVaultError::from)?
    };
    let total_fee = withdrawal_fee
        .checked_add(instant_fee)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let net_assets = gross_assets
        .checked_sub(total_fee)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    require!(net_assets > 0, AsyncVaultError::InsufficientRedeemAmount);

    ctx.accounts
        .vault
        .consume_redemption_rolling_limit(gross_assets, current_slot)?;
    ctx.accounts.burn_user_shares(shares)?;

    let share_mint_key = ctx.accounts.share_mint.key();
    let vault_bump = ctx.accounts.vault.bump;
    let seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];
    let (protocol_fee, fee_recipient_fee) =
        split_protocol_fee(total_fee, ctx.accounts.vault.protocol_fee_bps)?;
    let mut remaining = ctx.remaining_accounts.iter().peekable();
    if fee_recipient_fee > 0 {
        let fee_recipient_token_account = ctx
            .accounts
            .fee_recipient_token_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingFeeRecipient)?;
        ctx.accounts.transfer_vault_assets_to(
            fee_recipient_token_account.to_account_info(),
            fee_recipient_fee,
            seeds,
        )?;
    }
    if protocol_fee > 0 {
        let (protocol_fee_recipient, consumed_protocol_fee_config) =
            resolve_protocol_fee_recipient(&ctx.accounts.vault, remaining.peek().copied())?;
        if consumed_protocol_fee_config {
            remaining.next();
        }
        let protocol_fee_recipient_token_account = ctx
            .accounts
            .protocol_fee_recipient_token_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingFeeRecipient)?;
        validate_token_account_owner(
            &protocol_fee_recipient_token_account.to_account_info(),
            &protocol_fee_recipient,
        )?;
        ctx.accounts.transfer_vault_assets_to(
            protocol_fee_recipient_token_account.to_account_info(),
            protocol_fee,
            seeds,
        )?;
    }
    ctx.accounts.transfer_vault_assets_to(
        ctx.accounts.user_asset_account.to_account_info(),
        net_assets,
        seeds,
    )?;

    ctx.accounts.vault.total_asset_balance = ctx
        .accounts
        .vault
        .total_asset_balance
        .checked_sub(gross_assets)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
