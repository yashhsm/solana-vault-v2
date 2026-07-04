use anchor_lang::prelude::*;
use anchor_spl::token_interface::Mint;

use crate::{
    error::AsyncVaultError,
    state::{NavMode, Vault, VAULT_CONFIG_SEED},
};

#[derive(AnchorDeserialize, AnchorSerialize, Clone, InitSpace)]
pub struct UpdateVaultArgs {
    pub paused: Option<bool>,
    pub fee_recipient: Option<Pubkey>,
    pub manager: Option<Pubkey>,
    pub hot_manager: Option<Pubkey>,
    pub fulfiller: Option<Pubkey>,
    pub breaker: Option<Pubkey>,
    pub nav_mode: Option<NavMode>,
    pub require_fresh_nav: Option<bool>,
    pub max_nav_delta_bps: Option<u16>,
    pub max_implied_apy_bps: Option<u32>,
    pub max_nav_staleness_slots: Option<u64>,
    pub deposit_cap: Option<u64>,
    pub rolling_limit_window_slots: Option<u64>,
    pub manager_rolling_limit: Option<u64>,
    pub external_withdraw_rolling_limit: Option<u64>,
    pub redemption_rolling_limit: Option<u64>,
    pub timelock_delay_slots: Option<u64>,
    pub performance_fee_bps: Option<u16>,
    pub performance_fee_crystallization_interval_seconds: Option<u64>,
    pub protocol_fee_bps: Option<u16>,
    pub protocol_fee_recipient: Option<Pubkey>,
    pub instant_redemption_fee_bps: Option<u16>,
}

impl UpdateVaultArgs {
    pub fn has_timelocked_fields(&self) -> bool {
        self.fee_recipient.is_some()
            || self.manager.is_some()
            || self.hot_manager.is_some()
            || self.fulfiller.is_some()
            || self.nav_mode.is_some()
            || self.require_fresh_nav.is_some()
            || self.max_nav_delta_bps.is_some()
            || self.max_implied_apy_bps.is_some()
            || self.max_nav_staleness_slots.is_some()
            || self.deposit_cap.is_some()
            || self.rolling_limit_window_slots.is_some()
            || self.manager_rolling_limit.is_some()
            || self.external_withdraw_rolling_limit.is_some()
            || self.redemption_rolling_limit.is_some()
            || self.timelock_delay_slots.is_some()
            || self.performance_fee_bps.is_some()
            || self
                .performance_fee_crystallization_interval_seconds
                .is_some()
            || self.protocol_fee_bps.is_some()
            || self.protocol_fee_recipient.is_some()
            || self.instant_redemption_fee_bps.is_some()
    }

    pub fn has_pause_change(&self) -> bool {
        self.paused.is_some()
    }
}

#[derive(Accounts)]
pub struct UpdateVault<'info> {
    pub authority: Signer<'info>,

    pub share_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [VAULT_CONFIG_SEED, share_mint.key().as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
}

pub fn handler(ctx: Context<UpdateVault>, args: UpdateVaultArgs) -> Result<()> {
    let vault = &mut ctx.accounts.vault;
    vault.assert_curator(ctx.accounts.authority.key())?;

    if vault.timelock_delay_slots > 0 && args.has_timelocked_fields() {
        return err!(AsyncVaultError::TimelockRequired);
    }

    apply_vault_update(vault, &args)
}

pub fn validate_queued_vault_update(vault: &Vault, args: &UpdateVaultArgs) -> Result<()> {
    require!(
        args.has_timelocked_fields() && !args.has_pause_change(),
        AsyncVaultError::InvalidTimelockChange
    );
    require!(
        args.breaker.is_none(),
        AsyncVaultError::InvalidTimelockChange
    );
    validate_update_args_supported(vault, args)
}

fn validate_update_args_supported(vault: &Vault, args: &UpdateVaultArgs) -> Result<()> {
    let max_bps = vault_common::MAX_BPS;
    if let Some(max_nav_delta_bps) = args.max_nav_delta_bps {
        require!(
            max_nav_delta_bps <= max_bps,
            AsyncVaultError::FeeBpsExceeded
        );
    }
    if let Some(performance_fee_bps) = args.performance_fee_bps {
        require!(
            performance_fee_bps <= max_bps,
            AsyncVaultError::FeeBpsExceeded
        );
    }
    if let Some(protocol_fee_bps) = args.protocol_fee_bps {
        require!(protocol_fee_bps <= max_bps, AsyncVaultError::FeeBpsExceeded);
    }
    let protocol_fee_bps = args.protocol_fee_bps.unwrap_or(vault.protocol_fee_bps);
    let protocol_fee_recipient = args
        .protocol_fee_recipient
        .unwrap_or(vault.protocol_fee_recipient);
    if protocol_fee_bps > 0 {
        require!(
            protocol_fee_recipient != Pubkey::default(),
            AsyncVaultError::InvalidFeeRecipient
        );
    }
    if let Some(instant_redemption_fee_bps) = args.instant_redemption_fee_bps {
        require!(
            instant_redemption_fee_bps <= max_bps,
            AsyncVaultError::FeeBpsExceeded
        );
    }
    if let Some(nav_mode) = args.nav_mode {
        require!(
            nav_mode == NavMode::AuthoritySigned,
            AsyncVaultError::UnsupportedPhaseConfig
        );
    }
    let rolling_limit_window_slots = args
        .rolling_limit_window_slots
        .unwrap_or(vault.rolling_limit_window_slots);
    let manager_rolling_limit = args
        .manager_rolling_limit
        .unwrap_or(vault.manager_rolling_limit);
    let external_withdraw_rolling_limit = args
        .external_withdraw_rolling_limit
        .unwrap_or(vault.external_withdraw_rolling_limit);
    let redemption_rolling_limit = args
        .redemption_rolling_limit
        .unwrap_or(vault.redemption_rolling_limit);
    require!(
        rolling_limit_window_slots > 0
            || (manager_rolling_limit == 0
                && external_withdraw_rolling_limit == 0
                && redemption_rolling_limit == 0),
        AsyncVaultError::InvalidRollingLimitConfig
    );

    Ok(())
}

pub fn apply_vault_update(vault: &mut Vault, args: &UpdateVaultArgs) -> Result<()> {
    if let Some(paused) = args.paused {
        vault.paused = paused;
    }

    if let Some(fee_recipient) = args.fee_recipient {
        vault.fee_recipient = fee_recipient;
    }
    if let Some(manager) = args.manager {
        vault.manager = manager;
    }
    if let Some(hot_manager) = args.hot_manager {
        vault.hot_manager = hot_manager;
    }
    if let Some(fulfiller) = args.fulfiller {
        vault.fulfiller = fulfiller;
    }
    if let Some(breaker) = args.breaker {
        vault.breaker = breaker;
    }
    if let Some(nav_mode) = args.nav_mode {
        vault.nav_mode = nav_mode;
    }
    if let Some(require_fresh_nav) = args.require_fresh_nav {
        vault.require_fresh_nav = require_fresh_nav;
    }
    if let Some(max_nav_delta_bps) = args.max_nav_delta_bps {
        vault.max_nav_delta_bps = max_nav_delta_bps;
    }
    if let Some(max_implied_apy_bps) = args.max_implied_apy_bps {
        vault.max_implied_apy_bps = max_implied_apy_bps;
    }
    if let Some(max_nav_staleness_slots) = args.max_nav_staleness_slots {
        vault.max_nav_staleness_slots = max_nav_staleness_slots;
    }
    if let Some(deposit_cap) = args.deposit_cap {
        vault.deposit_cap = deposit_cap;
    }
    if let Some(rolling_limit_window_slots) = args.rolling_limit_window_slots {
        vault.rolling_limit_window_slots = rolling_limit_window_slots;
    }
    if let Some(manager_rolling_limit) = args.manager_rolling_limit {
        vault.manager_rolling_limit = manager_rolling_limit;
    }
    if let Some(external_withdraw_rolling_limit) = args.external_withdraw_rolling_limit {
        vault.external_withdraw_rolling_limit = external_withdraw_rolling_limit;
    }
    if let Some(redemption_rolling_limit) = args.redemption_rolling_limit {
        vault.redemption_rolling_limit = redemption_rolling_limit;
    }
    if let Some(timelock_delay_slots) = args.timelock_delay_slots {
        vault.timelock_delay_slots = timelock_delay_slots;
    }
    if let Some(performance_fee_bps) = args.performance_fee_bps {
        vault.performance_fee_bps = performance_fee_bps;
    }
    if let Some(performance_fee_crystallization_interval_seconds) =
        args.performance_fee_crystallization_interval_seconds
    {
        vault.performance_fee_crystallization_interval_seconds =
            performance_fee_crystallization_interval_seconds;
    }
    if let Some(protocol_fee_bps) = args.protocol_fee_bps {
        vault.protocol_fee_bps = protocol_fee_bps;
    }
    if let Some(protocol_fee_recipient) = args.protocol_fee_recipient {
        vault.protocol_fee_recipient = protocol_fee_recipient;
    }
    if let Some(instant_redemption_fee_bps) = args.instant_redemption_fee_bps {
        vault.instant_redemption_fee_bps = instant_redemption_fee_bps;
    }

    vault.validate_config()?;

    Ok(())
}
