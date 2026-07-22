use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::ProtocolFeeGovernanceInitialized,
    state::{
        ProtocolFeeConfig, ProtocolFeeGovernance, PROTOCOL_FEE_CONFIG_SEED,
        PROTOCOL_FEE_GOVERNANCE_SEED,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct InitializeProtocolFeeConfigV2Args {
    pub authority: Pubkey,
    pub breaker: Pubkey,
    pub timelock_delay_slots: u64,
}

#[derive(Accounts)]
pub struct InitializeProtocolFeeConfigV2<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub upgrade_authority: Signer<'info>,

    #[account(
        constraint = program.programdata_address()? == Some(program_data.key())
            @ AsyncVaultError::InvalidProtocolFeeGovernance,
    )]
    pub program: Program<'info, crate::program::AsyncVaultV2>,

    #[account(
        constraint = program_data.upgrade_authority_address == Some(upgrade_authority.key())
            @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub program_data: Account<'info, ProgramData>,

    #[account(
        init_if_needed,
        payer = payer,
        space = 8 + ProtocolFeeConfig::INIT_SPACE,
        seeds = [PROTOCOL_FEE_CONFIG_SEED],
        bump,
    )]
    pub protocol_fee_config: Account<'info, ProtocolFeeConfig>,

    #[account(
        init,
        payer = payer,
        space = 8 + ProtocolFeeGovernance::INIT_SPACE,
        seeds = [PROTOCOL_FEE_GOVERNANCE_SEED],
        bump,
    )]
    pub protocol_fee_governance: Account<'info, ProtocolFeeGovernance>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeProtocolFeeConfigV2>,
    args: InitializeProtocolFeeConfigV2Args,
) -> Result<()> {
    require!(
        args.authority != Pubkey::default() && args.breaker != Pubkey::default(),
        AsyncVaultError::UnauthorizedSigner
    );
    require!(
        args.timelock_delay_slots > 0,
        AsyncVaultError::InvalidProtocolFeeTimelock
    );

    ctx.accounts
        .protocol_fee_config
        .set_inner(ProtocolFeeConfig {
            authority: args.authority,
            // The override starts fail-closed. A timelocked update activates it.
            protocol_fee_recipient: Pubkey::default(),
            bump: ctx.bumps.protocol_fee_config,
        });
    ctx.accounts
        .protocol_fee_governance
        .set_inner(ProtocolFeeGovernance {
            protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
            breaker: args.breaker,
            timelock_delay_slots: args.timelock_delay_slots,
            paused: true,
            version: 0,
            bump: ctx.bumps.protocol_fee_governance,
        });

    emit!(ProtocolFeeGovernanceInitialized {
        protocol_fee_config: ctx.accounts.protocol_fee_config.key(),
        protocol_fee_governance: ctx.accounts.protocol_fee_governance.key(),
        authority: args.authority,
        breaker: args.breaker,
        timelock_delay_slots: args.timelock_delay_slots,
    });
    Ok(())
}
