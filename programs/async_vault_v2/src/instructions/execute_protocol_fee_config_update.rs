use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::ProtocolFeeConfigUpdated,
    instructions::queue_protocol_fee_config_update::{
        validate_protocol_fee_config_update, PendingProtocolFeeConfigUpdate,
    },
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[derive(Accounts)]
pub struct ExecuteProtocolFeeConfigUpdate<'info> {
    #[account(mut)]
    pub executor: Signer<'info>,

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
        close = executor,
        constraint = pending_update.protocol_fee_config == protocol_fee_config.key()
            @ AsyncVaultError::InvalidTimelockChange,
        constraint = pending_update.protocol_fee_governance == protocol_fee_governance.key()
            @ AsyncVaultError::InvalidTimelockChange,
    )]
    pub pending_update: Account<'info, PendingProtocolFeeConfigUpdate>,
}

pub fn handler(ctx: Context<ExecuteProtocolFeeConfigUpdate>) -> Result<()> {
    let pending = &ctx.accounts.pending_update;
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
    validate_protocol_fee_config_update(&pending.args)?;

    ctx.accounts.protocol_fee_config.protocol_fee_recipient = pending.args.protocol_fee_recipient;
    if let Some(delay) = pending.args.timelock_delay_slots {
        ctx.accounts.protocol_fee_governance.timelock_delay_slots = delay;
    }
    ctx.accounts.protocol_fee_governance.paused = false;
    ctx.accounts.protocol_fee_governance.version = ctx
        .accounts
        .protocol_fee_governance
        .version
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    emit!(ProtocolFeeConfigUpdated {
        protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
        protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
        authority: ctx.accounts.protocol_fee_config.authority,
        protocol_fee_recipient: ctx.accounts.protocol_fee_config.protocol_fee_recipient,
        timelock_delay_slots: ctx.accounts.protocol_fee_governance.timelock_delay_slots,
        version: ctx.accounts.protocol_fee_governance.version,
    });
    Ok(())
}
