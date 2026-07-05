# Solana Vault V2 Build Report

Date: 2026-07-04

## Summary

This pass delivered a renamed, buildable Solana Vault V2 fork and implemented a
conservative Phase 1 subset: role fields, curator/fulfiller settlement
permissions, breaker-only pause, strict fresh-NAV settlement by default, optional
NAV bounds/staleness controls, aggregate deposit-cap reservations,
external-withdrawal/redemption rolling-limit enforcement, and primary plus
secondary per-asset manager deploy/pull rolling buckets, plus vault-config and
deposit/withdrawal fee timelock queues, mutable non-fee TLV extension timelock
queues, single-tranche performance-fee/HWM crystallization on NAV updates with a
configurable crystallization interval, partial Phase 2 asset-PDA
plus secondary async lifecycle slices, and a Phase 3 venue subset: externally
managed withdrawal opt-in with active venue-approval gating, venue registry
state, per-vault venue approvals, and a primary/approved-secondary SPL
token-account position stub, plus Phase 4 tranche
scaffolding and waterfall accounting: a
pre-initialization `TrancheConfig` PDA for senior/junior share mints, a
`Vault.tranche_config` pointer, and senior-target/junior-first NAV allocation on
tranche-enabled NAV updates, plus tranche-scoped async request binding,
selected tranche share mint/burn settlement, per-direction tranche request
min/max bounds, junior-ratio floor enforcement for senior deposits and junior
redemptions, and per-tranche FIFO queue counters. It also implements the first
Phase 5 instant-settlement slice: a creation-time
`InstantSettlement` TLV opt-in for primary-asset, non-tranche `instant_deposit`
and `instant_redeem`, with optional min/max per-transaction deposit and redeem
bounds plus per-user rolling limits. This pass also implements protocol fee
splits: vault-level `protocol_fee_bps` skims from already-computed deposit,
withdrawal, primary instant-redemption, and single-tranche performance fees
before the remaining fee is paid to the regular fee recipient. `Vault`
recipient routing remains the fallback, and a singleton `ProtocolFeeConfig` PDA
can override recipient routing when supplied.

The full MD plan is broader than this implementation. USD-normalized aggregate
NAV, validated venue CPI execution, external protocol custody, tranche-aware or
secondary-asset instant settlement, oracle adapters, tranche-aware performance
fees, global protocol fee bps overrides, and richer program-wide protocol fee
governance are not implemented.

## Phase Results

| Phase                          | Result                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Phase 0 fork baseline          | Implemented: renamed crates, clients, IDL, program ID, package metadata, NOTICE, audit provenance.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Phase 1 roles/risk/NAV         | Partial: core role split, fresh NAV, NAV guard tests, breaker, deposit cap, withdrawal/redemption plus manager deploy/pull rolling limits, secondary per-asset manager rolling buckets, vault-config, deposit/withdrawal fee, and mutable non-fee TLV extension timelock queues, plus single-tranche performance fees with a configurable crystallization interval, vault-level protocol fee bps, and singleton protocol fee recipient routing implemented; tranche-aware fee accrual, global protocol fee bps overrides, and richer program-wide protocol fee governance remain future work.                                                                                                                                                                                                                                                                                                                                                       |
| Phase 2 multi-asset            | Partial: curator-only `VaultAsset` add/remove instructions, per-asset reserve/pending token PDAs, max-approved-asset guard, zero-balance removal guard, secondary-asset request creation plus cancel/reject unwind paths, fail-closed secondary approvals until pricing exists, per-asset deployed-balance updates through the constrained venue-position path, and per-asset deposit cap tests are implemented. USD NAV aggregation remains future work.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Phase 3 venue registry         | Partial: `withdraw_assets` is now disabled by default and gated by the creation-time `ExternallyManagedWithdrawals` TLV extension plus an active `VenueEntry`, matching active `VaultVenue` approval, and recipient token account owned by `VaultVenue.recipient_authority`. `VenueEntry`, `VaultVenue`, and primary/approved-secondary asset `Position` PDAs exist, with a vault-owned SPL token-account deploy/pull stub and post-transfer balance-delta checks. Validated `execute_venue_action`, arbitrary external-protocol custody, and vault-in-vault cycle prevention remain future work.                                                                                                                                                                                                                                                                                                                                                   |
| Phase 4 tranches               | Partial: `initialize_tranches` creates a `TrancheConfig` PDA before vault initialization, requires exactly two tranche mints by reusing the existing `Vault.share_mint` as one side, transfers the second zero-supply tranche mint to vault authority, and stores `Vault.tranche_config`. Tranche-enabled `update_vault_nav` now requires tranche accounts and applies senior-target gain allocation plus junior-first loss allocation. Async request lifecycle paths store `Request.share_mint_address`, validate `TrancheConfig`, enforce configured per-direction request min/max bounds, and mint/burn/claim against the selected tranche mint. `approve_request` enforces the junior-buffer floor for senior deposits and junior redemptions. Optional subscription/redemption queues now use senior/junior lane-local counters in tranche mode. Tranche-aware fees remain future work; tranche vaults reject single-tranche performance fees. |
| Phase 5 instant path/reporting | Partial: `initialize_instant_settlement` enables primary-asset, non-tranche instant deposits and redeems with mandatory nonzero NAV staleness and instant-redemption-fee guards plus optional min/max per-transaction deposit/redeem bounds and per-user rolling limits through `InstantSettlementUser`. The path requires a set NAV, honors NAV staleness, applies deposit/withdrawal plus instant redemption fees, checks reserve liquidity, consumes per-user instant limits plus gross redemption rolling limits, and updates `Vault.total_asset_balance`. Secondary assets, tranche-aware instant settlement, and oracle-priced instant settlement remain future work.                                                                                                                                                                                                                                                                         |
| Phase 6 hardening              | Local verification complete for the implemented subset; added property-test coverage for asset/share round-trip value conservation, tranche-waterfall conservation/loss ordering, rolling-limit epoch-window accounting, secondary venue-position ledger conservation/fail-closed movement, and fee rounding/pass-through invariants, plus integration regressions for secondary per-asset manager rolling buckets and instant-path Token-2022 transfer fees being re-enabled after vault setup. External audit still required.                                                                                                                                                                                                                                                                                                                                                                                                                     |

## Verification

Final verification:

- `PATH="/Users/yashagarwal/.local/share/solana/install/active_release/bin:$PATH" anchor build --ignore-keys`: passed. Anchor 1.0.2 was unavailable through AVM locally, so AVM ran Anchor 0.32.1; Solana 3.1.14 was on PATH.
- `cmp -s target/idl/async_vault_v2.json idl/async_vault_v2.json`: passed.
- `pnpm run generate-clients`: passed. Codama warned that generated Rust client Solana dependency ranges could be updated.
- `cargo +nightly fmt -p async_vault_v2 -p vault_common -p integration-tests -p async-vault-v2-client -- --check`: passed.
- `cargo check -p async-vault-v2-client`: passed.
- `cargo clippy -p async_vault_v2 -p vault_common -p integration-tests -- -D warnings`: passed with no warnings.
- `pnpm run format:check`: passed.
- `pnpm lint`: passed.
- `pnpm run typecheck`: passed.
- `cargo test -p async_vault_v2 -p vault_common`: passed, 50 program tests and 6 shared-library tests.
- `cargo test -p integration-tests`: passed, 280 integration tests.
- `CU_REPORT=1 CU_REPORT_DATE=2026-07-04 cargo test -p integration-tests`:
  passed, 280 integration tests, generated
  `integration-tests/cu_report.md`.
- `git diff --check`: passed.
- `pnpm audit --ignore-unfixable`: passed, no new vulnerabilities ignored.
- `cargo audit`: an earlier pass succeeded with exit 0 across 558 crate dependencies and reported 10 allowed warnings for unmaintained or unsound transitive crates (`ansi_term`, `bincode`, `derivative`, `libsecp256k1`, `paste`, `proc-macro-error2`, `anyhow`, and `rand` versions 0.7.3/0.8.5/0.9.2). Later reruns were blocked by the sandbox's read-only `~/.cargo/advisory-db` lock path and the escalation request was rejected by the approval system; no Rust dependencies changed in the queued extension-update or protocol-fee slices.

## Spec Coverage

See [docs/SPEC_COVERAGE.md](docs/SPEC_COVERAGE.md). No unimplemented Phase 2-6
requirement is marked complete.

## Compute Units

The integration CU map was regenerated/guarded for `async_vault_v2`, including
`pause_vault`. A full
`CU_REPORT=1 CU_REPORT_DATE=2026-07-04 cargo test -p integration-tests` pass
generated [integration-tests/cu_report.md](integration-tests/cu_report.md).

All measured instructions are below the 200k CU flag threshold. The highest
minimum sample was `create_vault` at 29,370 CU; the highest newly added V2 paths
were `add_vault_asset` at 28,086 CU, `create_venue_position` at 26,270 CU,
`pull_venue_position` at 21,666 CU, `deploy_venue_position` at 21,665 CU,
`instant_redeem` at 20,528 CU, `instant_deposit` at 20,519 CU, and
`remove_vault_asset` at 20,192 CU. The venue-gated `withdraw_assets` path
measured 14,375 CU. The mutable non-fee TLV queue instructions measured
`queue_extension_update` at 7,465 CU, `execute_extension_update` at 6,955 CU,
and `cancel_extension_update` at 3,770 CU.

## Deviations From The MD Plan

- The implementation intentionally stops short of claiming Phase 2-6 completion.
- Phase 2 currently covers the asset approval/accounting PDA surface and
  secondary request creation plus cancel/reject unwind paths. Secondary
  approvals fail closed until USD-normalized aggregate NAV and oracle pricing
  exist.
- External-withdrawal and gross redemption rolling limits are enforced with
  in-vault epoch windows. Primary manager deploy/pull uses the vault-level
  manager epoch window, and secondary manager deploy/pull uses per-`VaultAsset`
  epoch windows with the same configured limit and window length.
- `timelock_delay_slots` now enables queued vault config changes through
  `PendingVaultUpdate` and queued deposit/withdrawal fee changes through
  `PendingFeeUpdate`; mutable non-fee TLV extension values for min subscription,
  min redemption, pausable subscriptions, and pausable redemptions queue through
  `PendingExtensionUpdate`. Creation-time TLV extension initialization is still
  not queueable after vault initialization.
- Tranche mode includes config, waterfall accounting on NAV updates, and async
  request settlement against the selected tranche mint, with junior-buffer
  enforcement on senior deposits and junior redemptions plus per-direction
  request min/max bounds and senior/junior lane-local FIFO queue counters.
  Tranche-aware fee accounting is still not implemented.
- Performance fees are implemented for the legacy single-share-mint NAV path,
  including a configurable crystallization interval. Protocol fee skims apply to
  those single-tranche performance fee shares when configured. Tranche vaults
  reject `performance_fee_bps > 0` until tranche-specific high-water marks and
  tranche-aware fee accrual exist.
- Protocol fees are implemented as vault-level `protocol_fee_bps` with
  `Vault.protocol_fee_recipient` as fallback routing. A singleton
  `ProtocolFeeConfig` PDA can override recipient routing, but global protocol
  fee bps, authority transfer, and migration policy remain future work.
- Instant settlement is implemented only for primary-asset, non-tranche vaults.
  It requires nonzero NAV staleness and instant-redemption-fee guards and
  supports optional per-transaction and per-user rolling limits, but does not
  support secondary assets, tranche-aware settlement, or oracle-priced NAV
  verification.
- `Oracle` and `Hybrid` NAV modes are enum/config scaffolding only and rejected
  on-chain.
- `withdraw_assets` is behind an externally managed withdrawal TLV opt-in,
  active venue approval, approved recipient authority, curator authorization,
  and rolling-limit gates. It is not a validated CPI boundary and does not
  validate downstream venue account templates.
- Web app/UI work was out of scope in the MD plan; only stale package/import
  names were updated.

## Recommended Audit Scope

- Full diff against upstream fork base
  `c667cf8079d90f79fc0daf32d332a3693b37e6c6`.
- Account-size and TLV offset changes, especially `Vault` layout offset `672`,
  `Request` extension offset `211`, `InstantSettlement` TLV size `56`,
  `PendingExtensionUpdate`, and the `InstantSettlementUser` PDA.
- Secondary asset approval and removal semantics, including zero-balance close
  checks and Token-2022 mint-extension validation.
- Secondary async asset lifecycle accounting across deposit/redeem request
  creation and cancel/reject unwind paths, fail-closed approval, request asset
  identity checks, pending-vault validation, and per-asset cap enforcement.
- Venue registry state semantics, including registry-authority provenance,
  per-vault approval timelock blocking, paused-entry rejection, use as a
  `withdraw_assets` gate, and the absence of CPI execution in the current
  slice.
- Primary position deploy/pull semantics, including vault-owned token account
  custody, manager/hot-manager authorization, post-transfer balance checks, and
  zero-balance close guards.
- Secondary position deploy/pull semantics, including matching `VaultAsset`
  validation, canonical secondary reserve checks, idle/deployed ledger updates,
  manager/hot-manager authorization, and zero-balance close guards.
- Tranche config initialization and waterfall semantics, including
  pre-initialization-only creation, exact two-mint model using the existing
  `Vault.share_mint`, mint authority transfer for the second tranche mint,
  `Vault.tranche_config` enforcement, bps bounds, zero-supply guards,
  senior-target gain allocation, and junior-first loss allocation.
- Tranche request lifecycle semantics, including `Request.share_mint_address`,
  selected share-mint validation against `TrancheConfig`, base-share-mint PDA
  signer seeds, selected-tranche NAV pricing fallback, and junior share
  mint/burn/claim flows.
- Tranche request-limit semantics, including request-direction indexing,
  zero-as-disabled bounds, and enforcement before deposit transfer or redeem
  share burn.
- Tranche FIFO queue semantics, including lane-local senior/junior
  subscription/redemption counters, writable remaining-account requirements,
  queued cancel tombstones, skip-canceled advancement, and preservation of
  non-tranche global FIFO behavior.
- Junior-ratio floor enforcement, including remaining-account ordering for
  tranche approvals, committed supply caching across NAV updates for
  approved-but-unclaimed deposits, and redemption supply accounting after burn.
- Primary instant-settlement semantics, including the creation-time
  `InstantSettlement` opt-in, non-tranche guard, NAV staleness guard,
  per-transaction deposit/redeem bounds, per-user instant deposit/redeem rolling
  buckets, deposit and withdrawal/instant fee ordering, gross redemption
  rolling-limit accounting, reserve liquidity checks, and
  `Vault.total_asset_balance` updates.
- Primary manager deploy/pull rolling-limit semantics, including shared
  epoch-window reset behavior, gross amount consumption on both deploy and pull,
  interaction with hot-manager routine-safe authorization, and post-transfer
  balance-delta checks.
- Externally managed withdrawal extension initialization and default-disabled
  `withdraw_assets` behavior.
- New authorization checks for curator, fulfiller, and breaker.
- NAV freshness, delta, APY, and staleness guards.
- Performance-fee share minting, high-water-mark accounting, and
  crystallization interval gating.
- Protocol fee recipient and split semantics across async deposit/redeem,
  primary instant redeem, primary instant deposit, and single-tranche
  performance-fee minting, including remaining-account ordering and wrong-owner
  failure behavior.
- Deposit-cap reservation accounting across create/cancel/reject/approve and
  queue-cancel paths.
- Timelock queue authority freshness and cancellation/execute semantics for
  vault config, fee, and mutable non-fee TLV extension updates.
- Confirmation that remaining reserved fields cannot be enabled before
  enforcement exists.

## Review Passes

Security review completed. Blocking findings were fixed:

- Old default-coupled roles remained with the previous authority after curator
  transfer. Fixed by rotating any role equal to the previous authority during
  `accept_authority_invitation`, with regression coverage for `update_nav` and
  `pause_vault`.
- `Oracle` and `Hybrid` NAV modes could be configured without oracle enforcement.
  Fixed by rejecting non-`AuthoritySigned` mode until oracle validation exists.
- Unenforced Phase 1 fields could be set nonzero. Fixed by rejecting unsupported
  protocol/instant-fee and manager-limit fields until enforcement exists, then
  implemented external-withdrawal/redemption rolling limits, single-tranche
  performance fees, and the vault-config timelock queue.

Code review completed over the high-risk diff. Findings were fixed:

- Redemption rolling limits were initially charged against net assets after
  withdrawal fees. Fixed by charging gross redemption assets and adding a fee
  regression.
- Non-fee TLV extension updates could still bypass an active vault timelock.
  Fixed by centralizing the timelock guard in `BasicExtensionAccounts` and
  adding a min-subscription regression.
- The performance-fee CPI initially trusted the caller-supplied token program
  through the mint owner. Fixed by requiring SPL Token or Token-2022 explicitly
  before minting fee shares.

Phase 1 performance-fee interval review completed. No blocking findings:

- The interval is stored in `Vault` and uses the same direct/queued
  `UpdateVault` config path as the existing single-tranche performance-fee bps.
- `update_vault_nav` skips fee minting before the interval elapses and leaves
  `high_water_mark` plus `last_fee_crystallization_timestamp` unchanged, so a
  later post-interval update crystallizes against the previous HWM.
- Existing zero-interval behavior is preserved, including first-HWM
  initialization without fee remaining accounts.
- LiteSVM coverage proves early NAV increases need no fee accounts and mint no
  shares, while post-interval NAV increases mint fee shares and advance the HWM.

Phase 1 queued non-fee TLV extension review completed. No blocking findings:

- `queue_extension_update` is curator-only, requires
  `Vault.timelock_delay_slots > 0`, validates the selected mutable extension
  already exists, and stores the current curator plus `eta_slot` in
  `PendingExtensionUpdate`.
- `execute_extension_update` is permissionless after `eta_slot`, rejects stale
  queued curators, closes the pending account, and reuses the same
  `update_vault_extension` helper as direct updates.
- The queue is intentionally limited to `MinSubscription`, `MinRedemption`,
  `PausableSubscriptions`, and `PausableRedemptions`; live FIFO counters and
  creation-time extensions are not queued.
- LiteSVM coverage proves direct updates are blocked under timelock, early
  execution fails, execution applies after the delay, cancellation preserves the
  existing TLV value, non-curator queue/cancel attempts fail, and invalid
  argument shapes do not leave pending accounts behind.
- Authority-transfer coverage proves a queued extension update from a stale
  curator cannot execute after `eta_slot`, leaves the existing TLV value
  unchanged, and remains cancelable by the current curator.

Phase 1 protocol-fee review completed. No blocking findings:

- `protocol_fee_bps` and `protocol_fee_recipient` use the same direct/queued
  `UpdateVault` config path as other timelocked vault settings and reject
  nonzero bps with a default recipient.
- Existing bps-zero behavior is preserved; protocol recipient accounts are only
  required when a protocol skim is nonzero.
- Async deposit and redeem fee paths validate both the fee-recipient and
  protocol-recipient token accounts before transferring either fee leg.
- Primary instant deposit/redeem paths use optional Anchor-constrained protocol
  recipient token accounts, and performance-fee minting validates the protocol
  share account owner before minting split fee shares.
- The remaining-account order for performance fees accounts for the extra
  protocol share account before tranche accounts, and the generated client
  layout now starts vault TLV extensions at byte `672`.

Phase 3 withdrawal-gate review completed. No blocking findings:

- `withdraw_assets` now rejects vaults without the
  `ExternallyManagedWithdrawals` extension before consuming rolling-limit state
  or transferring tokens.
- The initializer follows the existing TLV lifecycle: curator-only,
  duplicate-protected, and blocked after vault initialization.
- Existing pause, signer, primary-asset, and rolling-limit checks still apply to
  opted-in withdrawals.

Phase 3 venue-registry state review completed. No blocking findings:

- `VenueEntry` creation is bounded by `MAX_VENUE_DISCRIMINATORS`, zeroes unused
  discriminator bytes, and stores explicit registry-authority provenance.
- `VaultVenue` approval is curator-only, rejects paused venue entries, stores
  the approved withdrawal recipient authority, and is blocked by vault timelock
  until a queued venue-approval flow exists.
- No instruction executes arbitrary venue CPI, so the new registry state cannot
  become a permissive CPI escape hatch.

Phase 3 withdraw-assets venue-approval review completed. No blocking findings:

- `withdraw_assets` now requires both the creation-time TLV opt-in and an
  active approved venue with matching recipient authority before consuming
  external-withdraw rolling-limit state or moving reserve tokens.
- The `VaultVenue` PDA is constrained to `(vault, venue_entry)` and checked
  against both stored keys, so a venue approval for another vault or entry
  cannot be replayed.
- Pausing the registry `VenueEntry` invalidates later withdrawal attempts
  through that venue, while existing curator, pause, primary-asset, token
  account, and rolling-limit checks remain in force.
- This remains intentionally narrower than a CPI safety boundary: it binds the
  withdrawal recipient authority to the `VaultVenue` approval, but does not
  validate downstream account templates or target program semantics.

Exit-path pause behavior review completed. No blocking findings:

- `claim` and owner `cancel_request` remain blocked while the vault is paused.
- Curator/fulfiller `reject_request` intentionally remains available while
  paused so pending requests can be unwound during incident response; refunds
  and share mints still route to request-owner token accounts.

Phase 3 position-stub review completed. No blocking findings:

- Deploy/pull moves only between vault-owned token accounts using the vault PDA
  signer and verifies exact reserve/position token-account balance deltas before
  updating `Position.amount`.
- Hot-manager access is limited to routine-safe venue approvals; manager remains
  authorized for non-routine positions.
- Position and vault-venue removal are zero-balance/zero-position gated.

Phase 3 secondary-position review completed. No blocking findings:

- Secondary positions require the matching `VaultAsset` PDA and canonical
  secondary reserve token account before any deploy/pull movement.
- Secondary deploy/pull updates `VaultAsset.idle_balance` and
  `VaultAsset.deployed_balance` only after exact token-account delta
  verification and `Position.amount` accounting.
- LiteSVM coverage pre-funds the secondary reserve/ledger fixture, then verifies
  secondary deploy, partial pull, full pull, and zero-balance position removal.
  A missing-`VaultAsset` secondary position creation attempt fails closed.

Phase 4 tranche-scaffold/waterfall review completed. No blocking findings:

- `initialize_tranches` is curator-only and blocked after vault initialization,
  matching the extension/config setup lifecycle.
- The instruction requires exactly two tranche mints by forcing one side to be
  the existing `Vault.share_mint`; the additional zero-supply mint is transferred
  to vault mint authority.
- Bps bounds, asset-mint rejection, zero-supply guards, token-program owner
  checks, share-mint extension validation, and mint-authority assertions are
  covered by LiteSVM tests.
- `Vault.tranche_config` forces tranche-enabled NAV updates to include the
  writable `TrancheConfig` and tranche mint accounts; waterfall tests cover
  first-update initialization, senior target accrual, residual junior gains, and
  junior-first losses.

Phase 4 tranche-request review completed. No blocking findings:

- Request lifecycle instructions derive the vault PDA from the base
  `Vault.share_mint` but bind settlement to `Request.share_mint_address`.
- Tranche-enabled request paths require `TrancheConfig` as the first remaining
  account, reject non-senior/junior mints, and consume fee-recipient remaining
  accounts after the tranche config during approval.
- LiteSVM coverage includes junior deposit approve/claim, junior redeem
  create/approve/claim, wrong-share-mint rejection, and non-tranche rejection of
  non-base share mints.

Phase 4 junior-ratio review completed. Blocking finding fixed:

- A tranche NAV update between a senior deposit approval and claim could reset
  cached supply back to live mint supply, undercounting approved-but-unclaimed
  senior shares for later ratio checks. Fixed by preserving effective tranche
  supply across NAV updates and updating the selected tranche supply cache on
  approvals.

No remaining blocking findings in the Phase 4 junior-ratio review:

- `approve_request` now rejects senior deposit approvals and junior redemption
  approvals that would take junior value below `min_junior_ratio_bps`.
- Guarded approvals require senior and junior mint accounts after
  `TrancheConfig`, then leave fee-recipient accounts after those tranche
  accounts when fees are owed.
- Deposit approvals cache committed tranche supply in `TrancheConfig`; tranche
  NAV updates preserve that effective supply so approved-but-unclaimed shares
  are counted by later guarded approvals.
- LiteSVM coverage rejects senior deposits and junior redemptions that would
  breach the configured junior floor and covers a NAV update between senior
  approval and claim before a second senior approval attempt.

Phase 4 tranche-request-limit review completed. No blocking findings:

- `initialize_tranches` validates fixed request-limit arrays so any nonzero
  maximum must be greater than or equal to its matching minimum.
- `create_deposit_request` and `create_redeem_request` enforce the selected
  tranche/request-type bounds after `TrancheConfig` and share-mint validation and
  before any deposit transfer or redeem share burn.
- Zero minimum and maximum values disable their respective bounds, preserving
  existing behavior for tranche tests and all non-tranche request paths.
- LiteSVM coverage rejects invalid limit config, senior deposits below their
  minimum, and junior redemptions above their maximum.

Phase 4 tranche FIFO queue review completed. No blocking findings:

- The existing subscription/redemption queue TLV remains the opt-in switch; in
  tranche mode, request IDs are assigned from `TrancheConfig` senior/junior
  deposit/redeem lanes and stored in the existing request queue extension.
- Approve, reject, and skip-canceled paths require a writable `TrancheConfig`
  remaining account before mutating lane-local counters.
- Queued cancel instructions validate the selected request share mint against
  `TrancheConfig` and keep tombstones for lane-local skip.
- LiteSVM coverage proves senior and junior deposit/redeem lanes advance
  independently, same-lane out-of-order approval still fails, canceled junior
  deposit tombstones can be skipped, and existing non-tranche queue suites still
  pass.

Phase 2 secondary-redemption review completed. No blocking findings:

- `create_redeem_request` now permits approved secondary assets only when the
  matching `VaultAsset` PDA is supplied and validates
  `Request.asset_mint_address`/`Request.share_mint_address` through later
  lifecycle steps.
- `approve_request` reuses the existing secondary-asset reserve/pending PDA
  validation and then rejects secondary assets until per-asset pricing exists.
- Secondary redeem claim remains disabled because approval cannot make a
  secondary redeem claimable without pricing.
- Redeem cancel/reject paths occur before assets move and restore the burned
  shares while still validating request asset/share identity.
- LiteSVM coverage includes secondary redeem creation, approval fail-closed
  behavior, missing `VaultAsset` rejection at request creation, and secondary
  redeem cancel/reject share restoration.

Phase 5 instant-settlement review completed. Blocking finding fixed:

- `instant_deposit` and `instant_redeem` initially accepted any vault-owned
  primary-asset token account as the reserve. Fixed by requiring the account to
  equal `Vault.vault_token_account`, with regression coverage proving the
  pending vault cannot be substituted.

No remaining blocking findings in the Phase 5 instant/per-user review:

- The opt-in extension is curator-only, duplicate-protected, and blocked after
  vault initialization.
- Instant settlement is disabled unless the `InstantSettlement` TLV extension is
  present and enabled.
- The handlers reject tranche-enabled vaults, require initialized/unpaused
  state, require nonzero NAV, require nonzero staleness plus
  instant-redemption-fee guards, validate asset-mint extensions, enforce
  per-transaction thresholds before token movement, use checked math, and keep
  reserve liquidity/redemption rolling limits on gross redeemed assets.
- Per-user instant limits are optional and default-disabled; when configured,
  they require `rolling_limit_window_slots`, require and initialize/validate the
  `InstantSettlementUser` PDA keyed by `(vault, user)`, and consume gross deposit
  amount or redeem share amount before token movement or share burn.
- Code review found the first per-user implementation made the user-limit PDA
  rent-bearing even when both per-user limits were disabled. Fixed by making the
  PDA optional and manually creating it only inside nonzero-limit branches, with
  regressions proving zero-limit instant deposit/redeem omit the PDA and
  configured limits reject missing PDA accounts before side effects.
- Local security review of the per-user slice found no signer, seed,
  token-account substitution, checked-math, or failed-limit side-effect gaps
  beyond the fixed reserve-substitution and optional-PDA issues above.

Phase 6 primary manager rolling-limit review completed. No blocking findings:

- `manager_rolling_limit` is no longer inert: nonzero values require a configured
  `rolling_limit_window_slots`, and both direct and queued vault updates share
  the same validation path.
- `deploy_venue_position` and `pull_venue_position` consume the gross movement
  amount after authorization and active venue checks and before token movement;
  failed CPI or balance-delta checks revert the consumed counter with the
  transaction.
- The existing canonical reserve/position token-account constraints and
  post-transfer balance-delta checks remain in place.
- LiteSVM coverage rejects missing window config, rejects deploy+pull over the
  same window limit, and proves reset after the configured slot window.

Phase 6 secondary per-asset manager rolling-bucket review completed. No blocking
findings:

- Secondary `deploy_venue_position` and `pull_venue_position` still require the
  matching writable `VaultAsset`, canonical reserve account, active venue
  approval, and exact post-transfer token-account deltas.
- Primary asset movement continues to consume the vault-level manager bucket;
  secondary asset movement consumes only the matching `VaultAsset` bucket with
  the vault's configured manager limit and window length.
- LiteSVM coverage proves independent secondary assets can each consume the
  configured manager limit in the same window, while same-asset deploy+pull over
  the limit rejects and later succeeds after the slot window resets.

Phase 6 math property-test slice completed. No blocking findings:

- `calculate_shares`/`calculate_assets` property coverage checks randomized
  bounded NAV, decimals, and asset amounts to prove asset→share→asset
  round-trips never mint value and retain only bounded rounding dust.
- `FeeType::Percentage` property coverage checks randomized valid bps and
  amounts against the documented round-up formula, including zero-bps and
  full-bps boundaries.
- `FeeType::FixedAmount` property coverage checks pass-through semantics for
  arbitrary fixed amounts.

Phase 6 venue-position property-test slice completed. No blocking findings:

- Secondary position movement now uses a shared `PositionLedger` helper for
  deploy/pull idle, deployed, and position-amount transitions.
- Property coverage proves secondary deploy/pull conserves idle plus deployed
  asset balances across randomized bounded ledgers.
- Property coverage proves overdeploy and overpull attempts reject without
  mutating the modeled ledger.
- LiteSVM venue tests still validate the account, authority, SPL token-account,
  and post-transfer balance-delta layer around the pure ledger helper.

Phase 6 instant transfer-fee regression slice completed. No blocking findings:

- `instant_deposit` rejects Token-2022 primary asset mints when a nonzero
  transfer fee is re-enabled after vault initialization, matching the upstream
  asset-mint extension invariant.
- `instant_redeem` applies the same re-enabled transfer-fee rejection after the
  reserve has already been funded through an instant deposit.
- Both regressions assert user asset balance, reserve balance, user share
  balance, and share supply remain unchanged on failure.

Phase 6 waterfall property-test slice completed. No blocking findings:

- Tranche waterfall allocation math is now factored into a private pure helper
  that is still called by `update_vault_nav`.
- Property coverage proves senior plus junior allocated assets exactly conserve
  the updated pool value before NAV rounding.
- Property coverage proves rounded NAVs never overstate assets and that
  resulting dust is bounded by the combined one-NAV-unit senior/junior loss.
- Randomized loss cases prove junior assets absorb losses before senior assets
  are impaired.

Phase 6 rolling-limit property-test slice completed. No blocking findings:

- The shared rolling-limit helper is covered against an independent
  epoch-window model over randomized nondecreasing slot sequences and amounts.
- Successful consumptions must exactly match the modeled window start and
  consumed amount.
- Over-limit consumptions reject instead of advancing consumed amount beyond the
  configured cap.
- The disabled zero-limit path succeeds without a nonzero window and does not
  mutate existing rolling-window state.

Remaining gaps are documented in `docs/SPEC_COVERAGE.md`.
