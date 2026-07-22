use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::ProtocolFeeConfigPaused,
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[derive(Accounts)]
pub struct PauseProtocolFeeConfig<'info> {
    pub authority: Signer<'info>,

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
}

pub fn handler(ctx: Context<PauseProtocolFeeConfig>) -> Result<()> {
    let authority = ctx.accounts.authority.key();
    require!(
        authority == ctx.accounts.protocol_fee_config.authority
            || authority == ctx.accounts.protocol_fee_governance.breaker,
        AsyncVaultError::UnauthorizedSigner
    );

    ctx.accounts.protocol_fee_config.protocol_fee_recipient = Pubkey::default();
    ctx.accounts.protocol_fee_governance.paused = true;
    ctx.accounts.protocol_fee_governance.version = ctx
        .accounts
        .protocol_fee_governance
        .version
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    emit!(ProtocolFeeConfigPaused {
        protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
        protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
        authority,
        version: ctx.accounts.protocol_fee_governance.version,
    });
    Ok(())
}
