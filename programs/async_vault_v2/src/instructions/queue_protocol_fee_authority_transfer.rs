use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::ProtocolFeeAuthorityTransferQueued,
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[account]
#[derive(InitSpace)]
pub struct PendingProtocolFeeAuthorityTransfer {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub queued_by: Pubkey,
    pub new_authority: Pubkey,
    pub created_slot: u64,
    pub eta_slot: u64,
    pub expected_version: u64,
}

#[derive(Accounts)]
pub struct QueueProtocolFeeAuthorityTransfer<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(
        seeds = [PROTOCOL_FEE_CONFIG_SEED],
        bump = protocol_fee_config.bump,
        has_one = authority @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub protocol_fee_config: Account<'info, ProtocolFeeConfig>,

    #[account(
        seeds = [PROTOCOL_FEE_GOVERNANCE_SEED],
        bump = protocol_fee_governance.bump,
        constraint = protocol_fee_governance.protocol_fee_config == protocol_fee_config.key()
            @ AsyncVaultError::InvalidProtocolFeeGovernance,
    )]
    pub protocol_fee_governance: Account<'info, ProtocolFeeGovernance>,

    #[account(
        init,
        payer = payer,
        space = 8 + PendingProtocolFeeAuthorityTransfer::INIT_SPACE,
    )]
    pub pending_transfer: Account<'info, PendingProtocolFeeAuthorityTransfer>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<QueueProtocolFeeAuthorityTransfer>,
    new_authority: Pubkey,
) -> Result<()> {
    require!(
        new_authority != Pubkey::default()
            && new_authority != ctx.accounts.protocol_fee_config.authority,
        AsyncVaultError::UnauthorizedSigner
    );
    require!(
        ctx.accounts.protocol_fee_governance.timelock_delay_slots > 0,
        AsyncVaultError::InvalidProtocolFeeTimelock
    );

    let created_slot = Clock::get()?.slot;
    let eta_slot = created_slot
        .checked_add(ctx.accounts.protocol_fee_governance.timelock_delay_slots)
        .ok_or(AsyncVaultError::ArithmeticError)?;
    let expected_version = ctx.accounts.protocol_fee_governance.version;
    ctx.accounts
        .pending_transfer
        .set_inner(PendingProtocolFeeAuthorityTransfer {
            protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
            protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
            queued_by: ctx.accounts.authority.key(),
            new_authority,
            created_slot,
            eta_slot,
            expected_version,
        });

    emit!(ProtocolFeeAuthorityTransferQueued {
        protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
        protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
        pending_transfer: ctx.accounts.pending_transfer.key(),
        current_authority: ctx.accounts.authority.key(),
        new_authority,
        expected_version,
        eta_slot,
    });
    Ok(())
}
