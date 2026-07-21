use anchor_lang::prelude::*;

use crate::{
    error::AsyncVaultError,
    events::StrategyPolicyInitialized,
    state::{StrategyPolicy, Vault, STRATEGY_POLICY_SEED},
};

#[derive(Accounts)]
#[instruction(strategist: Pubkey)]
pub struct InitializeStrategyPolicy<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    pub vault: Account<'info, Vault>,

    #[account(
        init,
        payer = payer,
        space = 8 + StrategyPolicy::INIT_SPACE,
        seeds = [STRATEGY_POLICY_SEED, vault.key().as_ref(), strategist.as_ref()],
        bump,
    )]
    pub strategy_policy: Account<'info, StrategyPolicy>,

    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<InitializeStrategyPolicy>, strategist: Pubkey) -> Result<()> {
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    require_keys_neq!(
        strategist,
        Pubkey::default(),
        AsyncVaultError::InvalidStrategyPolicy
    );

    ctx.accounts.strategy_policy.set_inner(StrategyPolicy {
        vault: ctx.accounts.vault.key(),
        strategist,
        merkle_root: [0_u8; 32],
        version: 0,
        paused: true,
        executing: false,
        bump: ctx.bumps.strategy_policy,
    });

    emit!(StrategyPolicyInitialized {
        vault: ctx.accounts.vault.key(),
        strategist,
        strategy_policy: ctx.accounts.strategy_policy.key(),
    });
    Ok(())
}
