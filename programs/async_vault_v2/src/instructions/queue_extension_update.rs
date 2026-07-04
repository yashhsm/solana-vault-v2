use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    extensions::{
        has_extension, min_redemption::MinRedemption, min_subscription::MinSubscription,
        pausable_redemptions::PausableRedemption, pausable_subscriptions::PausableSubscription,
        update_vault_extension, ExtensionType, TLV_START,
    },
    state::Vault,
};

#[derive(AnchorDeserialize, AnchorSerialize, Clone, Copy, InitSpace, PartialEq, Eq)]
pub enum ExtensionUpdateKind {
    MinSubscription,
    MinRedemption,
    PausableSubscriptions,
    PausableRedemptions,
}

impl ExtensionUpdateKind {
    pub fn extension_type(self) -> ExtensionType {
        match self {
            Self::MinSubscription => ExtensionType::MinSubscription,
            Self::MinRedemption => ExtensionType::MinRedemption,
            Self::PausableSubscriptions => ExtensionType::PausableSubscriptions,
            Self::PausableRedemptions => ExtensionType::PausableRedemptions,
        }
    }
}

#[derive(AnchorDeserialize, AnchorSerialize, Clone, Copy, InitSpace, PartialEq, Eq)]
pub struct ExtensionUpdateArgs {
    pub kind: ExtensionUpdateKind,
    pub threshold: u64,
    pub paused: bool,
}

#[account]
#[derive(InitSpace)]
pub struct PendingExtensionUpdate {
    pub vault: Pubkey,
    pub queued_by: Pubkey,
    pub created_slot: u64,
    pub eta_slot: u64,
    pub args: ExtensionUpdateArgs,
}

#[derive(Accounts)]
pub struct QueueExtensionUpdate<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
        init,
        payer = payer,
        space = 8 + PendingExtensionUpdate::INIT_SPACE,
    )]
    pub pending_extension_update: Account<'info, PendingExtensionUpdate>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<QueueExtensionUpdate>, args: ExtensionUpdateArgs) -> Result<()> {
    let vault = &ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots > 0,
        AsyncVaultError::TimelockNotConfigured
    );
    validate_extension_update(args)?;
    require_extension_exists(
        &ctx.accounts.vault.to_account_info(),
        args.kind.extension_type(),
    )?;

    let current_slot = Clock::get()?.slot;
    let eta_slot = current_slot
        .checked_add(vault.timelock_delay_slots)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    let pending_extension_update = &mut ctx.accounts.pending_extension_update;
    pending_extension_update.vault = vault.key();
    pending_extension_update.queued_by = ctx.accounts.authority.key();
    pending_extension_update.created_slot = current_slot;
    pending_extension_update.eta_slot = eta_slot;
    pending_extension_update.args = args;

    Ok(())
}

pub fn validate_extension_update(args: ExtensionUpdateArgs) -> Result<()> {
    match args.kind {
        ExtensionUpdateKind::MinSubscription | ExtensionUpdateKind::MinRedemption => {
            require!(!args.paused, AsyncVaultError::InvalidExtensionData);
        }
        ExtensionUpdateKind::PausableSubscriptions | ExtensionUpdateKind::PausableRedemptions => {
            require!(args.threshold == 0, AsyncVaultError::InvalidExtensionData);
        }
    }
    Ok(())
}

pub fn apply_extension_update(vault_info: &AccountInfo, args: ExtensionUpdateArgs) -> Result<()> {
    validate_extension_update(args)?;
    match args.kind {
        ExtensionUpdateKind::MinSubscription => update_vault_extension(
            vault_info,
            &MinSubscription {
                threshold: args.threshold,
            },
        ),
        ExtensionUpdateKind::MinRedemption => update_vault_extension(
            vault_info,
            &MinRedemption {
                threshold: args.threshold,
            },
        ),
        ExtensionUpdateKind::PausableSubscriptions => update_vault_extension(
            vault_info,
            &PausableSubscription {
                paused: args.paused as u8,
            },
        ),
        ExtensionUpdateKind::PausableRedemptions => update_vault_extension(
            vault_info,
            &PausableRedemption {
                paused: args.paused as u8,
            },
        ),
    }
}

fn require_extension_exists(vault_info: &AccountInfo, extension_type: ExtensionType) -> Result<()> {
    let data = vault_info
        .data
        .try_borrow()
        .map_err(|_| ProgramError::AccountBorrowFailed)?;
    require!(
        data.len() > TLV_START,
        AsyncVaultError::UninitializedExtension
    );
    require!(
        has_extension(&data[TLV_START..], extension_type),
        AsyncVaultError::UninitializedExtension
    );
    Ok(())
}
