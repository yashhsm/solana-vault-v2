#![allow(ambiguous_glob_reexports)]
#![allow(clippy::diverging_sub_expression)]

use anchor_lang::prelude::*;

pub mod error;
pub mod extensions;
pub mod instructions;
pub mod state;
pub mod utils;

use extensions::*;
use instructions::*;

declare_id!("3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg");

#[program]
pub mod async_vault_v2 {
    use super::*;

    /* Vault Authority instructions */

    /// Creates a new async vault with reserve and pending token accounts,
    /// transfers share mint authority to the vault PDA, and initializes
    /// the vault config in a paused + uninitialized state.
    pub fn create_vault(ctx: Context<CreateVault>, args: AsyncVaultArgs) -> Result<()> {
        instructions::create_vault::handler(ctx, args)
    }

    /// Finalizes vault setup by marking it as initialized. Once initialized,
    /// no new extensions can be added. Requires authority signature.
    pub fn initialize_vault(ctx: Context<InitializeVault>) -> Result<()> {
        instructions::initialize_vault::handler(ctx)
    }

    /// Enables optional tranche mode by recording senior/junior share mints
    /// before vault initialization. Waterfall and tranche-scoped request paths
    /// are added by later Phase 4 slices.
    pub fn initialize_tranches(
        ctx: Context<InitializeTranches>,
        args: InitializeTranchesArgs,
    ) -> Result<()> {
        instructions::initialize_tranches::handler(ctx, args)
    }

    /// User claims their shares or assets from an approved Deposit or Redemption request.
    /// Request must be Claimable.
    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        instructions::claim::handler(ctx)
    }

    /* Vault Authority instructions */

    /// Stores a pending authority on the vault without transferring control.
    /// The new authority must later call `accept_authority_invitation` to
    /// complete the transfer. Requires current authority signature.
    pub fn invite_new_authority(
        ctx: Context<InviteNewAuthority>,
        args: InviteNewAuthorityArgs,
    ) -> Result<()> {
        instructions::invite_new_authority::handler(ctx, args)
    }

    /// Completes the authority transfer by setting the vault authority to
    /// the pending authority. Requires both the current and new authority
    /// to sign.
    pub fn accept_authority_invitation(ctx: Context<AcceptAuthorityInvitation>) -> Result<()> {
        instructions::accept_authority_invitation::handler(ctx)
    }
    /// Updates the async vault configuration. Can modify paused status
    /// and the fee_recipient. Requires authority signature.
    pub fn update_vault(ctx: Context<UpdateVault>, args: UpdateVaultArgs) -> Result<()> {
        instructions::update_vault::handler(ctx, args)
    }

    /// Queues a timelocked vault configuration update.
    pub fn queue_vault_update(ctx: Context<QueueVaultUpdate>, args: UpdateVaultArgs) -> Result<()> {
        instructions::queue_vault_update::handler(ctx, args)
    }

    /// Executes a queued vault configuration update after its eta slot.
    pub fn execute_vault_update(ctx: Context<ExecuteVaultUpdate>) -> Result<()> {
        instructions::execute_vault_update::handler(ctx)
    }

    /// Cancels a queued vault configuration update. Requires current curator.
    pub fn cancel_vault_update(ctx: Context<CancelVaultUpdate>) -> Result<()> {
        instructions::cancel_vault_update::handler(ctx)
    }

    /// Queues a timelocked deposit/withdrawal fee update.
    pub fn queue_fee_update(ctx: Context<QueueFeeUpdate>, args: FeeUpdateArgs) -> Result<()> {
        instructions::queue_fee_update::handler(ctx, args)
    }

    /// Executes a queued fee update after its eta slot.
    pub fn execute_fee_update(ctx: Context<ExecuteFeeUpdate>) -> Result<()> {
        instructions::execute_fee_update::handler(ctx)
    }

    /// Cancels a queued fee update. Requires current curator.
    pub fn cancel_fee_update(ctx: Context<CancelFeeUpdate>) -> Result<()> {
        instructions::cancel_fee_update::handler(ctx)
    }

    /// Initializes the singleton program-level protocol fee recipient config.
    pub fn initialize_protocol_fee_config(
        ctx: Context<InitializeProtocolFeeConfig>,
        args: InitializeProtocolFeeConfigArgs,
    ) -> Result<()> {
        instructions::initialize_protocol_fee_config::handler(ctx, args)
    }

    /// Updates the singleton program-level protocol fee recipient config.
    pub fn update_protocol_fee_config(
        ctx: Context<UpdateProtocolFeeConfig>,
        args: UpdateProtocolFeeConfigArgs,
    ) -> Result<()> {
        instructions::update_protocol_fee_config::handler(ctx, args)
    }

    /// Queues a timelocked mutable non-fee TLV extension update.
    pub fn queue_extension_update(
        ctx: Context<QueueExtensionUpdate>,
        args: ExtensionUpdateArgs,
    ) -> Result<()> {
        instructions::queue_extension_update::handler(ctx, args)
    }

    /// Executes a queued non-fee TLV extension update after its eta slot.
    pub fn execute_extension_update(ctx: Context<ExecuteExtensionUpdate>) -> Result<()> {
        instructions::execute_extension_update::handler(ctx)
    }

    /// Cancels a queued non-fee TLV extension update. Requires current curator.
    pub fn cancel_extension_update(ctx: Context<CancelExtensionUpdate>) -> Result<()> {
        instructions::cancel_extension_update::handler(ctx)
    }

    /// Adds an approved asset mint and its reserve/pending token accounts.
    pub fn add_vault_asset(ctx: Context<AddVaultAsset>, args: AddVaultAssetArgs) -> Result<()> {
        instructions::add_vault_asset::handler(ctx, args)
    }

    /// Removes an approved asset after all balances for that asset are zero.
    pub fn remove_vault_asset(ctx: Context<RemoveVaultAsset>) -> Result<()> {
        instructions::remove_vault_asset::handler(ctx)
    }

    /// Registers a venue entry under a registry-authority namespace.
    pub fn register_venue(ctx: Context<RegisterVenue>, args: RegisterVenueArgs) -> Result<()> {
        instructions::register_venue::handler(ctx, args)
    }

    /// Pauses or unpauses a venue entry. Requires the registry authority.
    pub fn set_venue_entry_paused(
        ctx: Context<SetVenueEntryPaused>,
        args: SetVenueEntryPausedArgs,
    ) -> Result<()> {
        instructions::set_venue_entry_paused::handler(ctx, args)
    }

    /// Approves a registered venue for a specific vault.
    pub fn approve_vault_venue(ctx: Context<ApproveVaultVenue>) -> Result<()> {
        instructions::approve_vault_venue::handler(ctx)
    }

    /// Removes a zero-position venue approval from a vault.
    pub fn remove_vault_venue(ctx: Context<RemoveVaultVenue>) -> Result<()> {
        instructions::remove_vault_venue::handler(ctx)
    }

    /// Creates a primary-asset position token account for an approved venue.
    pub fn create_venue_position(ctx: Context<CreateVenuePosition>) -> Result<()> {
        instructions::create_venue_position::handler(ctx)
    }

    /// Deploys primary assets from the vault reserve into a vault-owned venue position.
    pub fn deploy_venue_position(ctx: Context<DeployVenuePosition>, amount: u64) -> Result<()> {
        instructions::deploy_venue_position::handler(ctx, amount)
    }

    /// Pulls primary assets from a vault-owned venue position back to the vault reserve.
    pub fn pull_venue_position(ctx: Context<PullVenuePosition>, amount: u64) -> Result<()> {
        instructions::pull_venue_position::handler(ctx, amount)
    }

    /// Removes a zero-balance primary-asset venue position.
    pub fn remove_venue_position(ctx: Context<RemoveVenuePosition>) -> Result<()> {
        instructions::remove_venue_position::handler(ctx)
    }

    /// Pauses the vault using the breaker role. Breaker cannot unpause.
    pub fn pause_vault(ctx: Context<PauseVault>) -> Result<()> {
        instructions::pause_vault::handler(ctx)
    }

    /// Updates the vault nav and increases nav version by 1
    /// Requires authority signature.
    pub fn update_vault_nav<'info>(
        ctx: Context<'info, UpdateVaultNav<'info>>,
        updated_nav: u128,
    ) -> Result<()> {
        instructions::update_nav::handler(ctx, updated_nav)
    }

    /// Approve a pending request, allowing the User to execute the Claim instruction.
    /// This sets the Request's claimable NAV to the Vault's current NAV.
    pub fn approve_request<'info>(
        ctx: Context<'info, ApproveRequest<'info>>,
        args: ApproveRequestArgs,
    ) -> Result<()> {
        instructions::approve_request::handler(ctx, args)
    }

    /// Reject a pending request. For deposit requests, the deposited assets are
    /// refunded to the user. For redeem requests, the shares are minted back to the user.
    /// The request account is closed and its rent is returned to the user.
    pub fn reject_request(ctx: Context<RejectRequest>, args: RejectRequestArgs) -> Result<()> {
        instructions::reject_request::handler(ctx, args)
    }

    /* EXTENSION INSTRUCTIONS */

    /// Adds a deposit fee TLV extension to the vault. Must be called
    /// before vault initialization. Requires authority signature.
    pub fn initialize_deposit_fee(
        ctx: Context<InitDepositFee>,
        args: InitDepositFeeArgs,
    ) -> Result<()> {
        extensions::fee::instructions::initialize_deposit_fee::handler(ctx, args)
    }

    /// Adds a withdrawal fee TLV extension to the vault. Must be called
    /// before vault initialization. Requires authority signature.
    pub fn initialize_withdrawal_fee(
        ctx: Context<InitWithdrawalFee>,
        args: InitWithdrawalFeeArgs,
    ) -> Result<()> {
        extensions::fee::instructions::initialize_withdrawal_fee::handler(ctx, args)
    }

    /// Enables free-form externally managed withdrawals for this vault.
    /// Must be initialized before the vault is finalized.
    pub fn initialize_externally_managed_withdrawals(
        ctx: Context<InitializeExternallyManagedWithdrawals>,
    ) -> Result<()> {
        extensions::externally_managed_withdrawals::instructions::initialize_externally_managed_withdrawals::handler(ctx)
    }

    /// Enables primary-asset instant deposit/redeem for this vault.
    /// Must be initialized before the vault is finalized.
    pub fn initialize_instant_settlement(
        ctx: Context<InitializeInstantSettlement>,
        args: InitializeInstantSettlementArgs,
    ) -> Result<()> {
        extensions::instant_settlement::instructions::initialize_instant_settlement::handler(
            ctx, args,
        )
    }

    /// Updates an existing deposit fee extension. The fee must have been
    /// previously initialized. Requires authority signature.
    pub fn update_deposit_fee(
        ctx: Context<BasicExtensionAccounts>,
        args: UpdateDepositFeeArgs,
    ) -> Result<()> {
        extensions::fee::instructions::update_deposit_fee::handler(ctx, args)
    }

    /// Updates an existing withdrawal fee extension. The fee must have been
    /// previously initialized. Requires authority signature.
    pub fn update_withdrawal_fee(
        ctx: Context<BasicExtensionAccounts>,
        args: UpdateWithdrawalFeeArgs,
    ) -> Result<()> {
        extensions::fee::instructions::update_withdrawal_fee::handler(ctx, args)
    }

    /// Adds a PausableSubscriptions TLV extension to the vault. Must be called
    /// before vault initialization. Requires authority signature.
    pub fn initialize_pausable_subscriptions(
        ctx: Context<InitPausableSubscriptions>,
        args: InitPausableSubscriptionsArgs,
    ) -> Result<()> {
        extensions::pausable_subscriptions::instructions::initialize_pausable_subscriptions::handler(
            ctx, args,
        )
    }

    /// Updates the paused state of an existing PausableSubscriptions extension.
    /// When paused is true, new deposit requests are rejected. Requires authority signature.
    pub fn update_pausable_subscriptions(
        ctx: Context<BasicExtensionAccounts>,
        args: UpdatePausableSubscriptionsArgs,
    ) -> Result<()> {
        extensions::pausable_subscriptions::instructions::update_pausable_subscriptions::handler(
            ctx, args,
        )
    }

    /// Adds a PausableRedemptions TLV extension to the vault. Must be called
    /// before vault initialization. Requires authority signature.
    pub fn initialize_pausable_redemptions(
        ctx: Context<InitPausableRedemptions>,
        args: InitPausableRedemptionsArgs,
    ) -> Result<()> {
        extensions::pausable_redemptions::instructions::initialize_pausable_redemptions::handler(
            ctx, args,
        )
    }

    /// Updates the paused state of an existing PausableRedemptions extension.
    /// When paused is true, new redeem requests are rejected. Requires authority signature.
    pub fn update_pausable_redemptions(
        ctx: Context<BasicExtensionAccounts>,
        args: UpdatePausableRedemptionsArgs,
    ) -> Result<()> {
        extensions::pausable_redemptions::instructions::update_pausable_redemptions::handler(
            ctx, args,
        )
    }

    /// Adds MinSubscription extension to the vault. Must be called before vault
    /// initialization. When active, deposit requests below the threshold are rejected.
    /// Requires authority signature.
    pub fn initialize_min_subscription(
        ctx: Context<InitMinSubscription>,
        args: InitMinSubscriptionArgs,
    ) -> Result<()> {
        extensions::min_subscription::instructions::initialize_min_subscription::handler(ctx, args)
    }

    /// Updates the threshold of an existing MinSubscription extension.
    /// Requires authority signature.
    pub fn update_min_subscription(
        ctx: Context<BasicExtensionAccounts>,
        args: UpdateMinSubscriptionArgs,
    ) -> Result<()> {
        extensions::min_subscription::instructions::update_min_subscription::handler(ctx, args)
    }

    /// Adds MinRedemption extension to the vault. Must be called before vault
    /// initialization. When active, redemption requests below the threshold are rejected.
    /// Requires authority signature.
    pub fn initialize_min_redemption(
        ctx: Context<InitMinRedemption>,
        args: InitMinRedemptionArgs,
    ) -> Result<()> {
        extensions::min_redemption::instructions::initialize_min_redemption::handler(ctx, args)
    }

    /// Updates the threshold of an existing MinRedemption extension.
    /// Requires authority signature.
    pub fn update_min_redemption(
        ctx: Context<BasicExtensionAccounts>,
        args: UpdateMinRedemptionArgs,
    ) -> Result<()> {
        extensions::min_redemption::instructions::update_min_redemption::handler(ctx, args)
    }

    /// Adds a SubscriptionQueue TLV extension to the vault, enabling FIFO ordering
    /// for deposit requests. Must be called before vault initialization. Requires authority
    /// signature.
    pub fn initialize_subscription_queue(ctx: Context<InitializeSubscriptionQueue>) -> Result<()> {
        extensions::subscription_queue::instructions::initialize_subscription_queue::handler(ctx)
    }

    /// Cancels a pending queued deposit request. Assets are refunded immediately. The request
    /// account remains open as a tombstone so the subscription queue can advance past it via
    /// `skip_canceled_queue_request`. Only valid for vaults with SubscriptionQueue active.
    pub fn cancel_queued_deposit_request(ctx: Context<CancelQueuedDepositRequest>) -> Result<()> {
        extensions::subscription_queue::instructions::cancel_queued_deposit_request::handler(ctx)
    }

    /// Adds a RedemptionQueue TLV extension to the vault, enabling FIFO ordering
    /// for redeem requests. Must be called before vault initialization. Requires authority
    /// signature.
    pub fn initialize_redemption_queue(ctx: Context<InitializeRedemptionQueue>) -> Result<()> {
        extensions::redemption_queue::instructions::initialize_redemption_queue::handler(ctx)
    }

    /// Cancels a pending queued redeem request. Shares are minted back to the user immediately.
    /// The request account remains open as a tombstone so the redemption queue can advance past
    /// it via `skip_canceled_queue_request`. Only valid for vaults with RedemptionQueue
    /// active.
    pub fn cancel_queued_redemption_request(
        ctx: Context<CancelQueuedRedemptionRequest>,
    ) -> Result<()> {
        extensions::redemption_queue::instructions::cancel_queued_redemption_request::handler(ctx)
    }

    /// Permissionless instruction that advances the queue past a canceled request, closes the
    /// request account, and returns rent to the original owner. Works for both subscription and
    /// redemption queues; queue type is inferred from the request's type. Must be called in
    /// ascending request ID order for consecutive tombstones.
    pub fn skip_canceled_queue_request(ctx: Context<SkipCanceledQueueRequest>) -> Result<()> {
        extensions::fifo_queues::skip_canceled_queue_request::handler(ctx)
    }

    /* USER INSTRUCTIONS */

    /// Creates a deposit request with state pending (Pending vault authority acceptance)
    pub fn create_deposit_request(
        ctx: Context<CreateDepositRequest>,
        args: RequestArgs,
    ) -> Result<()> {
        instructions::create_deposit_request::handler(ctx, args)
    }

    /// Creates a redeem request with state pending (Pending vault authority acceptance)
    pub fn create_redeem_request(
        ctx: Context<CreateRedeemRequest>,
        args: RequestArgs,
    ) -> Result<()> {
        instructions::create_redeem_request::handler(ctx, args)
    }

    /// Instantly deposits primary assets and mints shares when the opt-in extension is enabled.
    pub fn instant_deposit<'info>(
        ctx: Context<'info, InstantDeposit<'info>>,
        amount: u64,
    ) -> Result<()> {
        instructions::instant_deposit::handler(ctx, amount)
    }

    /// Instantly burns shares and redeems primary assets when the opt-in extension is enabled.
    pub fn instant_redeem<'info>(
        ctx: Context<'info, InstantRedeem<'info>>,
        shares: u64,
    ) -> Result<()> {
        instructions::instant_redeem::handler(ctx, shares)
    }

    /// Cancels a pending request. For deposits, refunds the full amount
    /// back to the user. For redemptions, mints the shares back.
    pub fn cancel_request(ctx: Context<CancelRequest>) -> Result<()> {
        instructions::cancel_request::handler(ctx)
    }

    /// Withdraws assets from the vault reserve to a specified token account.
    /// Used for async operations such as deploying assets offchain.
    /// Requires authority signature.
    pub fn withdraw_assets(ctx: Context<WithdrawAssets>, amount: u64) -> Result<()> {
        instructions::withdraw_assets::handler(ctx, amount)
    }

    /// Sets an operator for the Request.
    /// Requires Request owner signature.
    pub fn set_operator(ctx: Context<SetOperator>) -> Result<()> {
        instructions::set_operator::handler(ctx)
    }
}
