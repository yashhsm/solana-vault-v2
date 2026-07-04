use anchor_lang::{prelude::*, solana_program::program_option::COption};
use anchor_spl::{
    token_2022::{set_authority, spl_token_2022::instruction::AuthorityType, SetAuthority},
    token_interface::{Mint, TokenInterface},
};

use crate::{
    error::AsyncVaultError,
    state::{TrancheConfig, Vault, TRANCHE_CONFIG_SEED},
    utils::validate_share_mint_extensions_from_acct_info,
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct InitializeTranchesArgs {
    pub senior_target_bps: u16,
    pub min_junior_ratio_bps: u16,
    pub min_request_amounts: [u64; crate::state::TRANCHE_REQUEST_LIMIT_COUNT],
    pub max_request_amounts: [u64; crate::state::TRANCHE_REQUEST_LIMIT_COUNT],
}

#[derive(Accounts)]
pub struct InitializeTranches<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub mint_authority: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    #[account(mut)]
    pub senior_share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub junior_share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        space = 8 + TrancheConfig::INIT_SPACE,
        payer = payer,
        seeds = [TRANCHE_CONFIG_SEED, vault.key().as_ref()],
        bump
    )]
    pub tranche_config: Box<Account<'info, TrancheConfig>>,

    pub senior_share_token_program: Interface<'info, TokenInterface>,
    pub junior_share_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> InitializeTranches<'info> {
    fn set_senior_authority(&self) -> Result<()> {
        let cpi_accounts = SetAuthority {
            current_authority: self.mint_authority.to_account_info(),
            account_or_mint: self.senior_share_mint.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(self.senior_share_token_program.key(), cpi_accounts);
        set_authority(cpi_ctx, AuthorityType::MintTokens, Some(self.vault.key()))
    }

    fn set_junior_authority(&self) -> Result<()> {
        let cpi_accounts = SetAuthority {
            current_authority: self.mint_authority.to_account_info(),
            account_or_mint: self.junior_share_mint.to_account_info(),
        };
        let cpi_ctx = CpiContext::new(self.junior_share_token_program.key(), cpi_accounts);
        set_authority(cpi_ctx, AuthorityType::MintTokens, Some(self.vault.key()))
    }
}

fn require_mint_owned_by(mint: &InterfaceAccount<Mint>, authority: Pubkey) -> Result<()> {
    match mint.mint_authority {
        COption::Some(current_authority) => {
            require_keys_eq!(
                current_authority,
                authority,
                AsyncVaultError::UnauthorizedSigner
            );
            Ok(())
        }
        COption::None => err!(AsyncVaultError::UnauthorizedSigner),
    }
}

pub fn handler(ctx: Context<InitializeTranches>, args: InitializeTranchesArgs) -> Result<()> {
    ctx.accounts.vault.assert_uninitialized()?;
    ctx.accounts
        .vault
        .assert_curator(ctx.accounts.authority.key())?;
    require!(
        ctx.accounts.vault.tranche_config.is_none(),
        AsyncVaultError::ExtensionAlreadyInitialized
    );

    require!(
        args.senior_target_bps <= vault_common::MAX_BPS
            && args.min_junior_ratio_bps <= vault_common::MAX_BPS,
        AsyncVaultError::FeeBpsExceeded
    );
    for (min_amount, max_amount) in args
        .min_request_amounts
        .iter()
        .zip(args.max_request_amounts.iter())
    {
        require!(
            *max_amount == 0 || *min_amount <= *max_amount,
            AsyncVaultError::InvalidTrancheRequestLimitConfig
        );
    }

    let vault_share_mint = ctx.accounts.vault.share_mint;
    let senior_share_mint = ctx.accounts.senior_share_mint.key();
    let junior_share_mint = ctx.accounts.junior_share_mint.key();

    require_keys_neq!(
        senior_share_mint,
        junior_share_mint,
        AsyncVaultError::MintsShouldBeDifferent
    );
    require_keys_neq!(
        senior_share_mint,
        ctx.accounts.vault.asset_mint,
        AsyncVaultError::InvalidShareMint
    );
    require_keys_neq!(
        junior_share_mint,
        ctx.accounts.vault.asset_mint,
        AsyncVaultError::InvalidShareMint
    );
    require!(
        senior_share_mint == vault_share_mint || junior_share_mint == vault_share_mint,
        AsyncVaultError::InvalidShareMint
    );

    require!(
        ctx.accounts.senior_share_mint.supply == 0 && ctx.accounts.junior_share_mint.supply == 0,
        AsyncVaultError::ShareMintSupplyShouldBeZero
    );

    require_keys_eq!(
        *ctx.accounts.senior_share_mint.to_account_info().owner,
        ctx.accounts.senior_share_token_program.key(),
        AsyncVaultError::InvalidShareMint
    );
    require_keys_eq!(
        *ctx.accounts.junior_share_mint.to_account_info().owner,
        ctx.accounts.junior_share_token_program.key(),
        AsyncVaultError::InvalidShareMint
    );

    validate_share_mint_extensions_from_acct_info(
        &ctx.accounts.senior_share_mint.to_account_info(),
    )?;
    validate_share_mint_extensions_from_acct_info(
        &ctx.accounts.junior_share_mint.to_account_info(),
    )?;

    if senior_share_mint == vault_share_mint {
        require_mint_owned_by(&ctx.accounts.senior_share_mint, ctx.accounts.vault.key())?;
        ctx.accounts.set_junior_authority()?;
    } else {
        require_mint_owned_by(&ctx.accounts.junior_share_mint, ctx.accounts.vault.key())?;
        ctx.accounts.set_senior_authority()?;
    }

    ctx.accounts.tranche_config.set_inner(TrancheConfig {
        vault: ctx.accounts.vault.key(),
        senior_share_mint,
        junior_share_mint,
        senior_nav: 0,
        junior_nav: 0,
        senior_supply: 0,
        junior_supply: 0,
        senior_target_bps: args.senior_target_bps,
        min_junior_ratio_bps: args.min_junior_ratio_bps,
        min_request_amounts: args.min_request_amounts,
        max_request_amounts: args.max_request_amounts,
        senior_subscription_request_total: 0,
        senior_subscription_request_last_processed: 0,
        senior_redemption_request_total: 0,
        senior_redemption_request_last_processed: 0,
        junior_subscription_request_total: 0,
        junior_subscription_request_last_processed: 0,
        junior_redemption_request_total: 0,
        junior_redemption_request_last_processed: 0,
        last_waterfall_slot: 0,
        last_waterfall_timestamp: 0,
        bump: ctx.bumps.tranche_config,
    });
    ctx.accounts.vault.tranche_config = Some(ctx.accounts.tranche_config.key());

    Ok(())
}
