# Accounts

This document records the account and PDA surface for the implemented V2 subset.
Anything listed as scaffolding is stored in state but not fully enforced.

## Program

- Program name: `async_vault_v2`
- Program ID: `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg`
- IDL: `idl/async_vault_v2.json`

## Vault

- Account type: `Vault`
- Seeds: `[VAULT_CONFIG_SEED, share_mint]`
- Bump field: `vault.bump`
- Owner: `async_vault_v2`
- Purpose: core vault configuration, role keys, NAV configuration, lifecycle
  counters, virtual asset balance, fee config, and V2 risk-control fields.

Implemented V2 fields:

- `curator`: config, unpause, role management, legacy authority alias.
- `manager`: may deploy/pull primary and approved-secondary position stubs for
  approved venues.
- `hot_manager`: may deploy/pull only routine-safe primary and
  approved-secondary position stubs.
- `fulfiller`: may update NAV and approve/reject async requests.
- `breaker`: may only pause through `pause_vault`.
- `nav_mode`: only `AuthoritySigned` is accepted until oracle validation exists.
- `require_fresh_nav`: when true, settlement requires a NAV update after request
  creation.
- `max_nav_delta_bps`, `max_implied_apy_bps`, `max_nav_staleness_slots`: optional
  NAV guards.
- `deposit_cap`, `pending_deposit_amount`: aggregate deposit-cap accounting.

Implemented rolling-limit fields:

- `rolling_limit_window_slots`: shared epoch-window length for implemented
  rolling limits.
- `manager_rolling_limit`, `manager_window_start_slot`,
  `manager_window_amount`: vault-level manager bucket for primary-asset
  deploy/pull.
- `external_withdraw_rolling_limit`, `external_withdraw_window_start_slot`,
  `external_withdraw_window_amount`.
- `redemption_rolling_limit`, `redemption_window_start_slot`,
  `redemption_window_amount`.

Implemented timelock field:

- `timelock_delay_slots`: when nonzero, timelocked vault config fields must be
  queued through `PendingVaultUpdate`; deposit/withdrawal fee updates must be
  queued through `PendingFeeUpdate`; mutable non-fee TLV extension values must
  be queued through `PendingExtensionUpdate`; and strategy policy roots must be
  queued through `PendingStrategyPolicyUpdate`.

Implemented fee fields:

- `performance_fee_bps`: single-tranche performance fee charged in newly minted
  shares on NAV updates above the high-water mark. Rejected when
  `tranche_config` is set.
- `performance_fee_crystallization_interval_seconds`: minimum elapsed seconds
  between performance-fee crystallizations. Zero preserves immediate
  crystallization on each new high-water mark.
- `protocol_fee_bps`: vault-level skim, in basis points, taken from already
  computed deposit, withdrawal, instant-redemption, and single-tranche
  performance fees before the remaining fee is paid to `fee_recipient`.
- `protocol_fee_recipient`: owner required for protocol-fee token accounts.
  Initialized to `fee_recipient` and mutable through the vault config update
  path. Fee-paying instructions use this as the fallback recipient when no
  `ProtocolFeeConfig` PDA is supplied.
- `instant_redemption_fee_bps`: charged in assets on primary instant redeems
  when the `InstantSettlement` TLV extension is enabled.
- `tranche_config`: optional `TrancheConfig` PDA. When set, `update_vault_nav`
  requires tranche remaining accounts and applies waterfall accounting.
- `high_water_mark`, `last_fee_crystallization_timestamp`: performance-fee
  tracking fields updated by `update_vault_nav`.
- `approved_asset_count`: number of approved assets, including the primary
  `asset_mint`.

## Protocol Fee Config

- Account type: `ProtocolFeeConfig`
- Seeds: `[PROTOCOL_FEE_CONFIG_SEED]`
- Bump field: `protocol_fee_config.bump`
- Owner: `async_vault_v2`
- Purpose: ABI-compatible singleton program-level protocol fee recipient routing.

Fields:

- `authority`: governance signer allowed to queue changes and transfers.
- `protocol_fee_recipient`: owner required for protocol-fee token accounts when
  this PDA is supplied. The default public key means the global override is
  disabled and fee paths fall back to `Vault.protocol_fee_recipient`.

## Protocol Fee Governance

- Account type: `ProtocolFeeGovernance`
- Seeds: `[PROTOCOL_FEE_GOVERNANCE_SEED]`
- Bump field: `protocol_fee_governance.bump`
- Owner: `async_vault_v2`
- Purpose: sidecar controls for the ABI-stable `ProtocolFeeConfig` account.

Fields:

- `protocol_fee_config`: binds the sidecar to the singleton config PDA.
- `breaker`: signer allowed to disable the global override immediately.
- `timelock_delay_slots`: nonzero delay for config and authority changes.
- `paused`: reports whether the global override is disabled.
- `version`: increments on executed updates, pauses, and accepted authority
  transfers; queued operations bind to the version observed at creation.

Implemented instructions:

- `initialize_protocol_fee_config_v2`: requires the deployed program's current
  upgrade authority, creates or securely adopts the config, creates the
  governance sidecar, and starts paused with no global override. Adoption
  overwrites legacy authority/recipient state fail-closed. It must run before
  making the program immutable if global routing will be used.
- `queue_protocol_fee_config_update` / `execute_protocol_fee_config_update` /
  `cancel_protocol_fee_config_update`: version-bound delayed activation or
  rotation of the recipient and optional future delay.
- `pause_protocol_fee_config`: current authority or breaker may immediately set
  the recipient to the default key, disable the override, and invalidate queued
  operations by incrementing the governance version.
- `queue_protocol_fee_authority_transfer` /
  `accept_protocol_fee_authority_transfer` /
  `cancel_protocol_fee_authority_transfer`: delayed, two-step governance
  authority transfer that requires the proposed successor's signature.
- Legacy `initialize_protocol_fee_config` and `update_protocol_fee_config`
  retain their discriminators for compatibility but always fail; they cannot
  bypass bootstrap authorization or the timelock.

Fee-paying instructions preserve the vault-level fallback by default. When a
protocol fee is owed, callers may place the singleton PDA immediately before the
protocol-fee token account in `remaining_accounts`; approval, instant redeem,
instant deposit, and performance-fee minting then validate the protocol-fee
token account against `ProtocolFeeConfig.protocol_fee_recipient` instead of
`Vault.protocol_fee_recipient`. A supplied but paused config is consumed in the
same account position and resolves to the vault-level recipient.

## Instant Settlement Extension

- Extension type: `InstantSettlement`
- Stored in the vault TLV region.
- Purpose: creation-time opt-in for primary-asset, non-tranche instant deposit
  and redeem instructions.

Fields:

- `min_deposit_amount`: minimum gross asset amount accepted by
  `instant_deposit`. Zero disables the lower bound.
- `max_deposit_amount`: maximum gross asset amount accepted by
  `instant_deposit`. Zero disables the upper bound.
- `min_redeem_shares`: minimum share amount accepted by `instant_redeem`. Zero
  disables the lower bound.
- `max_redeem_shares`: maximum share amount accepted by `instant_redeem`. Zero
  disables the upper bound.
- `max_user_deposit_amount`: maximum gross instant-deposit assets one user can
  deposit in the current `rolling_limit_window_slots` epoch. Zero disables the
  per-user deposit bound.
- `max_user_redeem_shares`: maximum instant-redeem shares one user can redeem in
  the current `rolling_limit_window_slots` epoch. Zero disables the per-user
  redeem bound.
- `enabled`: currently initialized to `1`.

## Instant Settlement User

- Account type: `InstantSettlementUser`
- Seeds: `[INSTANT_USER_LIMIT_SEED, vault, user]`
- Bump field: `instant_user.bump`
- Owner: `async_vault_v2`
- Purpose: per-user epoch buckets for primary instant-settlement rolling limits.

Important fields:

- `vault`, `user`: bucket identity.
- `deposit_window_start_slot`, `deposit_window_amount`: gross instant-deposit
  amount consumed in the user's current epoch.
- `redeem_window_start_slot`, `redeem_window_amount`: instant-redeem shares
  consumed in the user's current epoch.

Implemented instructions:

- `instant_deposit`: when `max_user_deposit_amount` is nonzero, requires the
  PDA, initializes it on first limited use, validates the `(vault, user)`
  identity on later use, and consumes the limit before fee calculation or token
  movement.
- `instant_redeem`: when `max_user_redeem_shares` is nonzero, requires the same
  PDA, initializes or validates it, and consumes the limit before asset
  calculation, share burn, or token movement.
- Zero per-user limits leave the PDA optional and do not create it.

## Reserve Token Account

- Token account PDA seeds: `[RESERVE_CONFIG_SEED, share_mint]`
- Bump field: `vault.reserve_bump`
- Token owner: vault PDA
- Purpose: idle vault assets for the single approved `asset_mint`.

## Pending Vault Token Account

- Token account PDA seeds: `[PENDING_VAULT_SEED, share_mint]`
- Bump field: `vault.pending_vault_bump`
- Token owner: vault PDA
- Purpose: assets awaiting deposit approval or refund.

## Vault Asset

- Account type: `VaultAsset`
- Seeds: `[ASSET_CONFIG_SEED, vault, asset_mint]`
- Bump field: `vault_asset.bump`
- Owner: `async_vault_v2`
- Purpose: approved secondary asset record for the Phase 2 asset-PDA slice.

Important fields:

- `vault`, `asset_mint`: vault and approved mint identity.
- `reserve`: token account PDA from `[ASSET_RESERVE_SEED, vault, asset_mint]`.
- `pending_vault`: token account PDA from
  `[ASSET_PENDING_SEED, vault, asset_mint]`.
- `idle_balance`, `deployed_balance`, `pending_deposit_amount`: per-asset
  accounting fields. Secondary deposits update `pending_deposit_amount` at
  request creation and unwind through cancel/reject. Secondary redemption
  requests can unwind through cancel/reject and restore burned shares.
  Secondary approval is disabled until per-asset pricing exists; constrained
  secondary venue deploy/pull moves pre-funded balances between idle and
  deployed after exact token-account delta verification.
- `deposit_cap`: per-asset deposit cap for secondary deposit requests. Zero
  disables the cap.
- `manager_window_start_slot`, `manager_window_amount`: per-asset manager bucket
  for approved-secondary deploy/pull, using the vault's configured
  `manager_rolling_limit` and `rolling_limit_window_slots`.

Implemented instructions:

- `add_vault_asset`: curator-only, disabled when `timelock_delay_slots != 0`,
  validates the mint extensions, initializes the `VaultAsset` PDA plus its
  reserve and pending token accounts, and enforces `MAX_APPROVED_ASSETS = 8`
  including the primary asset.
- `remove_vault_asset`: curator-only, disabled when `timelock_delay_slots != 0`,
  requires all stored and token-account balances to be zero, closes the asset
  token accounts, and closes the `VaultAsset` account.
- Secondary async lifecycle: `create_deposit_request`, `create_redeem_request`,
  `cancel_request`, `reject_request`, and queued cancellation validate
  `Request.asset_mint_address`; `approve_request` validates the matching
  `VaultAsset` accounts and then rejects secondary assets until per-asset
  pricing is implemented.
- Secondary position lifecycle: `create_venue_position`,
  `deploy_venue_position`, `pull_venue_position`, and `remove_venue_position`
  validate the matching `VaultAsset` PDA and canonical reserve account before
  moving approved-secondary assets through the constrained position stub.

## Venue Entry

- Account type: `VenueEntry`
- Seeds: `[VENUE_ENTRY_SEED, registry_authority, venue_id]`
- Bump field: `venue_entry.bump`
- Owner: `async_vault_v2`
- Purpose: program-owned venue metadata registered by an explicit registry
  authority namespace.

Important fields:

- `registry_authority`: signer that created and can pause the entry.
- `venue_id`: fixed 32-byte caller-defined identifier.
- `target_program`: program ID the Merkle-verified venue CPI boundary may target.
- `allowed_discriminator_count`, `allowed_discriminators`: bounded list of
  active 8-byte instruction discriminators; unused bytes are zeroed.
- `venue_type`, `risk_class`, `routine_safe`: metadata for future policy and
  reader surfaces.
- `paused`: paused venues cannot be newly approved or used for managed CPI.

Implemented instructions:

- `register_venue`: creates the `VenueEntry` PDA and validates the discriminator
  count is between 1 and `MAX_VENUE_DISCRIMINATORS`.
- `set_venue_entry_paused`: registry-authority-only pause/unpause for the entry.

## Vault Venue

- Account type: `VaultVenue`
- Seeds: `[VAULT_VENUE_SEED, vault, venue_entry]`
- Bump field: `vault_venue.bump`
- Owner: `async_vault_v2`
- Purpose: curator approval linking one vault to one registered venue.

Important fields:

- `vault`, `venue_entry`: approval identity.
- `recipient_authority`: token-account owner approved to receive
  `withdraw_assets` transfers for this vault/venue approval.
- `target_program`, `routine_safe`: copied from `VenueEntry` at approval time.
- `position_count`: number of active positions for this venue approval; must be
  zero before removal.
- `approved_at_slot`: slot when the approval was created.

Implemented instructions:

- `approve_vault_venue`: curator-only, blocked when the vault timelock is active
  until a queued venue-approval flow exists, rejects paused venue entries, and
  stores a non-default recipient authority for externally managed withdrawals.
- `remove_vault_venue`: curator-only, blocked when timelock is active, and
  closes only zero-position approvals.

## Strategy Policy

- Account type: `StrategyPolicy`
- Seeds: `[STRATEGY_POLICY_SEED, vault, strategist]`
- Bump field: `strategy_policy.bump`
- Owner: `async_vault_v2`
- Purpose: optional capability root for vault-signed calls initiated by one
  manager or hot manager.

Important fields:

- `vault`, `strategist`: prevent cross-vault and cross-strategist proof replay.
- `merkle_root`, `version`: active commitment and monotonic policy version.
- `paused`: immediately blocks managed actions; curator updates are required to
  unpause.
- `executing`: transaction-scoped reentrancy guard around the external CPI.

Implemented instructions:

- `initialize_strategy_policy`: curator-only creation in a disabled state.
- `update_strategy_policy`: immediate curator root update only when the vault
  timelock is zero.
- `queue_strategy_policy_update`, `execute_strategy_policy_update`, and
  `cancel_strategy_policy_update`: version-bound root rotation through the
  existing vault timelock.
- `pause_strategy_policy`: immediate curator-or-breaker pause that increments
  the version to invalidate existing proofs and queued updates.
- `close_strategy_policy`: curator-only revocation and rent recovery.
- `manage_vault_with_merkle_verification`: manager/hot-manager execution for an
  active `VenueEntry`/`VaultVenue`. It reconstructs the canonical leaf from
  bounded instruction/account operators, verifies the proof, applies any
  selected amount to the manager rolling limit, rejects writable vault-owned
  token accounts, invokes the external program with the vault PDA signer, and
  requires share supply to remain unchanged.
- `manage_vault_with_token_balance_adapter`: manager/hot-manager typed custody
  movement for the canonical reserve and `Position` token account. A separate
  Merkle leaf binds the action, asset/account identities, venue records, policy
  version, token program, and per-call maximum. The instruction enforces a
  bounded dynamic amount, exact reserve/position deltas, share-supply
  invariance, and primary or secondary ledger reconciliation.

The full manifest and proofs remain off-chain. See
[`MERKLE_STRATEGY_POLICY.md`](MERKLE_STRATEGY_POLICY.md) for the byte encoding,
tree construction, trust model, and client flow.

## Position

- Account type: `Position`
- Seeds: `[POSITION_SEED, vault, venue_entry, asset_mint]`
- Bump field: `position.bump`
- Owner: `async_vault_v2`
- Purpose: deployed-position ledger for the Phase 3 SPL token-account venue
  stub.

Important fields:

- `vault`, `venue_entry`, `vault_venue`, `asset_mint`: position identity.
- `token_account`: vault-owned token-account PDA from
  `[POSITION_TOKEN_SEED, vault, venue_entry, asset_mint]`.
- `amount`: program ledger for deployed assets in the position.

Implemented instructions:

- `create_venue_position`: curator-only, accepts the primary asset or an
  approved secondary asset, creates the `Position` account and position token
  account, and increments `VaultVenue.position_count`.
- `deploy_venue_position`: manager-only, or hot-manager-only when the
  `VaultVenue` approval is routine-safe; transfers primary assets from the vault
  reserve or approved secondary assets from `VaultAsset.reserve` into the
  position token account and verifies exact post-transfer token-account deltas
  before increasing `Position.amount`. Primary movements consume the
  vault-level manager bucket; secondary movements consume the matching
  `VaultAsset` manager bucket.
- `pull_venue_position`: same manager/hot-manager authorization; transfers
  assets from the position token account back to the primary reserve or
  secondary reserve and verifies exact post-transfer token-account deltas before
  decreasing `Position.amount`.
- `manage_vault_with_token_balance_adapter`: provides the same canonical token
  movement through a strategist-bound Merkle capability. It additionally
  requires `Position.amount` to match the position token account before the
  call, proof-binds a per-call maximum, and reconciles secondary
  `VaultAsset.idle_balance`/`deployed_balance` after exact deltas.
- `remove_venue_position`: curator-only, blocked when timelock is active,
  requires stored and token balances to be zero, closes the position token
  account, closes the `Position`, and decrements `VaultVenue.position_count`.

## Tranche Config

- Account type: `TrancheConfig`
- Seeds: `[TRANCHE_CONFIG_SEED, vault]`
- Bump field: `tranche_config.bump`
- Owner: `async_vault_v2`
- Purpose: optional Phase 4 tranche-mode scaffold for one senior and one junior
  share mint against the vault's asset pool.

Important fields:

- `vault`: vault that owns this tranche config.
- `senior_share_mint`, `junior_share_mint`: tranche share mints. One of these
  must be the existing `Vault.share_mint`; the other is transferred to vault
  mint authority during initialization.
- `senior_target_bps`: annualized senior target return in basis points; gains
  credit senior up to this pro-rated target before residual gain goes to junior.
- `min_junior_ratio_bps`: junior-buffer floor enforced by guarded request
  approvals.
- `min_request_amounts`, `max_request_amounts`: request bounds for senior
  deposit, senior redeem, junior deposit, and junior redeem, in that order. A
  zero value disables that bound.
- `senior_nav`, `junior_nav`: tranche NAV accounting updated by
  `update_vault_nav`.
- `senior_supply`, `junior_supply`: effective supplies cached from tranche mint
  accounts during `update_vault_nav`; NAV updates preserve committed approved
  supply, and approvals update these caches for tranche deposits and
  redemptions.
- `{senior,junior}_{subscription,redemption}_request_total` and
  `{senior,junior}_{subscription,redemption}_request_last_processed`: lane-local
  FIFO counters used when the existing subscription/redemption queue TLV
  extensions are enabled on a tranche vault.
- `last_waterfall_slot`, `last_waterfall_timestamp`: waterfall bookkeeping.

Implemented instructions:

- `initialize_tranches`: curator-only, callable only before `initialize_vault`,
  validates bps bounds, rejects vaults with `performance_fee_bps > 0`, requires
  zero-supply tranche share mints, rejects asset mints as tranche share mints,
  requires one tranche mint to equal `Vault.share_mint`, validates share-mint
  extensions, ensures both tranche mints are vault-owned, and stores
  `Vault.tranche_config`.
- `update_vault_nav`: when `Vault.tranche_config` is set, requires remaining
  accounts `[TrancheConfig, senior_share_mint, junior_share_mint]` after any
  performance-fee accounts. It initializes tranche NAVs on the first waterfall
  update, credits senior gains up to the pro-rated target, credits residual
  gains to junior, and applies losses to junior before senior.
- `approve_request`: for tranche-enabled senior deposits and junior redemptions
  when `min_junior_ratio_bps` is nonzero, requires senior and junior mint
  accounts immediately after `TrancheConfig`, computes the post-approval junior
  ratio with checked `u128` math, and rejects approvals that would breach the
  configured floor.
- `create_deposit_request` and `create_redeem_request`: enforce the selected
  tranche/request-type min/max amount bounds after validating `TrancheConfig`.
- Tranche queue paths: when the existing subscription/redemption queue TLV is
  enabled, request creation assigns senior/junior lane-local IDs from
  `TrancheConfig`; approve/reject/skip-canceled paths advance only the matching
  lane while non-tranche vaults keep the original vault-level queue behavior.

## Request

- Account type: `Request`
- Created as a user-supplied keypair.
- Owner: `async_vault_v2`
- Purpose: one async deposit or redemption request.

Important fields:

- `owner`, `vault`, `request_type`, `amount`: identity checked at approval.
- `asset_mint_address`: primary asset mint or approved secondary asset mint for
  the request. Secondary deposits and redemptions use this to validate the
  `VaultAsset` PDA and per-asset reserve/pending accounts throughout the
  lifecycle.
- `share_mint_address`: selected share mint for the request. Normal vaults must
  use `Vault.share_mint`; tranche-enabled vaults may use either senior or junior
  share mint after validating the `TrancheConfig` remaining account.
- `created_at`: timestamp captured at request creation.
- `nav_update_version`: NAV version at request creation; V2 approval can require
  a newer `vault.nav_version`.
- `request_state`: `Pending`, `Claimable`, `Cancelled`, or `Rejected`.
- `operator`: optional delegated claimant. Pending cancellation remains
  owner-only.

## Pending Vault Update

- Account type: `PendingVaultUpdate`
- Created as a curator-supplied keypair by `queue_vault_update`.
- Owner: `async_vault_v2`
- Purpose: one queued vault config mutation with an `eta_slot`.

Important fields:

- `vault`: vault account the update applies to.
- `queued_by`: curator at queue time. Execution rejects the update if this key
  is no longer the current `Vault.curator`.
- `created_slot`, `eta_slot`: queue timing.
- `args`: serialized `UpdateVaultArgs` applied by `execute_vault_update` after
  the eta slot.

## Pending Fee Update

- Account type: `PendingFeeUpdate`
- Created as a curator-supplied keypair by `queue_fee_update`.
- Owner: `async_vault_v2`
- Purpose: one queued deposit or withdrawal fee TLV mutation with an `eta_slot`.

Important fields:

- `vault`: vault account the update applies to.
- `queued_by`: curator at queue time. Execution rejects the update if this key
  is no longer the current `Vault.curator`.
- `created_slot`, `eta_slot`: queue timing.
- `args`: `FeeUpdateArgs` containing `FeeUpdateKind::{Deposit,Withdrawal}` and
  the new `FeeType`.

## Pending Extension Update

- Account type: `PendingExtensionUpdate`
- Created as a curator-supplied keypair by `queue_extension_update`.
- Owner: `async_vault_v2`
- Purpose: one queued mutable non-fee TLV extension mutation with an `eta_slot`.

Important fields:

- `vault`: vault account the update applies to.
- `queued_by`: curator at queue time. Execution rejects the update if this key
  is no longer the current `Vault.curator`.
- `created_slot`, `eta_slot`: queue timing.
- `args`: `ExtensionUpdateArgs`, covering `MinSubscription`, `MinRedemption`,
  `PausableSubscriptions`, and `PausableRedemptions`.

## TLV Extensions

The upstream TLV extension model is retained. Vault extensions begin at byte
offset `672` for the new V2 vault layout. Request extensions begin at byte
offset `211` after `Request.share_mint_address`.

Implemented extensions:

- Deposit and withdrawal fee.
- Minimum subscription and redemption.
- Pausable subscriptions and redemptions.
- Subscription and redemption FIFO queues.
- Externally managed withdrawals. This is a creation-time opt-in gate for
  `withdraw_assets`; the instruction also requires an active `VenueEntry` and
  matching active `VaultVenue`, and the recipient token account must be owned by
  `VaultVenue.recipient_authority`. Without the TLV extension, active venue
  approval, or approved recipient owner, reserve withdrawals are rejected.
- Instant settlement. This is a creation-time opt-in gate for primary-asset,
  non-tranche `instant_deposit` and `instant_redeem`. Initialization and
  execution require nonzero `max_nav_staleness_slots` and nonzero
  `instant_redemption_fee_bps`, with optional per-transaction thresholds and
  per-user rolling limits.

## Not Yet Present

The following planned accounts do not exist yet: oracle adapter accounts,
arbitrary venue execution/account template accounts, vault-in-vault
cycle-prevention accounts, tranche-aware or secondary-asset instant-settlement
accounts, and richer protocol-fee governance accounts for global bps overrides
or authority transfer.
