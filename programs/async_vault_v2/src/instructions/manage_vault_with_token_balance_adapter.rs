use anchor_lang::prelude::*;
use anchor_spl::token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked};

use crate::{
    error::AsyncVaultError,
    events::TokenBalanceAdapterExecuted,
    state::{
        Position, PositionLedger, StrategyPolicy, TokenBalanceAdapterAction, Vault, VaultAsset,
        VaultVenue, VenueEntry, ASSET_CONFIG_SEED, POSITION_SEED, POSITION_TOKEN_SEED,
        STRATEGY_POLICY_SEED, VAULT_CONFIG_SEED, VAULT_VENUE_SEED,
    },
    utils::merkle::{
        hash_token_balance_adapter_leaf, verify_strategy_proof, TokenBalanceAdapterLeaf,
    },
};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct ManageVaultWithTokenBalanceAdapterArgs {
    pub policy_version: u64,
    pub action: TokenBalanceAdapterAction,
    pub amount: u64,
    /// Per-call bound committed into the Merkle leaf. `amount` remains dynamic.
    pub policy_max_amount: u64,
    pub proof: Vec<[u8; 32]>,
}

#[derive(Accounts)]
pub struct ManageVaultWithTokenBalanceAdapter<'info> {
    pub strategist: Signer<'info>,

    #[account(
        mut,
        constraint = share_mint.key() == vault.share_mint @ AsyncVaultError::InvalidShareMint,
    )]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Box<Account<'info, Vault>>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(mut)]
    pub vault_asset: Option<Box<Account<'info, VaultAsset>>>,

    pub venue_entry: Box<Account<'info, VenueEntry>>,

    #[account(
        seeds = [VAULT_VENUE_SEED, vault.key().as_ref(), venue_entry.key().as_ref()],
        bump = vault_venue.bump,
        constraint = vault_venue.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = vault_venue.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
    )]
    pub vault_venue: Box<Account<'info, VaultVenue>>,

    #[account(
        mut,
        seeds = [
            STRATEGY_POLICY_SEED,
            vault.key().as_ref(),
            strategist.key().as_ref(),
        ],
        bump = strategy_policy.bump,
        constraint = strategy_policy.vault == vault.key() @ AsyncVaultError::InvalidStrategyPolicy,
        constraint = strategy_policy.strategist == strategist.key() @ AsyncVaultError::InvalidStrategyPolicy,
    )]
    pub strategy_policy: Box<Account<'info, StrategyPolicy>>,

    #[account(
        mut,
        seeds = [
            POSITION_SEED,
            vault.key().as_ref(),
            venue_entry.key().as_ref(),
            asset_mint.key().as_ref(),
        ],
        bump = position.bump,
        constraint = position.vault == vault.key() @ AsyncVaultError::InvalidVault,
        constraint = position.venue_entry == venue_entry.key() @ AsyncVaultError::InvalidVenueEntry,
        constraint = position.vault_venue == vault_venue.key() @ AsyncVaultError::InvalidVenueEntry,
        constraint = position.asset_mint == asset_mint.key() @ AsyncVaultError::InvalidAssetMint,
    )]
    pub position: Box<Account<'info, Position>>,

    #[account(
        mut,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
    )]
    pub vault_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [
            POSITION_TOKEN_SEED,
            vault.key().as_ref(),
            venue_entry.key().as_ref(),
            asset_mint.key().as_ref(),
        ],
        bump = position.token_account_bump,
        token::mint = asset_mint,
        token::authority = vault,
        token::token_program = asset_token_program,
        constraint = position.token_account == position_token_account.key()
            @ AsyncVaultError::InvalidPosition,
    )]
    pub position_token_account: Box<InterfaceAccount<'info, TokenAccount>>,

    pub asset_token_program: Interface<'info, TokenInterface>,
}

impl<'info> ManageVaultWithTokenBalanceAdapter<'info> {
    fn transfer(&self, action: TokenBalanceAdapterAction, amount: u64) -> Result<()> {
        let (from, to) = match action {
            TokenBalanceAdapterAction::Deploy => (
                self.vault_token_account.to_account_info(),
                self.position_token_account.to_account_info(),
            ),
            TokenBalanceAdapterAction::Pull => (
                self.position_token_account.to_account_info(),
                self.vault_token_account.to_account_info(),
            ),
        };
        let signer_seeds: &[&[&[u8]]] = &[&[
            VAULT_CONFIG_SEED,
            self.vault.share_mint.as_ref(),
            &[self.vault.bump],
        ]];
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                self.asset_token_program.key(),
                TransferChecked {
                    from,
                    mint: self.asset_mint.to_account_info(),
                    to,
                    authority: self.vault.to_account_info(),
                },
                signer_seeds,
            ),
            amount,
            self.asset_mint.decimals,
        )
    }
}

pub fn handler(
    ctx: Context<ManageVaultWithTokenBalanceAdapter>,
    args: ManageVaultWithTokenBalanceAdapterArgs,
) -> Result<()> {
    ctx.accounts.vault.assert_unpaused_and_initialized()?;
    ctx.accounts
        .strategy_policy
        .assert_bound(ctx.accounts.vault.key(), ctx.accounts.strategist.key())?;
    ctx.accounts.strategy_policy.assert_active()?;
    require!(
        args.policy_version == ctx.accounts.strategy_policy.version,
        AsyncVaultError::StaleStrategyPolicyVersion
    );
    require!(
        args.amount > 0 && args.policy_max_amount > 0,
        AsyncVaultError::InvalidStrategyAdapter
    );
    require!(
        args.amount <= args.policy_max_amount,
        AsyncVaultError::StrategyAdapterAmountExceeded
    );

    ctx.accounts.venue_entry.assert_active()?;
    ctx.accounts.vault_venue.assert_active()?;
    ctx.accounts
        .vault_venue
        .assert_manager_authorized(&ctx.accounts.vault, ctx.accounts.strategist.key())?;
    require_keys_eq!(
        ctx.accounts.asset_token_program.key(),
        ctx.accounts.venue_entry.target_program,
        AsyncVaultError::TargetProgramMismatch
    );
    require_keys_eq!(
        ctx.accounts.asset_token_program.key(),
        ctx.accounts.vault_venue.target_program,
        AsyncVaultError::TargetProgramMismatch
    );

    let is_primary_asset = ctx
        .accounts
        .vault
        .is_primary_asset(ctx.accounts.asset_mint.key());
    if is_primary_asset {
        require!(
            ctx.accounts.vault_asset.is_none(),
            AsyncVaultError::InvalidStrategyAdapter
        );
        require_keys_eq!(
            ctx.accounts.vault.vault_token_account,
            ctx.accounts.vault_token_account.key(),
            AsyncVaultError::InvalidVault
        );
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_ref()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        let expected_vault_asset = Pubkey::find_program_address(
            &[
                ASSET_CONFIG_SEED,
                ctx.accounts.vault.key().as_ref(),
                ctx.accounts.asset_mint.key().as_ref(),
            ],
            ctx.program_id,
        )
        .0;
        require_keys_eq!(
            vault_asset.key(),
            expected_vault_asset,
            AsyncVaultError::InvalidAssetMint
        );
        vault_asset.assert_matches(ctx.accounts.vault.key(), ctx.accounts.asset_mint.key())?;
        vault_asset.assert_reserve(ctx.accounts.vault_token_account.key())?;
    }

    let reserve_before = ctx.accounts.vault_token_account.amount;
    let position_before = ctx.accounts.position_token_account.amount;
    require!(
        ctx.accounts.position.amount == position_before,
        AsyncVaultError::PositionAccountingMismatch
    );
    if matches!(args.action, TokenBalanceAdapterAction::Pull) {
        require!(
            position_before >= args.amount,
            AsyncVaultError::InsufficientPositionBalance
        );
    }

    let leaf = hash_token_balance_adapter_leaf(TokenBalanceAdapterLeaf {
        vault: ctx.accounts.vault.key(),
        strategist: ctx.accounts.strategist.key(),
        policy_version: args.policy_version,
        venue_entry: ctx.accounts.venue_entry.key(),
        vault_venue: ctx.accounts.vault_venue.key(),
        target_program: ctx.accounts.asset_token_program.key(),
        action: args.action,
        asset_mint: ctx.accounts.asset_mint.key(),
        vault_token_account: ctx.accounts.vault_token_account.key(),
        position: ctx.accounts.position.key(),
        position_token_account: ctx.accounts.position_token_account.key(),
        policy_max_amount: args.policy_max_amount,
    });
    verify_strategy_proof(leaf, &args.proof, ctx.accounts.strategy_policy.merkle_root)?;

    let current_slot = Clock::get()?.slot;
    let secondary_ledger = if is_primary_asset {
        ctx.accounts
            .vault
            .consume_manager_rolling_limit(args.amount, current_slot)?;
        None
    } else {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_mut()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.consume_manager_rolling_limit(
            args.amount,
            current_slot,
            ctx.accounts.vault.manager_rolling_limit,
            ctx.accounts.vault.rolling_limit_window_slots,
        )?;
        let ledger = PositionLedger::new(
            ctx.accounts.position.amount,
            vault_asset.idle_balance,
            vault_asset.deployed_balance,
        );
        Some(match args.action {
            TokenBalanceAdapterAction::Deploy => ledger.deploy(args.amount)?,
            TokenBalanceAdapterAction::Pull => ledger.pull(args.amount)?,
        })
    };

    let share_supply_before = ctx.accounts.share_mint.supply;
    ctx.accounts.strategy_policy.executing = true;
    // Flush guards and rolling-limit state before CPI. A failed CPI or invariant
    // check rolls the entire transaction back.
    ctx.accounts.strategy_policy.exit(ctx.program_id)?;
    ctx.accounts.vault.exit(ctx.program_id)?;
    if let Some(vault_asset) = ctx.accounts.vault_asset.as_mut() {
        vault_asset.exit(ctx.program_id)?;
    }
    ctx.accounts.transfer(args.action, args.amount)?;
    ctx.accounts.strategy_policy.executing = false;

    ctx.accounts.vault_token_account.reload()?;
    ctx.accounts.position_token_account.reload()?;
    ctx.accounts.share_mint.reload()?;
    require!(
        ctx.accounts.share_mint.supply == share_supply_before,
        AsyncVaultError::ShareSupplyChanged
    );

    let (expected_reserve_after, expected_position_after) = match args.action {
        TokenBalanceAdapterAction::Deploy => (
            reserve_before
                .checked_sub(args.amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
            position_before
                .checked_add(args.amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
        ),
        TokenBalanceAdapterAction::Pull => (
            reserve_before
                .checked_add(args.amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
            position_before
                .checked_sub(args.amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
        ),
    };
    require!(
        ctx.accounts.vault_token_account.amount == expected_reserve_after
            && ctx.accounts.position_token_account.amount == expected_position_after,
        AsyncVaultError::InvalidBalanceDelta
    );

    ctx.accounts.position.amount = expected_position_after;
    if let Some(ledger) = secondary_ledger {
        let vault_asset = ctx
            .accounts
            .vault_asset
            .as_mut()
            .ok_or(AsyncVaultError::InvalidAssetMint)?;
        vault_asset.idle_balance = ledger.idle_balance;
        vault_asset.deployed_balance = ledger.deployed_balance;
    }

    emit!(TokenBalanceAdapterExecuted {
        vault: ctx.accounts.vault.key(),
        strategy_policy: ctx.accounts.strategy_policy.key(),
        strategist: ctx.accounts.strategist.key(),
        venue_entry: ctx.accounts.venue_entry.key(),
        vault_venue: ctx.accounts.vault_venue.key(),
        position: ctx.accounts.position.key(),
        asset_mint: ctx.accounts.asset_mint.key(),
        token_program: ctx.accounts.asset_token_program.key(),
        policy_version: args.policy_version,
        action: args.action,
        amount: args.amount,
        policy_max_amount: args.policy_max_amount,
        reserve_balance_before: reserve_before,
        reserve_balance_after: ctx.accounts.vault_token_account.amount,
        position_balance_before: position_before,
        position_balance_after: ctx.accounts.position_token_account.amount,
        leaf,
    });
    Ok(())
}
