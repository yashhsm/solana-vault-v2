use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    instructions::queue_protocol_fee_authority_transfer::PendingProtocolFeeAuthorityTransfer,
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[derive(Accounts)]
pub struct CancelProtocolFeeAuthorityTransfer<'info> {
    #[account(mut)]
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
        mut,
        close = authority,
        constraint = pending_transfer.protocol_fee_config == protocol_fee_config.key()
            @ AsyncVaultError::InvalidTimelockChange,
        constraint = pending_transfer.protocol_fee_governance == protocol_fee_governance.key()
            @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_transfer: Account<'info, PendingProtocolFeeAuthorityTransfer>,
}

pub fn handler(_ctx: Context<CancelProtocolFeeAuthorityTransfer>) -> Result<()> {
    Ok(())
}
