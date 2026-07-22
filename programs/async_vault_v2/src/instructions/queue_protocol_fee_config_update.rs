use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::ProtocolFeeConfigUpdateQueued,
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace)]
pub struct ProtocolFeeConfigUpdateArgs {
    pub protocol_fee_recipient: Pubkey,
    pub timelock_delay_slots: Option<u64>,
}

#[account]
#[derive(InitSpace)]
pub struct PendingProtocolFeeConfigUpdate {
    pub protocol_fee_config: Pubkey,
    pub protocol_fee_governance: Pubkey,
    pub queued_by: Pubkey,
    pub created_slot: u64,
    pub eta_slot: u64,
    pub expected_version: u64,
    pub args: ProtocolFeeConfigUpdateArgs,
}

#[derive(Accounts)]
pub struct QueueProtocolFeeConfigUpdate<'info> {
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
        space = 8 + PendingProtocolFeeConfigUpdate::INIT_SPACE,
    )]
    pub pending_update: Account<'info, PendingProtocolFeeConfigUpdate>,

    pub system_program: Program<'info, System>,
}

pub fn validate_protocol_fee_config_update(args: &ProtocolFeeConfigUpdateArgs) -> Result<()> {
    require!(
        args.protocol_fee_recipient != Pubkey::default(),
        AsyncVaultError::InvalidFeeRecipient
    );
    if let Some(delay) = args.timelock_delay_slots {
        require!(delay > 0, AsyncVaultError::InvalidProtocolFeeTimelock);
    }
    Ok(())
}

pub fn handler(
    ctx: Context<QueueProtocolFeeConfigUpdate>,
    args: ProtocolFeeConfigUpdateArgs,
) -> Result<()> {
    validate_protocol_fee_config_update(&args)?;
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
        .pending_update
        .set_inner(PendingProtocolFeeConfigUpdate {
            protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
            protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
            queued_by: ctx.accounts.authority.key(),
            created_slot,
            eta_slot,
            expected_version,
            args,
        });

    emit!(ProtocolFeeConfigUpdateQueued {
        protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
        protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
        pending_update: ctx.accounts.pending_update.key(),
        queued_by: ctx.accounts.authority.key(),
        expected_version,
        eta_slot,
        protocol_fee_recipient: args.protocol_fee_recipient,
        timelock_delay_slots: args.timelock_delay_slots,
    });
    Ok(())
}
