use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, CloseAccount, Mint, TokenAccount, TokenInterface};

use crate::{
    error::AsyncVaultError,
    state::{Vault, VaultAsset, ASSET_CONFIG_SEED, ASSET_PENDING_SEED, ASSET_RESERVE_SEED},
};

#[derive(Accounts)]
pub struct RemoveVaultAsset<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    #[account(mut)]
    pub vault: Box<Account<'info, Vault>>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        close = authority,
        seeds = [ASSET_CONFIG_SEED, vault.key().as_ref(), asset_mint.key().as_ref()],
        bump = vault_asset.bump,
        constraint = vault_asset.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = vault_asset.asset_mint == asset_mint.key() @ AsyncVaultError::InvalidAssetMint,
    )]
    pub vault_asset: Box<Account<'info, VaultAsset>>,

    #[account(
        mut,
        seeds = [ASSET_RESERVE_SEED, vault.key().as_ref(), asset_mint.key().as_ref()],
        bump = vault_asset.reserve_bump,
        constraint = vault_asset.reserve == reserve.key() @ AsyncVaultError::InvalidVault,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub reserve: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [ASSET_PENDING_SEED, vault.key().as_ref(), asset_mint.key().as_ref()],
        bump = vault_asset.pending_vault_bump,
        constraint = vault_asset.pending_vault == pending_vault.key() @ AsyncVaultError::InvalidPendingVault,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub pending_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

impl<'info> RemoveVaultAsset<'info> {
    fn close_token_account(&self, account: AccountInfo<'info>) -> Result<()> {
        let share_mint = self.vault.share_mint;
        let vault_bump = self.vault.bump;
        let seeds: &[&[&[u8]]] = &[&[
            crate::state::VAULT_CONFIG_SEED,
            share_mint.as_ref(),
            &[vault_bump],
        ]];
        token_interface::close_account(CpiContext::new_with_signer(
            self.asset_token_program.key(),
            CloseAccount {
                account,
                destination: self.authority.to_account_info(),
                authority: self.vault.to_account_info(),
            },
            seeds,
        ))
    }
}

pub fn handler(ctx: Context<RemoveVaultAsset>) -> Result<()> {
    let vault = &mut ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;
    require!(
        vault.timelock_delay_slots == 0,
        AsyncVaultError::TimelockRequired
    );
    require!(
        ctx.accounts.vault_asset.idle_balance == 0
            && ctx.accounts.vault_asset.deployed_balance == 0
            && ctx.accounts.vault_asset.pending_deposit_amount == 0
            && ctx.accounts.reserve.amount == 0
            && ctx.accounts.pending_vault.amount == 0,
        AsyncVaultError::AssetBalanceNonZero
    );

    vault.approved_asset_count = vault
        .approved_asset_count
        .checked_sub(1)
        .ok_or(AsyncVaultError::ArithmeticError)?;

    ctx.accounts
        .close_token_account(ctx.accounts.reserve.to_account_info())?;
    ctx.accounts
        .close_token_account(ctx.accounts.pending_vault.to_account_info())?;

    Ok(())
}
