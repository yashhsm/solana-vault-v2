use anchor_lang::prelude::*;

#[error_code]
pub enum AsyncVaultError {
    #[msg("Initial price cannot be zero")]
    InvalidInitialPrice,
    #[msg("Unauthorized signer")]
    UnauthorizedSigner,
    #[msg("Vault is not initialized")]
    UninitializedVault,
    #[msg("Vault is paused")]
    PausedVault,
    #[msg("Vault is already initialized")]
    VaultAlreadyInitialized,
    #[msg("Extension is already initialized")]
    ExtensionAlreadyInitialized,
    #[msg("Extension is not initialized")]
    UninitializedExtension,
    #[msg("Invalid extension data")]
    InvalidExtensionData,
    #[msg("Fee basis points exceed maximum")]
    FeeBpsExceeded,
    #[msg("Arithmetic error")]
    ArithmeticError,
    #[msg("Mints should be different.")]
    MintsShouldBeDifferent,
    #[msg("Share mint supply should be zero.")]
    ShareMintSupplyShouldBeZero,
    #[msg("No pending authority invitation")]
    NoPendingAuthority,
    #[msg("Pending Vault is not valid.")]
    InvalidPendingVault,
    #[msg("Pending Shares Vault is not valid.")]
    InvalidPendingSharesVault,
    #[msg("Fee recipient account must be provided as a remaining account when fee > 0.")]
    MissingFeeRecipient,
    #[msg("Fee recipient account is invalid.")]
    InvalidFeeRecipient,
    #[msg("Request current state is not valid.")]
    RequestInvalidState,
    #[msg("Request is not pending.")]
    RequestIsNotPending,
    #[msg("Invalid request type for this instruction.")]
    InvalidRequestType,
    #[msg("A required optional account was not provided.")]
    MissingRequiredAccount,
    #[msg("Asset mint has invalid extensions.")]
    InvalidAssetMintExtensions,
    #[msg("Asset mint is not valid.")]
    InvalidAssetMint,
    #[msg("Share mint is not valid.")]
    InvalidShareMint,
    #[msg("Request address is not valid.")]
    InvalidRequest,
    #[msg("Request is not in a Pending state.")]
    RequestNotPending,
    #[msg("Request is not in a Claimable state.")]
    RequestNotClaimable,
    #[msg("Subscriptions are paused")]
    SubscriptionsPaused,
    #[msg("Redemptions are paused")]
    RedemptionsPaused,
    #[msg("Deposit request is not next in the subscription queue")]
    SubscriptionQueueOutOfOrder,
    #[msg("Request is not in a Canceled state.")]
    RequestIsNotCanceled,
    #[msg("Queued deposit requests must be canceled via cancel_queued_deposit_request.")]
    MustUseCancelQueuedDepositRequest,
    #[msg("Redeem request is not next in the redemption queue")]
    RedemptionQueueOutOfOrder,
    #[msg("Queued redeem requests must be canceled via cancel_queued_redemption_request.")]
    MustUseCancelQueuedRedemptionRequest,
    #[msg("Deposit amount is below the minimum subscription threshold")]
    SubscriptionAmountBelowMinimum,
    #[msg("Redemption amount is below the minimum redemption threshold")]
    RedemptionAmountBelowMinimum,
    #[msg("Nav is not set.")]
    NavIsNotSet,
    #[msg("Redeem shares amount too small.")]
    InsufficientRedeemAmount,
    #[msg("Approval does not match the live request instance.")]
    ApprovalRequestMismatch,
    #[msg("Deposit amount too small.")]
    InsufficientDepositAmount,
    #[msg("Share mint has invalid extensions.")]
    InvalidShareMintExtensions,
    #[msg("NAV is stale for this instruction.")]
    StaleNav,
    #[msg("NAV delta exceeds configured bounds.")]
    NavDeltaExceeded,
    #[msg("Implied APY exceeds configured bounds.")]
    NavApyExceeded,
    #[msg("Deposit cap exceeded.")]
    DepositCapExceeded,
    #[msg("Breaker can only pause the vault.")]
    BreakerCanOnlyPause,
    #[msg("Configuration is reserved for a later V2 phase and is not enforced yet.")]
    UnsupportedPhaseConfig,
    #[msg("Rolling limit exceeded.")]
    RollingLimitExceeded,
    #[msg("Invalid rolling limit configuration.")]
    InvalidRollingLimitConfig,
    #[msg("A timelock queue is required for this configuration change.")]
    TimelockRequired,
    #[msg("Vault timelock is not configured.")]
    TimelockNotConfigured,
    #[msg("Timelock has not reached its eta slot.")]
    TimelockNotReady,
    #[msg("Invalid timelock change.")]
    InvalidTimelockChange,
    #[msg("Queued timelock authority no longer matches the vault curator.")]
    StaleTimelockAuthority,
    #[msg("Asset is already approved for this vault.")]
    AssetAlreadyApproved,
    #[msg("Maximum approved asset count exceeded.")]
    MaxApprovedAssetsExceeded,
    #[msg("Asset balances must be zero before removal.")]
    AssetBalanceNonZero,
    #[msg("Invalid vault account.")]
    InvalidVault,
    #[msg("Externally managed withdrawals are not enabled for this vault.")]
    ExternallyManagedWithdrawalsDisabled,
    #[msg("Venue discriminator count is invalid.")]
    InvalidVenueDiscriminatorCount,
    #[msg("Venue entry is paused.")]
    VenuePaused,
    #[msg("Venue positions must be zero before removal.")]
    VenuePositionNonZero,
    #[msg("Invalid venue entry.")]
    InvalidVenueEntry,
    #[msg("Invalid position account.")]
    InvalidPosition,
    #[msg("Position balances must be zero before removal.")]
    PositionBalanceNonZero,
    #[msg("Position balance is insufficient.")]
    InsufficientPositionBalance,
    #[msg("Post-transfer token balance delta did not match the requested amount.")]
    InvalidBalanceDelta,
    #[msg("Instant settlement is not enabled for this vault.")]
    InstantSettlementDisabled,
    #[msg("Vault reserve liquidity is insufficient for this redemption.")]
    InsufficientLiquidity,
    #[msg("Junior tranche ratio would fall below the configured minimum.")]
    JuniorRatioBelowMinimum,
    #[msg("Invalid tranche request limit configuration.")]
    InvalidTrancheRequestLimitConfig,
    #[msg("Tranche request amount is below the configured minimum.")]
    TrancheRequestAmountBelowMinimum,
    #[msg("Tranche request amount is above the configured maximum.")]
    TrancheRequestAmountAboveMaximum,
    #[msg("Invalid instant settlement threshold configuration.")]
    InvalidInstantSettlementThresholdConfig,
    #[msg("Instant deposit amount is below the configured minimum.")]
    InstantDepositAmountBelowMinimum,
    #[msg("Instant deposit amount is above the configured maximum.")]
    InstantDepositAmountAboveMaximum,
    #[msg("Instant redeem shares are below the configured minimum.")]
    InstantRedeemSharesBelowMinimum,
    #[msg("Instant redeem shares are above the configured maximum.")]
    InstantRedeemSharesAboveMaximum,
}

impl From<vault_common::VaultMathError> for AsyncVaultError {
    fn from(err: vault_common::VaultMathError) -> Self {
        match err {
            vault_common::VaultMathError::ArithmeticError => AsyncVaultError::ArithmeticError,
            vault_common::VaultMathError::FeeBpsLimitReached => AsyncVaultError::FeeBpsExceeded,
        }
    }
}
