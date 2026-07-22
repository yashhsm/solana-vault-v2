use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::ProtocolFeeAuthorityTransferred,
    instructions::queue_protocol_fee_authority_transfer::PendingProtocolFeeAuthorityTransfer,
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[derive(Accounts)]
pub struct AcceptProtocolFeeAuthorityTransfer<'info> {
    #[account(mut)]
    pub new_authority: Signer<'info>,

    #[account(
        mut,
        seeds = [PROTOCOL_FEE_CONFIG_SEED],
        bump = protocol_fee_config.bump,
    )]
    pub protocol_fee_config: Account<'info, ProtocolFeeConfig>,

    #[account(
        mut,
        seeds = [PROTOCOL_FEE_GOVERNANCE_SEED],
        bump = protocol_fee_governance.bump,
        constraint = protocol_fee_governance.protocol_fee_config == protocol_fee_config.key()
            @ AsyncVaultError::InvalidProtocolFeeGovernance,
    )]
    pub protocol_fee_governance: Account<'info, ProtocolFeeGovernance>,

    #[account(
        mut,
        close = new_authority,
        constraint = pending_transfer.protocol_fee_config == protocol_fee_config.key()
            @ AsyncVaultError::InvalidTimelockChange,
        constraint = pending_transfer.protocol_fee_governance == protocol_fee_governance.key()
            @ AsyncVaultError::InvalidTimelockChange,
        constraint = pending_transfer.new_authority == new_authority.key()
            @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub pending_transfer: Account<'info, PendingProtocolFeeAuthorityTransfer>,
}

pub fn handler(ctx: Context<AcceptProtocolFeeAuthorityTransfer>) -> Result<()> {
    let pending = &ctx.accounts.pending_transfer;
    require!(
        Clock::get()?.slot >= pending.eta_slot,
        AsyncVaultError::TimelockNotReady
    );
    require_keys_eq!(
        pending.queued_by,
        ctx.accounts.protocol_fee_config.authority,
        AsyncVaultError::StaleTimelockAuthority
    );
    require!(
        pending.expected_version == ctx.accounts.protocol_fee_governance.version,
        AsyncVaultError::StaleProtocolFeeGovernanceVersion
    );

    let previous_authority = ctx.accounts.protocol_fee_config.authority;
    ctx.accounts.protocol_fee_config.authority = ctx.accounts.new_authority.key();
    ctx.accounts.protocol_fee_governance.version = ctx
        .accounts
        .protocol_fee_governance
        .version
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    emit!(ProtocolFeeAuthorityTransferred {
        protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
        protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
        previous_authority,
        new_authority: ctx.accounts.new_authority.key(),
        version: ctx.accounts.protocol_fee_governance.version,
    });
    Ok(())
}
