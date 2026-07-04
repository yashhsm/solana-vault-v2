use anchor_lang::prelude::*;
use vault_common::FeeType;

use crate::{
    error::AsyncVaultError,
    extensions::{
        fee::{DepositFee, WithdrawalFee},
        update_vault_extension,
    },
    state::Vault,
};

#[derive(AnchorDeserialize, AnchorSerialize, Clone, Copy, InitSpace)]
pub enum FeeUpdateKind {
    Deposit,
    Withdrawal,
}

#[derive(AnchorDeserialize, AnchorSerialize, Clone, Copy, InitSpace)]
pub struct FeeUpdateArgs {
    pub kind: FeeUpdateKind,
    pub fee: FeeType,
}

#[account]
#[derive(InitSpace)]
pub struct PendingFeeUpdate {
    pub vault: Pubkey,
    pub queued_by: Pubkey,
    pub created_slot: u64,
    pub eta_slot: u64,
    pub args: FeeUpdateArgs,
}

#[derive(Accounts)]
pub struct QueueFeeUpdate<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
        init,
        payer = payer,
        space = 8 + PendingFeeUpdate::INIT_SPACE,
    )]
    pub pending_fee_update: Account<'info, PendingFeeUpdate>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<QueueFeeUpdate>, args: FeeUpdateArgs) -> Result<()> {
    let vault = &ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots > 0,
        AsyncVaultError::TimelockNotConfigured
    );
    validate_fee_update(args)?;

    let current_slot = Clock::get()?.slot;
    let eta_slot = current_slot
        .checked_add(vault.timelock_delay_slots)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    let pending_fee_update = &mut ctx.accounts.pending_fee_update;
    pending_fee_update.vault = vault.key();
    pending_fee_update.queued_by = ctx.accounts.authority.key();
    pending_fee_update.created_slot = current_slot;
    pending_fee_update.eta_slot = eta_slot;
    pending_fee_update.args = args;

    Ok(())
}

pub fn validate_fee_update(args: FeeUpdateArgs) -> Result<()> {
    args.fee.validate().map_err(AsyncVaultError::from)?;
    Ok(())
}

pub fn apply_fee_update(vault_info: &AccountInfo, args: FeeUpdateArgs) -> Result<()> {
    validate_fee_update(args)?;
    match args.kind {
        FeeUpdateKind::Deposit => {
            update_vault_extension(vault_info, &DepositFee::from_fee_type(args.fee))
        }
        FeeUpdateKind::Withdrawal => {
            update_vault_extension(vault_info, &WithdrawalFee::from_fee_type(args.fee))
        }
    }
}
