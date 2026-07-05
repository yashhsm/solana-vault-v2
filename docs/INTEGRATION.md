# Integration

This file maps the implemented read surface to accounts and fields. It does not
claim support for unimplemented Phase 2-6 requirements.

## Program Clients

- TypeScript package: `@sendai/solana-vault-v2`
- Rust crate: `async-vault-v2-client`
- IDL: `idl/async_vault_v2.json`

Regenerate after program changes:

```bash
anchor build --ignore-keys
pnpm run generate-clients
```

## Read Field Mapping

| Product field                  | Account / field                                                                                                                                 | Status                                                                                                      |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Vault address                  | `Vault` PDA from `[VAULT_CONFIG_SEED, share_mint]`                                                                                              | Implemented                                                                                                 |
| Asset mint                     | `Vault.asset_mint`                                                                                                                              | Implemented, single asset only                                                                              |
| Approved asset count           | `Vault.approved_asset_count`                                                                                                                    | Partial Phase 2 asset-PDA slice                                                                             |
| Share mint                     | `Vault.share_mint`                                                                                                                              | Implemented                                                                                                 |
| Reserve token account          | `Vault.vault_token_account`                                                                                                                     | Implemented                                                                                                 |
| Pending token account          | `Vault.pending_vault`                                                                                                                           | Implemented                                                                                                 |
| Curator                        | `Vault.curator` and legacy alias `Vault.authority`                                                                                              | Implemented                                                                                                 |
| Manager                        | `Vault.manager`                                                                                                                                 | Implemented for primary and approved-secondary position deploy/pull                                         |
| Hot manager                    | `Vault.hot_manager`                                                                                                                             | Implemented for routine-safe primary and approved-secondary positions                                       |
| Fulfiller                      | `Vault.fulfiller`                                                                                                                               | Implemented for NAV and fulfill/reject                                                                      |
| Breaker                        | `Vault.breaker`                                                                                                                                 | Implemented for pause only                                                                                  |
| Paused                         | `Vault.paused`                                                                                                                                  | Implemented                                                                                                 |
| Initialized                    | `Vault.initialized`                                                                                                                             | Implemented                                                                                                 |
| NAV                            | `Vault.nav`                                                                                                                                     | Implemented                                                                                                 |
| NAV version                    | `Vault.nav_version`                                                                                                                             | Implemented                                                                                                 |
| NAV mode                       | `Vault.nav_mode`                                                                                                                                | `AuthoritySigned` only; oracle modes rejected                                                               |
| Last NAV update slot           | `Vault.last_nav_update_slot`                                                                                                                    | Implemented                                                                                                 |
| Last NAV update timestamp      | `Vault.last_nav_update_timestamp`                                                                                                               | Implemented                                                                                                 |
| NAV freshness requirement      | `Vault.require_fresh_nav`                                                                                                                       | Implemented                                                                                                 |
| NAV bounds                     | `Vault.max_nav_delta_bps`, `Vault.max_implied_apy_bps`, `Vault.max_nav_staleness_slots`                                                         | Implemented for signed updates/settlement                                                                   |
| Total assets                   | `Vault.total_asset_balance`                                                                                                                     | Implemented as upstream virtual single-asset balance                                                        |
| Pending request count          | `Vault.pending_async_requests`                                                                                                                  | Implemented                                                                                                 |
| Deposit cap                    | `Vault.deposit_cap`                                                                                                                             | Implemented as aggregate single-asset cap                                                                   |
| Pending deposit reservation    | `Vault.pending_deposit_amount`                                                                                                                  | Implemented                                                                                                 |
| Deposit/withdrawal fees        | TLV fee extensions                                                                                                                              | Implemented                                                                                                 |
| Queue positions                | Subscription/redemption queue TLV extensions; tranche mode uses lane-local counters on `TrancheConfig`                                          | Implemented                                                                                                 |
| User share balance             | User share token account amount                                                                                                                 | Implemented by SPL Token account                                                                            |
| Pending request                | `Request` account fields                                                                                                                        | Implemented                                                                                                 |
| Claimable amount               | `Request.amount` when `Request.request_state == Claimable`                                                                                      | Implemented                                                                                                 |
| Secondary approved asset       | `VaultAsset` PDA from `[ASSET_CONFIG_SEED, vault, asset_mint]`                                                                                  | Partial; add/remove plus secondary request unwind paths; approval disabled until pricing exists             |
| Per-asset reserve/pending      | `VaultAsset.reserve`, `VaultAsset.pending_vault`                                                                                                | Partial; used by secondary request creation/unwind paths                                                    |
| Per-asset idle/deployed        | `VaultAsset.idle_balance`, `VaultAsset.deployed_balance`                                                                                        | Partial; constrained venue deploy/pull update the ledger for pre-funded secondary balances                  |
| Per-asset deposit cap          | `VaultAsset.deposit_cap`, `VaultAsset.pending_deposit_amount`                                                                                   | Partial; enforced for secondary deposits                                                                    |
| Per-asset manager bucket       | `VaultAsset.manager_window_start_slot`, `VaultAsset.manager_window_amount`, with `Vault.manager_rolling_limit` and `rolling_limit_window_slots` | Implemented for approved-secondary venue deploy/pull                                                        |
| Externally managed withdrawals | `ExternallyManagedWithdrawals` vault TLV extension plus active `VenueEntry`/`VaultVenue` accounts                                               | Partial; gates `withdraw_assets` opt-in and venue approval; no CPI template validation                      |
| Instant settlement             | `InstantSettlement` vault TLV extension and `InstantSettlementUser` PDA                                                                         | Partial; primary asset, non-tranche only; requires nonzero staleness and instant fee guards                 |
| Venue registry entry           | `VenueEntry` PDA from `[VENUE_ENTRY_SEED, registry_authority, venue_id]`                                                                        | Partial; state only, no CPI execution                                                                       |
| Per-vault venue approval       | `VaultVenue` PDA from `[VAULT_VENUE_SEED, vault, venue_entry]`                                                                                  | Partial; approval state gates `withdraw_assets` and position stubs                                          |
| Venue positions                | `Position` PDA from `[POSITION_SEED, vault, venue_entry, asset_mint]`                                                                           | Partial; primary and approved-secondary SPL token-account stub only                                         |
| Tranche config                 | `TrancheConfig` PDA from `[TRANCHE_CONFIG_SEED, vault]`                                                                                         | Partial; dual-mint scaffold, waterfall, request binding, bounds, junior-ratio floor, FIFO counters          |
| Per-tranche NAV/supply         | `Vault.tranche_config`, `TrancheConfig.senior_*`, `TrancheConfig.junior_*`                                                                      | Partial; waterfall on NAV update, request pricing, and approval guards                                      |
| Per-tranche queue counters     | `TrancheConfig.{senior,junior}_{subscription,redemption}_request_{total,last_processed}`                                                        | Implemented when queue TLVs are enabled in tranche mode                                                     |
| Rolling limit remaining        | `Vault.manager_window_*`, `Vault.external_withdraw_window_*`, `Vault.redemption_window_*`; secondary manager buckets live on `VaultAsset`       | Implemented for primary manager moves, secondary manager moves, external withdrawals, and gross redemptions |
| Timelocked vault updates       | `PendingVaultUpdate`, `Vault.timelock_delay_slots`                                                                                              | Implemented for vault config updates                                                                        |
| Timelocked fee changes         | `PendingFeeUpdate`, `Vault.timelock_delay_slots`                                                                                                | Implemented for deposit/withdrawal fee updates                                                              |
| Other timelocked TLV changes   | `PendingExtensionUpdate`, `Vault.timelock_delay_slots`                                                                                          | Implemented for min subscription, min redemption, and pausable subscription/redemption updates              |
| Performance fee                | `Vault.performance_fee_bps`, `Vault.high_water_mark`, `performance_fee_crystallization_interval_seconds`, `last_fee_crystallization_timestamp`  | Implemented for single-tranche NAV updates; rejected for tranche vaults                                     |
| Instant redemption fee         | `Vault.instant_redemption_fee_bps`                                                                                                              | Implemented for primary instant redeems only                                                                |
| Protocol fee                   | `Vault.protocol_fee_bps`, `Vault.protocol_fee_recipient`, singleton `ProtocolFeeConfig`                                                         | Partial; vault-level bps with optional program-level recipient routing for implemented fee sources          |

## Request Lifecycle

1. Create a vault with `create_vault`, initialize optional TLV extensions, then
   call `initialize_vault`. Vaults that need free-form external withdrawals
   must call `initialize_externally_managed_withdrawals` before initialization
   and later supply an active approved venue to `withdraw_assets`. Vaults that
   need tranche-mode scaffolding must call `initialize_tranches` before
   initialization. Vaults that need primary instant settlement must call
   `initialize_instant_settlement` before initialization.
2. Update NAV with `update_nav`. Tranche-enabled vaults must pass remaining
   accounts in this order after any performance-fee accounts: writable
   `TrancheConfig`, senior share mint, junior share mint.
3. Create deposit or redeem requests. Primary requests use `Vault.asset_mint`;
   secondary deposits and secondary async redemptions use an approved
   `VaultAsset` and pass its PDA as the optional `vault_asset` account at
   request creation. Tranche-enabled request lifecycle
   instructions pass the selected base/senior/junior share mint as `share_mint`
   and must pass `TrancheConfig` as the first remaining account.
4. If `require_fresh_nav` is true, update NAV again after request creation.
5. Curator or fulfiller approves/rejects the request. For tranche-enabled
   approvals, `TrancheConfig` comes first. If `min_junior_ratio_bps` is nonzero
   and the approval is a senior deposit or junior redemption, pass senior and
   junior mint accounts after `TrancheConfig`; fee-recipient accounts follow
   those tranche accounts when fees are owed. When a protocol fee is owed and
   `ProtocolFeeConfig` should override `Vault.protocol_fee_recipient`, pass the
   singleton PDA immediately before the protocol-fee token account.
6. User or operator claims/cancels according to upstream lifecycle rules.

## Instant Settlement

Instant settlement is a Phase 5 partial implementation for primary-asset,
non-tranche vaults:

1. After `create_vault` and before `initialize_vault`, the curator first sets a
   nonzero `max_nav_staleness_slots`, then calls `initialize_instant_settlement`
   with `0 < instant_redemption_fee_bps <= MAX_BPS`. The initializer rejects
   tranche-enabled vaults, missing safety guards, and invalid threshold pairs
   where a nonzero max is below its min. Optional per-user deposit/redeem limits
   use `Vault.rolling_limit_window_slots`; nonzero per-user limits require a
   nonzero window at execution time.
2. After initialization, the vault must have a nonzero NAV. `instant_deposit`
   and `instant_redeem` use the current `Vault.nav` and honor
   `max_nav_staleness_slots`.
3. `instant_deposit` accepts only the primary asset and base share mint. It
   first enforces optional min/max gross deposit bounds, requires and
   initializes or validates the user's `InstantSettlementUser` PDA only when a
   nonzero per-user deposit limit is configured, consumes any configured
   per-user gross-deposit window, applies any deposit fee extension, splits that
   fee between the fee recipient and protocol fee recipient when
   `protocol_fee_bps` is nonzero, optionally using `ProtocolFeeConfig` from
   `remaining_accounts` for recipient routing, moves net assets directly into the reserve,
   mints shares from current NAV, and increments
   `Vault.total_asset_balance` by the net deposit.
4. `instant_redeem` accepts only the base share mint, checks reserve liquidity,
   first enforces optional min/max share bounds, requires and initializes or
   validates the user's `InstantSettlementUser` PDA only when a nonzero per-user
   redeem limit is configured, consumes any configured per-user redeem-share
   window, applies withdrawal plus instant-redemption fees, splits those fees
   between the fee recipient and protocol fee recipient when
   `protocol_fee_bps` is nonzero, optionally using `ProtocolFeeConfig` from
   `remaining_accounts` for recipient routing, consumes the gross redemption rolling limit,
   burns shares, transfers net assets to the user, and decrements
   `Vault.total_asset_balance` by the gross redeemed assets.

Secondary assets, tranche-aware instant settlement, oracle-priced instant
settlement, and request-queue integration remain future work.

## Asset Administration

Secondary asset approval is an admin-only Phase 2 slice:

1. Curator calls `add_vault_asset` with a new mint and the derived
   `VaultAsset`, reserve, and pending token-account PDAs.
2. The instruction validates the asset mint extensions and increments
   `Vault.approved_asset_count`.
3. Curator can call `remove_vault_asset` only after all stored per-asset balances
   and both token accounts are zero.

Secondary deposit and redeem requests are asset-scoped through
`Request.asset_mint_address`, but `approve_request` rejects secondary assets
until USD-normalized per-asset pricing exists. Cancel/reject paths remain
available so escrowed secondary deposits can refund assets and secondary redeem
requests can restore burned shares. Constrained venue deploy/pull can move
pre-funded approved-secondary assets between `VaultAsset.idle_balance` and
`VaultAsset.deployed_balance`; USD-normalized NAV aggregation is not implemented
yet.

## Venue Administration

Venue registry state is a Phase 3 scaffold:

1. A registry authority calls `register_venue` with a fixed `venue_id`, target
   program, bounded list of allowed instruction discriminators, venue type, risk
   class, and routine-safe flag.
2. The registry authority can pause or unpause the `VenueEntry` with
   `set_venue_entry_paused`.
3. A vault curator can approve an active venue with `approve_vault_venue`, which
   creates a `VaultVenue` PDA keyed by `(vault, venue_entry)`.
4. Externally managed `withdraw_assets` calls must include an active
   `VenueEntry` and matching active `VaultVenue`, in addition to the TLV opt-in.
5. A curator can create a primary-asset or approved-secondary-asset `Position`
   and vault-owned position token account for an approved venue. Secondary
   positions require the matching `VaultAsset` PDA.
6. Manager can deploy/pull primary assets between the vault reserve and the
   position token account subject to the vault-level `manager_rolling_limit`
   when configured. For secondary assets, the same configured limit/window is
   consumed from the matching `VaultAsset` manager bucket instead of the
   vault-level primary bucket. Hot manager can do the same only when the
   approval is routine-safe.
7. A curator can remove a zero-balance `Position`, then remove the `VaultVenue`
   only while its position count is zero.

The registry state is not a generic execution boundary yet. No instruction can
execute third-party venue CPI, validate venue account templates, custody
arbitrary external protocol positions, or prevent vault-in-vault cycles.

## Tranche Administration

Tranche config is a Phase 4 partial implementation:

1. After `create_vault` and before `initialize_vault`, the curator calls
   `initialize_tranches` with senior and junior share mints.
2. One tranche mint must be the existing `Vault.share_mint`; the other mint must
   be a zero-supply SPL Token or Token-2022 mint controlled by the provided mint
   authority.
3. The instruction creates `TrancheConfig` keyed by `(vault)`, stores
   `Vault.tranche_config`, records bps config and zeroed NAV/supply fields, and
   ensures both tranche mints are vault-owned.
4. Every tranche-enabled `update_vault_nav` must pass the writable
   `TrancheConfig` plus senior and junior mint accounts. The program initializes
   tranche NAVs on the first supplied update, credits subsequent gains to senior
   up to the annualized `senior_target_bps`, credits residual gains to junior,
   and applies losses to junior before senior.
5. Tranche-enabled deposit/redeem request lifecycle instructions require
   `TrancheConfig` as the first remaining account, store the selected share mint
   on `Request.share_mint_address`, and mint/burn/restore/claim against that
   selected tranche mint. Approval pricing uses the selected tranche NAV when it
   is nonzero and falls back to aggregate vault NAV for zero-supply bootstrap
   tranches.
6. `create_deposit_request` and `create_redeem_request` enforce the selected
   tranche/request-type min/max amount bounds from `TrancheConfig`; zero values
   disable a bound.
7. When the subscription or redemption queue TLV extension is enabled on a
   tranche vault, request creation assigns queue IDs from senior/junior
   lane-local counters on `TrancheConfig`, and approve/reject/skip-canceled
   paths advance only the matching lane.
8. When `min_junior_ratio_bps` is nonzero, `approve_request` rejects senior
   deposits and junior redemptions that would push junior value below the
   configured ratio of senior plus junior value. Approval and NAV-update paths
   preserve committed tranche supply so approved-but-unclaimed shares are
   counted by later guarded approvals.

Tranche-aware performance fees are not implemented yet. `initialize_tranches`
and vault config updates reject combining `Vault.tranche_config` with
`performance_fee_bps > 0`; single-tranche performance fee shares are split
between `fee_recipient` and `protocol_fee_recipient` when `protocol_fee_bps` is
nonzero. To route through the singleton config, pass performance-fee remaining
accounts as share mint, fee-recipient share account, `ProtocolFeeConfig`,
protocol-fee share account, then share token program.

## Safety Notes

- A strict V2 vault defaults to fresh-NAV settlement. Test helpers disable this
  only to preserve upstream parity tests.
- `Oracle` and `Hybrid` NAV modes are not usable yet because no oracle account
  validation is implemented and the program rejects them.
- `withdraw_assets` is disabled by default and requires the
  `ExternallyManagedWithdrawals` TLV extension plus an active approved venue.
  It is still a curator-controlled free-form pull, not a validated venue CPI
  boundary.
- `VenueEntry` and `VaultVenue` accounts are approval metadata only. Do not treat
  them as proof that a downstream CPI action is validated or safe.
- `Position` currently represents a vault-owned SPL token-account stub for the
  primary asset or an approved secondary asset. It is useful for local
  ledger/deploy/pull integration, not for external protocol custody.
- `TrancheConfig` waterfall accounting runs only when tranche-enabled NAV
  updates include the required remaining accounts. Tranche request binding and
  mint/burn settlement exist for the async lifecycle, guarded approvals enforce
  the junior ratio floor, request creation enforces per-direction tranche
  min/max amount bounds, and optional FIFO queues use senior/junior lane-local
  counters. Tranche-aware fees are still absent.
- `InstantSettlement` is disabled by default and supports only primary-asset,
  non-tranche vaults with nonzero staleness and instant-redemption-fee guards,
  creation-time per-transaction min/max bounds, and optional per-user rolling
  limits. It is not a substitute for secondary-asset instant settlement,
  tranche-aware settlement, or oracle-priced NAV verification.
- Secondary approved assets can be used for request creation/unwind paths and
  the constrained venue-position deploy/pull stub, but secondary approvals are
  disabled until per-asset pricing exists. Treat `VaultAsset` as partial
  multi-asset custody, not complete multi-asset NAV or instant-settlement
  support.
