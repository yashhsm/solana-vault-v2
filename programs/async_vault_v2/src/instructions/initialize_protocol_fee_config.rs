use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{ProtocolFeeConfig, PROTOCOL_FEE_CONFIG_SEED},
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct InitializeProtocolFeeConfigArgs {
    pub protocol_fee_recipient: Pubkey,
}

#[derive(Accounts)]
pub struct InitializeProtocolFeeConfig<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(
        init,
        payer = payer,
        space = 8 + ProtocolFeeConfig::INIT_SPACE,
        seeds = [PROTOCOL_FEE_CONFIG_SEED],
        bump,
    )]
    pub protocol_fee_config: Account<'info, ProtocolFeeConfig>,

    pub system_program: Program<'info, System>,
}

pub fn handler(
    ctx: Context<InitializeProtocolFeeConfig>,
    args: InitializeProtocolFeeConfigArgs,
) -> Result<()> {
    require!(
        args.protocol_fee_recipient != Pubkey::default(),
        AsyncVaultError::InvalidFeeRecipient
    );

    ctx.accounts
        .protocol_fee_config
        .set_inner(ProtocolFeeConfig {
            authority: ctx.accounts.authority.key(),
            protocol_fee_recipient: args.protocol_fee_recipient,
            bump: ctx.bumps.protocol_fee_config,
        });

    Ok(())
}
