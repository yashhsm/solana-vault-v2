use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    extensions::{
        init_vault_extension,
        instant_settlement::{
            assert_instant_settlement_safety_guards, validate_instant_settlement_thresholds,
            InstantSettlement,
        },
        VaultExtension,
    },
    state::Vault,
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct InitializeInstantSettlementArgs {
    pub instant_redemption_fee_bps: u16,
    pub min_deposit_amount: u64,
    pub max_deposit_amount: u64,
    pub min_redeem_shares: u64,
    pub max_redeem_shares: u64,
    pub max_user_deposit_amount: u64,
    pub max_user_redeem_shares: u64,
}

#[derive(Accounts)]
pub struct InitializeInstantSettlement<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(
        mut,
        realloc = vault.to_account_info().data_len() + InstantSettlement::TLV_SIZE,
        realloc::payer = payer,
        realloc::zero = false,
        constraint = authority.key() == vault.curator @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub vault: Account<'info, Vault>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeInstantSettlement>,
    args: InitializeInstantSettlementArgs,
) -> Result<()> {
    require!(
        args.instant_redemption_fee_bps <= vault_common::MAX_BPS,
        AsyncVaultError::FeeBpsExceeded
    );
    assert_instant_settlement_safety_guards(
        args.instant_redemption_fee_bps,
        ctx.accounts.vault.max_nav_staleness_slots,
    )?;
    validate_instant_settlement_thresholds(
        args.min_deposit_amount,
        args.max_deposit_amount,
        args.min_redeem_shares,
        args.max_redeem_shares,
    )?;
    require!(
        ctx.accounts.vault.tranche_config.is_none(),
        AsyncVaultError::UnsupportedPhaseConfig
    );
    init_vault_extension(
        &ctx.accounts.vault.to_account_info(),
        &ctx.accounts.vault,
        &InstantSettlement {
            min_deposit_amount: args.min_deposit_amount,
            max_deposit_amount: args.max_deposit_amount,
            min_redeem_shares: args.min_redeem_shares,
            max_redeem_shares: args.max_redeem_shares,
            max_user_deposit_amount: args.max_user_deposit_amount,
            max_user_redeem_shares: args.max_user_redeem_shares,
            enabled: 1,
            _reserved: [0; 7],
        },
    )?;
    ctx.accounts.vault.instant_redemption_fee_bps = args.instant_redemption_fee_bps;
    Ok(())
}
