use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::{
    error::AsyncVaultError,
    instructions::update_vault::{validate_queued_vault_update, UpdateVaultArgs},
    state::{Vault, VAULT_CONFIG_SEED},
};

#[account]
#[derive(InitSpace)]
pub struct PendingVaultUpdate {
    pub vault: Pubkey,
    pub queued_by: Pubkey,
    pub created_slot: u64,
    pub eta_slot: u64,
    pub args: UpdateVaultArgs,
}

#[derive(Accounts)]
pub struct QueueVaultUpdate<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,

    #[account(
        init,
        payer = payer,
        space = 8 + PendingVaultUpdate::INIT_SPACE,
    )]
    pub pending_update: Account<'info, PendingVaultUpdate>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<QueueVaultUpdate>, args: UpdateVaultArgs) -> Result<()> {
    let vault = &ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots > 0,
        AsyncVaultError::TimelockNotConfigured
    );
    validate_queued_vault_update(vault, &args)?;

    let current_slot = Clock::get()?.slot;
    let eta_slot = current_slot
        .checked_add(vault.timelock_delay_slots)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    let pending_update = &mut ctx.accounts.pending_update;
    pending_update.vault = vault.key();
    pending_update.queued_by = ctx.accounts.authority.key();
    pending_update.created_slot = current_slot;
    pending_update.eta_slot = eta_slot;
    pending_update.args = args;

    Ok(())
}
