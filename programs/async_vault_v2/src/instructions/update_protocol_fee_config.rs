use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    state::{ProtocolFeeConfig, PROTOCOL_FEE_CONFIG_SEED},
};

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct UpdateProtocolFeeConfigArgs {
    pub protocol_fee_recipient: Pubkey,
}

#[derive(Accounts)]
pub struct UpdateProtocolFeeConfig<'info> {
    pub authority: Signer<'info>,

    #[account(
        mut,
        seeds = [PROTOCOL_FEE_CONFIG_SEED],
        bump = protocol_fee_config.bump,
        has_one = authority @ AsyncVaultError::UnauthorizedSigner,
    )]
    pub protocol_fee_config: Account<'info, ProtocolFeeConfig>,
}

pub fn handler(
    ctx: Context<UpdateProtocolFeeConfig>,
    args: UpdateProtocolFeeConfigArgs,
) -> Result<()> {
    require!(
        args.protocol_fee_recipient != Pubkey::default(),
        AsyncVaultError::InvalidFeeRecipient
    );

    ctx.accounts.protocol_fee_config.protocol_fee_recipient = args.protocol_fee_recipient;
    Ok(())
}
