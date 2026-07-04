use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    error::AsyncVaultError,
    extensions::{
        fee::processor::get_deposit_fee_and_net,
        instant_settlement::assert_instant_settlement_enabled,
    },
    state::{Vault, INSTANT_USER_LIMIT_SEED, VAULT_CONFIG_SEED},
    utils::{
        calculate_shares, load_or_init_instant_settlement_user, split_protocol_fee,
        validate_asset_mint_extensions_from_acct_info,
    },
};

#[derive(Accounts)]
pub struct InstantDeposit<'info> {
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
        token::mint = asset_mint,
        token::authority = user,
        token::token_program = asset_token_program,
    )]
    pub user_asset_account: Box<InterfaceAccount<'info, TokenAccount>>,

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
        token::authority = vault.fee_recipient,
        token::token_program = asset_token_program,
    )]
    pub fee_recipient_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault.protocol_fee_recipient,
        token::token_program = asset_token_program,
    )]
    pub protocol_fee_recipient_token_account: Option<Box<InterfaceAccount<'info, TokenAccount>>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> InstantDeposit<'info> {
    fn transfer_user_assets_to(&self, to: AccountInfo<'info>, amount: u64) -> Result<()> {
        token_interface::transfer_checked(
            CpiContext::new(
                self.asset_token_program.key(),
                TransferChecked {
                    from: self.user_asset_account.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to,
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
            self.asset_mint.decimals,
        )
    }

    fn mint_shares_to_user(&self, shares: u64, seeds: &[&[&[u8]]]) -> Result<()> {
        token_interface::mint_to(
            CpiContext::new_with_signer(
                self.share_token_program.key(),
                MintTo {
                    mint: self.share_mint.to_account_info(),
                    to: self.user_share_account.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                seeds,
            ),
            shares,
        )
    }
}

pub fn handler<'info>(ctx: Context<'info, InstantDeposit<'info>>, amount: u64) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    require!(
        ctx.accounts.vault.tranche_config.is_none(),
        AsyncVaultError::UnsupportedPhaseConfig
    );
    let instant_settlement =
        assert_instant_settlement_enabled(&ctx.accounts.vault.to_account_info())?;
    instant_settlement.assert_deposit_amount(amount)?;
    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;
    require!(ctx.accounts.vault.nav > 0, AsyncVaultError::NavIsNotSet);
    let current_slot = Clock::get()?.slot;
    ctx.accounts.vault.assert_nav_not_stale(current_slot)?;
    if instant_settlement.max_user_deposit_amount != 0 {
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
        instant_user.consume_deposit_limit(
            amount,
            current_slot,
            instant_settlement.max_user_deposit_amount,
            ctx.accounts.vault.rolling_limit_window_slots,
        )?;
        instant_user.exit(ctx.program_id)?;
    }
    ctx.accounts.vault.assert_deposit_cap_allows(amount)?;

    let (deposit_fee, net_deposit) =
        get_deposit_fee_and_net(&ctx.accounts.vault.to_account_info(), amount)?;
    require!(net_deposit > 0, AsyncVaultError::InsufficientDepositAmount);
    let shares = calculate_shares(
        ctx.accounts.vault.nav,
        ctx.accounts.share_mint.decimals,
        net_deposit,
    )?;
    require!(shares > 0, AsyncVaultError::InsufficientDepositAmount);

    let (protocol_fee, fee_recipient_fee) =
        split_protocol_fee(deposit_fee, ctx.accounts.vault.protocol_fee_bps)?;
    if fee_recipient_fee > 0 {
        let fee_recipient_token_account = ctx
            .accounts
            .fee_recipient_token_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingFeeRecipient)?;
        ctx.accounts.transfer_user_assets_to(
            fee_recipient_token_account.to_account_info(),
            fee_recipient_fee,
        )?;
    }
    if protocol_fee > 0 {
        let protocol_fee_recipient_token_account = ctx
            .accounts
            .protocol_fee_recipient_token_account
            .as_ref()
            .ok_or(AsyncVaultError::MissingFeeRecipient)?;
        ctx.accounts.transfer_user_assets_to(
            protocol_fee_recipient_token_account.to_account_info(),
            protocol_fee,
        )?;
    }
    ctx.accounts.transfer_user_assets_to(
        ctx.accounts.vault_token_account.to_account_info(),
        net_deposit,
    )?;

    let share_mint_key = ctx.accounts.share_mint.key();
    let vault_bump = ctx.accounts.vault.bump;
    let seeds: &[&[&[u8]]] = &[&[VAULT_CONFIG_SEED, share_mint_key.as_ref(), &[vault_bump]]];
    ctx.accounts.mint_shares_to_user(shares, seeds)?;

    ctx.accounts.vault.total_asset_balance = ctx
        .accounts
        .vault
        .total_asset_balance
        .checked_add(net_deposit)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
