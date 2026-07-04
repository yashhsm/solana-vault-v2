use anchor_lang::prelude::*;
use anchor_spl::token_interface::{Mint, TokenAccount, TokenInterface};

use crate::{
    error::AsyncVaultError,
    state::{
        Vault, VaultAsset, ASSET_CONFIG_SEED, ASSET_PENDING_SEED, ASSET_RESERVE_SEED,
        MAX_APPROVED_ASSETS,
    },
    utils::validate_asset_mint_extensions_from_acct_info,
};

#[derive(AnchorDeserialize, AnchorSerialize)]
pub struct AddVaultAssetArgs {
    pub deposit_cap: u64,
}

#[derive(Accounts)]
pub struct AddVaultAsset<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = payer,
        space = 8 + VaultAsset::INIT_SPACE,
        seeds = [ASSET_CONFIG_SEED, vault.key().as_ref(), asset_mint.key().as_ref()],
        bump,
    )]
    pub vault_asset: Box<Account<'info, VaultAsset>>,

    #[account(
        init,
        token::authority = vault,
        token::mint = asset_mint,
        token::token_program = asset_token_program,
        payer = payer,
        seeds = [ASSET_RESERVE_SEED, vault.key().as_ref(), asset_mint.key().as_ref()],
        bump,
    )]
    pub reserve: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        token::authority = vault,
        token::mint = asset_mint,
        token::token_program = asset_token_program,
        payer = payer,
        seeds = [ASSET_PENDING_SEED, vault.key().as_ref(), asset_mint.key().as_ref()],
        bump,
    )]
    pub pending_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

pub fn handler(ctx: Context<AddVaultAsset>, args: AddVaultAssetArgs) -> Result<()> {
    let vault = &mut ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots == 0,
        AsyncVaultError::TimelockRequired
    );
    require_keys_neq!(
        ctx.accounts.asset_mint.key(),
        vault.asset_mint,
        AsyncVaultError::AssetAlreadyApproved
    );
    require!(
        vault.approved_asset_count < MAX_APPROVED_ASSETS,
        AsyncVaultError::MaxApprovedAssetsExceeded
    );
    validate_asset_mint_extensions_from_acct_info(&ctx.accounts.asset_mint.to_account_info())?;

    ctx.accounts.vault_asset.set_inner(VaultAsset {
        vault: vault.key(),
        asset_mint: ctx.accounts.asset_mint.key(),
        reserve: ctx.accounts.reserve.key(),
        pending_vault: ctx.accounts.pending_vault.key(),
        idle_balance: 0,
        deployed_balance: 0,
        pending_deposit_amount: 0,
        deposit_cap: args.deposit_cap,
        manager_window_start_slot: 0,
        manager_window_amount: 0,
        reserve_bump: ctx.bumps.reserve,
        pending_vault_bump: ctx.bumps.pending_vault,
        bump: ctx.bumps.vault_asset,
    });

    vault.approved_asset_count = vault
        .approved_asset_count
        .checked_add(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    Ok(())
}
