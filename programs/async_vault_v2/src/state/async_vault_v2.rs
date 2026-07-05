use anchor_lang::prelude::*;

use crate::error::AsyncVaultError;

use super::request::RequestType;

const SECONDS_PER_YEAR: u128 = 31_536_000;

pub const TRANCHE_REQUEST_LIMIT_COUNT: usize = 4;

/// Program-wide protocol fee routing config.
#[account]
#[derive(InitSpace)]
pub struct ProtocolFeeConfig {
    /// signer allowed to update the program-level protocol fee recipient
    pub authority: Pubkey,
    /// owner required for protocol fee token accounts when this config is supplied
    pub protocol_fee_recipient: Pubkey,
    pub bump: u8,
}

/// NAV validation mode configured for a vault.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, PartialEq, Eq)]
pub enum NavMode {
    /// Curator or fulfiller signs the NAV update directly.
    AuthoritySigned,
    /// NAV must be computed from oracle-priced balances.
    Oracle,
    /// Authority-signed NAV must remain within oracle-computed bands.
    Hybrid,
}

#[account]
#[derive(InitSpace)]
pub struct Vault {
    pub asset_mint: Pubkey,
    /// number of approved assets, including the primary upstream-compatible asset
    pub approved_asset_count: u8,
    /// share mint address
    pub share_mint: Pubkey,
    /// token account holding confirmed vault assets
    pub vault_token_account: Pubkey,
    /// authority that can sign permissioned instructions
    pub authority: Pubkey,
    /// role that owns configuration, role management, and unpause authority
    pub curator: Pubkey,
    /// role allowed to deploy, pull, and rebalance capital
    pub manager: Pubkey,
    /// restricted manager role for routine-safe operations
    pub hot_manager: Pubkey,
    /// role allowed to fulfill async requests and authority-signed NAV updates
    pub fulfiller: Pubkey,
    /// role allowed only to pause the vault
    pub breaker: Pubkey,
    /// pubkey that is required to own the TokenAccount fees are sent to
    pub fee_recipient: Pubkey,
    /// paused
    pub paused: bool,
    /// once a vault is initialized, no extensions can be added
    pub initialized: bool,
    /// token account holding assets from deposits awaiting share issuance
    pub pending_vault: Pubkey,
    /// net asset value (assets per share), default 0 until first NAV update
    pub nav: u128,
    /// nav version, incremented on each NAV update
    pub nav_version: u64,
    /// NAV validation mode used by pricing-dependent instructions
    pub nav_mode: NavMode,
    /// when true, settlement requires a NAV update after request creation
    pub require_fresh_nav: bool,
    /// maximum allowed NAV delta per update in basis points; zero disables the guard
    pub max_nav_delta_bps: u16,
    /// maximum annualized NAV increase in basis points; zero disables the guard
    pub max_implied_apy_bps: u32,
    /// maximum allowed slots since last NAV update; zero disables the guard
    pub max_nav_staleness_slots: u64,
    /// slot of the most recent NAV update
    pub last_nav_update_slot: u64,
    /// unix timestamp of the most recent NAV update
    pub last_nav_update_timestamp: i64,
    /// count of pending async deposit/withdrawal requests
    pub pending_async_requests: u16,
    /// virtual vault asset balance, accounts for tokens that may
    /// have been withdrawn by the vault authority
    pub total_asset_balance: u64,
    /// maximum total assets accepted by the vault; zero disables the cap
    pub deposit_cap: u64,
    /// pending deposit amount reserved against the deposit cap
    pub pending_deposit_amount: u64,
    /// rolling-limit accounting window in slots; zero disables phase-1 limits
    pub rolling_limit_window_slots: u64,
    /// manager deploy/pull cap per rolling window
    pub manager_rolling_limit: u64,
    /// start slot for manager deploy/pull rolling-limit accounting
    pub manager_window_start_slot: u64,
    /// consumed manager deploy/pull amount in the current rolling window
    pub manager_window_amount: u64,
    /// externally managed withdrawal cap per rolling window
    pub external_withdraw_rolling_limit: u64,
    /// aggregate redemption settlement cap per rolling window
    pub redemption_rolling_limit: u64,
    /// start slot for externally managed withdrawal rolling-limit accounting
    pub external_withdraw_window_start_slot: u64,
    /// consumed external-withdraw amount in the current rolling window
    pub external_withdraw_window_amount: u64,
    /// start slot for redemption-settlement rolling-limit accounting
    pub redemption_window_start_slot: u64,
    /// consumed redemption-settlement amount in the current rolling window
    pub redemption_window_amount: u64,
    /// curator config timelock delay in slots
    pub timelock_delay_slots: u64,
    /// performance fee in basis points
    pub performance_fee_bps: u16,
    /// minimum seconds between performance-fee crystallizations; zero disables the interval gate
    pub performance_fee_crystallization_interval_seconds: u64,
    /// protocol fee skim in basis points
    pub protocol_fee_bps: u16,
    /// pubkey that is required to own the TokenAccount protocol fee skims are sent to
    pub protocol_fee_recipient: Pubkey,
    /// instant-redemption fee in basis points
    pub instant_redemption_fee_bps: u16,
    /// high-water mark used by performance fee accounting
    pub high_water_mark: u128,
    /// optional tranche configuration PDA; when set, NAV updates must run waterfall accounting
    pub tranche_config: Option<Pubkey>,
    /// unix timestamp of the last performance-fee crystallization
    pub last_fee_crystallization_timestamp: i64,
    pub reserve_bump: u8,
    pub pending_vault_bump: u8,
    pub bump: u8,
    // Used for updating the vault authority (New Authority)
    pub pending_authority: Option<Pubkey>,
}

/// Per-asset account for Phase 2 multi-asset custody.
#[account]
#[derive(InitSpace)]
pub struct VaultAsset {
    /// vault this asset belongs to
    pub vault: Pubkey,
    /// approved asset mint
    pub asset_mint: Pubkey,
    /// PDA token account holding idle/confirmed assets for this mint
    pub reserve: Pubkey,
    /// PDA token account holding deposits awaiting settlement for this mint
    pub pending_vault: Pubkey,
    /// idle accounting balance for this asset
    pub idle_balance: u64,
    /// deployed accounting balance for this asset
    pub deployed_balance: u64,
    /// pending deposit reservations for this asset
    pub pending_deposit_amount: u64,
    /// per-asset cap; zero disables the cap
    pub deposit_cap: u64,
    /// slot when this secondary asset's manager rolling-limit window started
    pub manager_window_start_slot: u64,
    /// amount consumed in this secondary asset's current manager rolling window
    pub manager_window_amount: u64,
    pub reserve_bump: u8,
    pub pending_vault_bump: u8,
    pub bump: u8,
}

impl VaultAsset {
    pub fn consume_manager_rolling_limit(
        &mut self,
        amount: u64,
        current_slot: u64,
        manager_rolling_limit: u64,
        rolling_limit_window_slots: u64,
    ) -> Result<()> {
        Vault::consume_rolling_limit(
            amount,
            current_slot,
            manager_rolling_limit,
            rolling_limit_window_slots,
            &mut self.manager_window_start_slot,
            &mut self.manager_window_amount,
        )
    }
}

/// Per-user rolling-limit buckets for primary instant settlement.
#[account]
#[derive(InitSpace)]
pub struct InstantSettlementUser {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub deposit_window_start_slot: u64,
    pub deposit_window_amount: u64,
    pub redeem_window_start_slot: u64,
    pub redeem_window_amount: u64,
    pub bump: u8,
}

impl InstantSettlementUser {
    pub fn ensure_initialized(&mut self, vault: Pubkey, user: Pubkey, bump: u8) -> Result<()> {
        if self.vault == Pubkey::default() && self.user == Pubkey::default() {
            self.vault = vault;
            self.user = user;
            self.bump = bump;
            return Ok(());
        }

        require_keys_eq!(self.vault, vault, AsyncVaultError::InvalidVault);
        require_keys_eq!(self.user, user, AsyncVaultError::UnauthorizedSigner);
        Ok(())
    }

    pub fn consume_deposit_limit(
        &mut self,
        amount: u64,
        current_slot: u64,
        limit: u64,
        window_slots: u64,
    ) -> Result<()> {
        Vault::consume_rolling_limit(
            amount,
            current_slot,
            limit,
            window_slots,
            &mut self.deposit_window_start_slot,
            &mut self.deposit_window_amount,
        )
    }

    pub fn consume_redeem_limit(
        &mut self,
        amount: u64,
        current_slot: u64,
        limit: u64,
        window_slots: u64,
    ) -> Result<()> {
        Vault::consume_rolling_limit(
            amount,
            current_slot,
            limit,
            window_slots,
            &mut self.redeem_window_start_slot,
            &mut self.redeem_window_amount,
        )
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, InitSpace, PartialEq, Eq)]
pub enum VenueType {
    ExternalProtocol,
    VaultV2,
}

/// Program-level venue entry created by a registry authority namespace.
#[account]
#[derive(InitSpace)]
pub struct VenueEntry {
    /// signer namespace that created and can pause this venue entry
    pub registry_authority: Pubkey,
    /// fixed caller-defined venue identifier
    pub venue_id: [u8; 32],
    /// program that future validated CPI actions may target
    pub target_program: Pubkey,
    /// number of active 8-byte discriminators in `allowed_discriminators`
    pub allowed_discriminator_count: u8,
    /// flat list of allowed 8-byte instruction discriminators
    pub allowed_discriminators: [u8; crate::state::MAX_VENUE_DISCRIMINATOR_BYTES],
    /// integrator-defined risk class for reading/UI and later policy checks
    pub risk_class: u8,
    pub venue_type: VenueType,
    /// whether the hot manager may eventually use this venue for routine actions
    pub routine_safe: bool,
    /// paused entries cannot be newly approved by vaults
    pub paused: bool,
    pub bump: u8,
}

/// Per-vault approval for a venue entry.
#[account]
#[derive(InitSpace)]
pub struct VaultVenue {
    pub vault: Pubkey,
    pub venue_entry: Pubkey,
    pub target_program: Pubkey,
    pub routine_safe: bool,
    pub paused: bool,
    /// future deploy/pull instructions must keep this at zero before removal
    pub position_count: u16,
    pub approved_at_slot: u64,
    pub bump: u8,
}

/// Per-asset deployed-position ledger for an approved venue.
#[account]
#[derive(InitSpace)]
pub struct Position {
    pub vault: Pubkey,
    pub venue_entry: Pubkey,
    pub vault_venue: Pubkey,
    pub asset_mint: Pubkey,
    pub token_account: Pubkey,
    pub amount: u64,
    pub token_account_bump: u8,
    pub bump: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionLedger {
    pub position_amount: u64,
    pub idle_balance: u64,
    pub deployed_balance: u64,
}

impl PositionLedger {
    pub fn new(position_amount: u64, idle_balance: u64, deployed_balance: u64) -> Self {
        Self {
            position_amount,
            idle_balance,
            deployed_balance,
        }
    }

    pub fn deploy(self, amount: u64) -> Result<Self> {
        require!(
            self.idle_balance >= amount,
            AsyncVaultError::InsufficientLiquidity
        );
        Ok(Self {
            position_amount: self
                .position_amount
                .checked_add(amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
            idle_balance: self
                .idle_balance
                .checked_sub(amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
            deployed_balance: self
                .deployed_balance
                .checked_add(amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
        })
    }

    pub fn pull(self, amount: u64) -> Result<Self> {
        require!(
            self.position_amount >= amount && self.deployed_balance >= amount,
            AsyncVaultError::InsufficientPositionBalance
        );
        Ok(Self {
            position_amount: self
                .position_amount
                .checked_sub(amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
            idle_balance: self
                .idle_balance
                .checked_add(amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
            deployed_balance: self
                .deployed_balance
                .checked_sub(amount)
                .ok_or(AsyncVaultError::ArithmeticError)?,
        })
    }
}

/// Optional tranche-mode configuration for a vault.
#[account]
#[derive(InitSpace)]
pub struct TrancheConfig {
    pub vault: Pubkey,
    pub senior_share_mint: Pubkey,
    pub junior_share_mint: Pubkey,
    /// Per-senior-share NAV placeholder; waterfall updates are future work.
    pub senior_nav: u128,
    /// Per-junior-share NAV placeholder; waterfall updates are future work.
    pub junior_nav: u128,
    /// Cached senior supply, including senior shares committed by approved but unclaimed deposits.
    pub senior_supply: u64,
    /// Cached junior supply, refreshed by waterfall and junior redemption approvals.
    pub junior_supply: u64,
    /// Annualized senior target return in basis points.
    pub senior_target_bps: u16,
    /// Minimum junior buffer ratio in basis points.
    pub min_junior_ratio_bps: u16,
    /// Request minimums in order: senior deposit, senior redeem, junior deposit, junior redeem.
    pub min_request_amounts: [u64; TRANCHE_REQUEST_LIMIT_COUNT],
    /// Request maximums in order: senior deposit, senior redeem, junior deposit, junior redeem.
    pub max_request_amounts: [u64; TRANCHE_REQUEST_LIMIT_COUNT],
    /// Total queued senior deposit requests ever created.
    pub senior_subscription_request_total: u64,
    /// Last senior deposit request processed or skipped.
    pub senior_subscription_request_last_processed: u64,
    /// Total queued senior redeem requests ever created.
    pub senior_redemption_request_total: u64,
    /// Last senior redeem request processed or skipped.
    pub senior_redemption_request_last_processed: u64,
    /// Total queued junior deposit requests ever created.
    pub junior_subscription_request_total: u64,
    /// Last junior deposit request processed or skipped.
    pub junior_subscription_request_last_processed: u64,
    /// Total queued junior redeem requests ever created.
    pub junior_redemption_request_total: u64,
    /// Last junior redeem request processed or skipped.
    pub junior_redemption_request_last_processed: u64,
    /// Last slot where waterfall accounting ran; zero until implemented.
    pub last_waterfall_slot: u64,
    /// Last timestamp where waterfall accounting ran; zero until implemented.
    pub last_waterfall_timestamp: i64,
    pub bump: u8,
}

impl TrancheConfig {
    pub fn next_queue_request_id(
        &mut self,
        selected_share_mint: Pubkey,
        request_type: RequestType,
    ) -> Result<u64> {
        let total = if selected_share_mint == self.senior_share_mint {
            if matches!(request_type, RequestType::Deposit) {
                &mut self.senior_subscription_request_total
            } else {
                &mut self.senior_redemption_request_total
            }
        } else if selected_share_mint == self.junior_share_mint {
            if matches!(request_type, RequestType::Deposit) {
                &mut self.junior_subscription_request_total
            } else {
                &mut self.junior_redemption_request_total
            }
        } else {
            return Err(AsyncVaultError::InvalidShareMint.into());
        };

        *total = total.wrapping_add(1);
        Ok(*total)
    }

    pub fn check_and_advance_queue(
        &mut self,
        selected_share_mint: Pubkey,
        request_type: RequestType,
        request_id: u64,
    ) -> Result<()> {
        let last_processed = if selected_share_mint == self.senior_share_mint {
            if matches!(request_type, RequestType::Deposit) {
                &mut self.senior_subscription_request_last_processed
            } else {
                &mut self.senior_redemption_request_last_processed
            }
        } else if selected_share_mint == self.junior_share_mint {
            if matches!(request_type, RequestType::Deposit) {
                &mut self.junior_subscription_request_last_processed
            } else {
                &mut self.junior_redemption_request_last_processed
            }
        } else {
            return Err(AsyncVaultError::InvalidShareMint.into());
        };

        let expected = last_processed.wrapping_add(1);
        let out_of_order_error = if matches!(request_type, RequestType::Deposit) {
            AsyncVaultError::SubscriptionQueueOutOfOrder
        } else {
            AsyncVaultError::RedemptionQueueOutOfOrder
        };
        if request_id != expected {
            return Err(out_of_order_error.into());
        }
        *last_processed = request_id;
        Ok(())
    }
}

impl Vault {
    pub fn is_primary_asset(&self, asset_mint: Pubkey) -> bool {
        self.asset_mint == asset_mint
    }

    pub fn assert_unpaused_and_initialized(&self) -> Result<()> {
        require!(self.initialized, AsyncVaultError::UninitializedVault);
        require!(!self.paused, AsyncVaultError::PausedVault);
        Ok(())
    }

    pub fn assert_uninitialized(&self) -> Result<()> {
        require!(!self.initialized, AsyncVaultError::VaultAlreadyInitialized);
        Ok(())
    }

    pub fn assert_curator(&self, signer: Pubkey) -> Result<()> {
        require_keys_eq!(signer, self.curator, AsyncVaultError::UnauthorizedSigner);
        Ok(())
    }

    pub fn assert_curator_or_fulfiller(&self, signer: Pubkey) -> Result<()> {
        require!(
            signer == self.curator || signer == self.fulfiller,
            AsyncVaultError::UnauthorizedSigner
        );
        Ok(())
    }

    pub fn assert_breaker(&self, signer: Pubkey) -> Result<()> {
        require_keys_eq!(signer, self.breaker, AsyncVaultError::UnauthorizedSigner);
        Ok(())
    }

    pub fn assert_fresh_nav_for_request(&self, request_nav_version: u64) -> Result<()> {
        if self.require_fresh_nav {
            require!(
                self.nav_version > request_nav_version,
                AsyncVaultError::StaleNav
            );
        }
        Ok(())
    }

    pub fn assert_nav_not_stale(&self, current_slot: u64) -> Result<()> {
        if self.max_nav_staleness_slots == 0 {
            return Ok(());
        }
        let elapsed_slots = current_slot
            .checked_sub(self.last_nav_update_slot)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        require!(
            elapsed_slots <= self.max_nav_staleness_slots,
            AsyncVaultError::StaleNav
        );
        Ok(())
    }

    pub fn assert_deposit_cap_allows(&self, additional_amount: u64) -> Result<()> {
        if self.deposit_cap == 0 {
            return Ok(());
        }
        let reserved_total = self
            .total_asset_balance
            .checked_add(self.pending_deposit_amount)
            .ok_or(AsyncVaultError::ArithmeticError)?
            .checked_add(additional_amount)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        require!(
            reserved_total <= self.deposit_cap,
            AsyncVaultError::DepositCapExceeded
        );
        Ok(())
    }

    pub fn validate_config(&self) -> Result<()> {
        let max_bps = vault_common::MAX_BPS;
        require!(
            self.max_nav_delta_bps <= max_bps
                && self.performance_fee_bps <= max_bps
                && self.protocol_fee_bps <= max_bps
                && self.instant_redemption_fee_bps <= max_bps,
            AsyncVaultError::FeeBpsExceeded
        );
        require!(
            self.nav_mode == NavMode::AuthoritySigned,
            AsyncVaultError::UnsupportedPhaseConfig
        );
        require!(
            self.tranche_config.is_none() || self.performance_fee_bps == 0,
            AsyncVaultError::UnsupportedPhaseConfig
        );
        if self.protocol_fee_bps > 0 {
            require!(
                self.protocol_fee_recipient != Pubkey::default(),
                AsyncVaultError::InvalidFeeRecipient
            );
        }
        require!(
            self.rolling_limit_window_slots > 0
                || (self.manager_rolling_limit == 0
                    && self.external_withdraw_rolling_limit == 0
                    && self.redemption_rolling_limit == 0),
            AsyncVaultError::InvalidRollingLimitConfig
        );
        Ok(())
    }

    pub fn consume_manager_rolling_limit(&mut self, amount: u64, current_slot: u64) -> Result<()> {
        Self::consume_rolling_limit(
            amount,
            current_slot,
            self.manager_rolling_limit,
            self.rolling_limit_window_slots,
            &mut self.manager_window_start_slot,
            &mut self.manager_window_amount,
        )
    }

    pub fn consume_external_withdraw_rolling_limit(
        &mut self,
        amount: u64,
        current_slot: u64,
    ) -> Result<()> {
        Self::consume_rolling_limit(
            amount,
            current_slot,
            self.external_withdraw_rolling_limit,
            self.rolling_limit_window_slots,
            &mut self.external_withdraw_window_start_slot,
            &mut self.external_withdraw_window_amount,
        )
    }

    pub fn consume_redemption_rolling_limit(
        &mut self,
        amount: u64,
        current_slot: u64,
    ) -> Result<()> {
        Self::consume_rolling_limit(
            amount,
            current_slot,
            self.redemption_rolling_limit,
            self.rolling_limit_window_slots,
            &mut self.redemption_window_start_slot,
            &mut self.redemption_window_amount,
        )
    }

    fn consume_rolling_limit(
        amount: u64,
        current_slot: u64,
        limit: u64,
        window_slots: u64,
        window_start_slot: &mut u64,
        window_amount: &mut u64,
    ) -> Result<()> {
        if limit == 0 {
            return Ok(());
        }
        require!(window_slots > 0, AsyncVaultError::InvalidRollingLimitConfig);

        let window_end = window_start_slot
            .checked_add(window_slots)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let window_uninitialized = *window_start_slot == 0 && *window_amount == 0;
        if window_uninitialized || current_slot >= window_end {
            *window_start_slot = current_slot;
            *window_amount = 0;
        }

        let next_amount = window_amount
            .checked_add(amount)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        require!(next_amount <= limit, AsyncVaultError::RollingLimitExceeded);
        *window_amount = next_amount;
        Ok(())
    }

    pub fn validate_nav_update(&self, updated_nav: u128, now: i64) -> Result<()> {
        self.validate_nav_delta(updated_nav)?;
        self.validate_implied_apy(updated_nav, now)?;
        Ok(())
    }

    fn validate_nav_delta(&self, updated_nav: u128) -> Result<()> {
        if self.max_nav_delta_bps == 0 || self.nav == 0 {
            return Ok(());
        }
        let delta = updated_nav.abs_diff(self.nav);
        let delta_bps = delta
            .checked_mul(u128::from(vault_common::MAX_BPS))
            .ok_or(AsyncVaultError::ArithmeticError)?
            .checked_div(self.nav)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        require!(
            delta_bps <= u128::from(self.max_nav_delta_bps),
            AsyncVaultError::NavDeltaExceeded
        );
        Ok(())
    }

    fn validate_implied_apy(&self, updated_nav: u128, now: i64) -> Result<()> {
        if self.max_implied_apy_bps == 0
            || self.nav == 0
            || updated_nav <= self.nav
            || self.last_nav_update_timestamp == 0
        {
            return Ok(());
        }
        let elapsed_seconds = now
            .checked_sub(self.last_nav_update_timestamp)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        if elapsed_seconds <= 0 {
            return Ok(());
        }
        let gain = updated_nav
            .checked_sub(self.nav)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let gain_bps = gain
            .checked_mul(u128::from(vault_common::MAX_BPS))
            .ok_or(AsyncVaultError::ArithmeticError)?
            .checked_div(self.nav)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        let annualized_bps = gain_bps
            .checked_mul(SECONDS_PER_YEAR)
            .ok_or(AsyncVaultError::ArithmeticError)?
            .checked_div(elapsed_seconds as u128)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        require!(
            annualized_bps <= u128::from(self.max_implied_apy_bps),
            AsyncVaultError::NavApyExceeded
        );
        Ok(())
    }
}

impl VaultAsset {
    pub fn assert_matches(&self, vault: Pubkey, asset_mint: Pubkey) -> Result<()> {
        require_keys_eq!(self.vault, vault, AsyncVaultError::InvalidVault);
        require_keys_eq!(
            self.asset_mint,
            asset_mint,
            AsyncVaultError::InvalidAssetMint
        );
        Ok(())
    }

    pub fn assert_pending_vault(&self, pending_vault: Pubkey) -> Result<()> {
        require_keys_eq!(
            self.pending_vault,
            pending_vault,
            AsyncVaultError::InvalidPendingVault
        );
        Ok(())
    }

    pub fn assert_reserve(&self, reserve: Pubkey) -> Result<()> {
        require_keys_eq!(self.reserve, reserve, AsyncVaultError::InvalidVault);
        Ok(())
    }

    pub fn assert_deposit_cap_allows(&self, additional_amount: u64) -> Result<()> {
        if self.deposit_cap == 0 {
            return Ok(());
        }
        let reserved_total = self
            .idle_balance
            .checked_add(self.pending_deposit_amount)
            .ok_or(AsyncVaultError::ArithmeticError)?
            .checked_add(additional_amount)
            .ok_or(AsyncVaultError::ArithmeticError)?;
        require!(
            reserved_total <= self.deposit_cap,
            AsyncVaultError::DepositCapExceeded
        );
        Ok(())
    }
}

impl VenueEntry {
    pub fn assert_active(&self) -> Result<()> {
        require!(!self.paused, AsyncVaultError::VenuePaused);
        Ok(())
    }

    pub fn assert_authority(&self, signer: Pubkey) -> Result<()> {
        require_keys_eq!(
            signer,
            self.registry_authority,
            AsyncVaultError::UnauthorizedSigner
        );
        Ok(())
    }
}

impl VaultVenue {
    pub fn assert_active(&self) -> Result<()> {
        require!(!self.paused, AsyncVaultError::VenuePaused);
        Ok(())
    }

    pub fn assert_manager_authorized(&self, vault: &Vault, signer: Pubkey) -> Result<()> {
        require!(
            signer == vault.manager || (signer == vault.hot_manager && self.routine_safe),
            AsyncVaultError::UnauthorizedSigner
        );
        Ok(())
    }

    pub fn assert_empty(&self) -> Result<()> {
        require!(
            self.position_count == 0,
            AsyncVaultError::VenuePositionNonZero
        );
        Ok(())
    }
}

impl Position {
    pub fn assert_empty(&self) -> Result<()> {
        require!(self.amount == 0, AsyncVaultError::PositionBalanceNonZero);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const MAX_TEST_LIMIT: u64 = 1_000_000;
    const MAX_TEST_WINDOW_SLOTS: u64 = 2_000;
    const MAX_TEST_SLOT_DELTA: u64 = 2_000;
    const MAX_TEST_LEDGER_BALANCE: u64 = 1_000_000;

    proptest! {
        #[test]
        fn rolling_limit_matches_epoch_window_model(
            limit in 1u64..=MAX_TEST_LIMIT,
            window_slots in 1u64..=MAX_TEST_WINDOW_SLOTS,
            operations in prop::collection::vec(
                (0u64..=MAX_TEST_SLOT_DELTA, 0u64..=MAX_TEST_LIMIT),
                1..64,
            ),
        ) {
            let mut current_slot = 0u64;
            let mut window_start_slot = 0u64;
            let mut window_amount = 0u64;
            let mut model_start_slot = 0u64;
            let mut model_amount = 0u64;

            for (slot_delta, amount) in operations {
                current_slot = current_slot
                    .checked_add(slot_delta)
                    .expect("bounded slot deltas should not overflow");

                let previous_window_start_slot = window_start_slot;
                let previous_window_amount = window_amount;
                let previous_model_start_slot = model_start_slot;
                let previous_model_amount = model_amount;

                let model_window_end = model_start_slot
                    .checked_add(window_slots)
                    .expect("bounded model window should not overflow");
                let model_uninitialized = model_start_slot == 0 && model_amount == 0;
                if model_uninitialized || current_slot >= model_window_end {
                    model_start_slot = current_slot;
                    model_amount = 0;
                }

                let model_next_amount = model_amount
                    .checked_add(amount)
                    .expect("bounded model amount should not overflow");
                let result = Vault::consume_rolling_limit(
                    amount,
                    current_slot,
                    limit,
                    window_slots,
                    &mut window_start_slot,
                    &mut window_amount,
                );

                if model_next_amount <= limit {
                    prop_assert!(result.is_ok());
                    model_amount = model_next_amount;
                    prop_assert_eq!(window_start_slot, model_start_slot);
                    prop_assert_eq!(window_amount, model_amount);
                } else {
                    prop_assert!(result.is_err());
                    window_start_slot = previous_window_start_slot;
                    window_amount = previous_window_amount;
                    model_start_slot = previous_model_start_slot;
                    model_amount = previous_model_amount;
                }
            }
        }

        #[test]
        fn zero_rolling_limit_is_disabled_and_does_not_mutate_state(
            amount in any::<u64>(),
            current_slot in any::<u64>(),
            mut window_start_slot in any::<u64>(),
            mut window_amount in any::<u64>(),
        ) {
            let original_window_start_slot = window_start_slot;
            let original_window_amount = window_amount;

            Vault::consume_rolling_limit(
                amount,
                current_slot,
                0,
                0,
                &mut window_start_slot,
                &mut window_amount,
            )
            .expect("zero limit should disable rolling-limit enforcement");

            prop_assert_eq!(window_start_slot, original_window_start_slot);
            prop_assert_eq!(window_amount, original_window_amount);
        }

        #[test]
        fn secondary_position_ledger_deploy_pull_conserves_asset_balances(
            position_amount in 0u64..=MAX_TEST_LEDGER_BALANCE,
            idle_balance in 0u64..=MAX_TEST_LEDGER_BALANCE,
            deployed_balance in 0u64..=MAX_TEST_LEDGER_BALANCE,
            deploy_amount in 0u64..=MAX_TEST_LEDGER_BALANCE,
            pull_seed in 0u64..=MAX_TEST_LEDGER_BALANCE,
        ) {
            prop_assume!(deploy_amount <= idle_balance);
            let starting = PositionLedger::new(position_amount, idle_balance, deployed_balance);
            let starting_assets = idle_balance
                .checked_add(deployed_balance)
                .expect("bounded ledger balances should not overflow");

            let after_deploy = starting
                .deploy(deploy_amount)
                .expect("deploy amount within idle balance should succeed");
            prop_assert_eq!(
                after_deploy.position_amount,
                position_amount
                    .checked_add(deploy_amount)
                    .expect("bounded position balance should not overflow")
            );
            prop_assert_eq!(after_deploy.idle_balance, idle_balance - deploy_amount);
            prop_assert_eq!(
                after_deploy.deployed_balance,
                deployed_balance
                    .checked_add(deploy_amount)
                    .expect("bounded deployed balance should not overflow")
            );
            prop_assert_eq!(
                after_deploy.idle_balance + after_deploy.deployed_balance,
                starting_assets
            );

            let max_pull = after_deploy
                .position_amount
                .min(after_deploy.deployed_balance);
            let pull_amount = if max_pull == 0 {
                0
            } else {
                pull_seed % (max_pull + 1)
            };
            let after_pull = after_deploy
                .pull(pull_amount)
                .expect("pull amount within position and deployed balances should succeed");
            prop_assert_eq!(after_pull.position_amount, after_deploy.position_amount - pull_amount);
            prop_assert_eq!(
                after_pull.idle_balance,
                after_deploy
                    .idle_balance
                    .checked_add(pull_amount)
                    .expect("bounded idle balance should not overflow")
            );
            prop_assert_eq!(after_pull.deployed_balance, after_deploy.deployed_balance - pull_amount);
            prop_assert_eq!(
                after_pull.idle_balance + after_pull.deployed_balance,
                starting_assets
            );
        }

        #[test]
        fn secondary_position_ledger_rejects_overdeploy_and_overpull(
            position_amount in 0u64..=MAX_TEST_LEDGER_BALANCE,
            idle_balance in 0u64..=MAX_TEST_LEDGER_BALANCE,
            deployed_balance in 0u64..=MAX_TEST_LEDGER_BALANCE,
            over_deploy_extra in 1u64..=MAX_TEST_LEDGER_BALANCE,
            over_pull_extra in 1u64..=MAX_TEST_LEDGER_BALANCE,
        ) {
            let starting = PositionLedger::new(position_amount, idle_balance, deployed_balance);
            let over_deploy = idle_balance
                .checked_add(over_deploy_extra)
                .expect("bounded overdeploy amount should not overflow");
            prop_assert!(starting.deploy(over_deploy).is_err());
            prop_assert_eq!(
                starting,
                PositionLedger::new(position_amount, idle_balance, deployed_balance)
            );

            let max_valid_pull = position_amount.min(deployed_balance);
            let over_pull = max_valid_pull
                .checked_add(over_pull_extra)
                .expect("bounded overpull amount should not overflow");
            prop_assert!(starting.pull(over_pull).is_err());
            prop_assert_eq!(
                starting,
                PositionLedger::new(position_amount, idle_balance, deployed_balance)
            );
        }
    }
}
