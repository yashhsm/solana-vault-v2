# Design Decisions

This catalogs the feature requests and requirements gathered from ecosystem teams, and for each, how the template handles it — either already implemented in the repo, or the intended approach a team would follow to build it.

The vault is an open-source **base async-vault-v2 template**: teams fork it and add the functionality their own use case needs. It's shipped as a template rather than deployed because every team's requirements differ. The base covers the common ground, with optional extensions for frequently shared needs.

_Both atomic and async vault models were explored. Async was the model teams actually needed, so the atomic track was dropped — this template and catalog cover the async vault only._

**Each item is marked:**

- 🟢 **Implemented in template**
- 🟡 **How it would be built** — a spec'd-but-not-yet-implemented extension, an integrator-side pattern, or an intentional exclusion with the reasoning.

---

## Design baseline (how the template is built)

How the core is approached, before any requested features:

- 🟢 **Shares minted/burned, never transferred** — at each step shares move via mint/burn on the user's account, so transfer-fee-style extensions don't corrupt accounting.
- 🟢 **Five V2 role fields with upstream-compatible defaults** — `curator`, `manager`, `hot_manager`, `fulfiller`, and `breaker` are stored on the vault. New vaults default all roles to the upstream `authority`; two-step curator transfer rotates any role still equal to the old authority so the previous key does not retain default-coupled powers.
- 🟢 **Async request lifecycle** — `RequestDeposit`/`RequestRedeem` move funds to a shared `pending_vault` → authority `ApproveRequest` (snapshots NAV into the request) / `RejectRequest` → user/operator `Claim`; `CancelRequest` while pending. Each request is a unique keypair.

## Deposit / redemption mechanics

- 🟢 **Let someone act on a user's behalf** — `SetOperator`; the operator can claim for the user, while cancel remains owner-only because it closes rent and reverses assets/shares to owner-owned token accounts.
- 🟡 **Auto-pairing of async deposits and redemptions** — an authority instruction that matches an open deposit against an open redemption so they settle against each other at NAV, instead of routing assets through an external strategy. Ecosystem teams flagged that 1:1 netting may have regulatory implications in some jurisdictions; those teams suggested netting against aggregated groups of requests as an alternative.
- 🟡 **Slippage protection on requests** — intentionally **not** supported: it doesn't fit the async + NAV model, and subscribers are expected to understand the fund. (No integrator workaround intended.)
- 🟡 **Multi-asset async lifecycle** — the Phase 2 asset-PDA slice supports curator-approved secondary asset records, token accounts, secondary deposit/redeem request creation plus cancel/reject unwind paths, per-asset deposit caps, and constrained secondary position deploy/pull accounting. Secondary approvals are intentionally disabled until USD-normalized pricing exists; otherwise secondary assets would settle at the primary-asset NAV.
- 🟡 **Multi-asset holdings** — unlikely to be needed: in the async model the vault doesn't hold deployed assets — the authority withdraws and allocates them, so holdings live outside the vault.

## Configurable extensions (opt-in TLV modules)

The most common shared needs, built as toggleable extensions.

| Requested feature                    | Use case                                                       | Approach                                                                    |
| ------------------------------------ | -------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Deposit / Withdraw fees              | Charge fixed or % (bps) fees                                   | 🟢 Fee extension                                                            |
| Performance / management fees        | Fee on gains, or accruing over time                            | 🟡 Single-tranche HWM performance fee implemented; management fees pending  |
| Pausable subscriptions / redemptions | Manual subscription/redemption windows (RWA)                   | 🟢 Pausable extensions                                                      |
| FIFO subscription / redemption queue | Fair, ordered processing                                       | 🟢 Queue extensions (see fairness note below)                               |
| Minimum subscription / redemption    | Floor per request                                              | 🟢 Min extensions                                                           |
| Instant share minting                | Small deposits skip approval, mint instantly under a threshold | 🟡 Primary non-tranche path implemented with thresholds and per-user limits |
| Subscription lock-up period          | Cooldown after approval before shares are claimable            | 🟡 Spec'd extension; not in template                                        |
| Partial subscriptions / redemptions  | Authority partially fills, down to a user-set minimum          | 🟡 Spec'd extension (`min_partial_fill` / `partial_fill`); not in template  |
| Vault asset cap                      | Hard cap on total assets (`deposit + balance ≤ cap`)           | 🟢 Aggregate cap enforced at request creation and deposit settlement        |

- **FIFO fairness** — pure FIFO is unfair when limits exist and a whale consumes the limit first. A team needing fairness would generalize the queue (granular control), or combine authority partial fills with a max-redemption + withdraw cooldown.

## Limits & caps (often regulatory)

- 🟢 **Per-request minimums** — `MinSubscription` / `MinRedemption`.
- 🟡 **Max depositors limit** (regulatory) — an extension tracking a depositor count and rejecting new entrants past the cap.
- 🟡 **Per-investor subscription/redemption caps** — track per-owner totals (likely a per-user account) and enforce a ceiling at request time.
- 🟡 **Withdraw cooldown** — enforce a wait before a user can request redemption; overlaps with the lock-up extension approach.

## Running strategies on vault assets & NAV

- 🟢 **Deploy vault assets into a strategy** (e.g. lend/borrow, an off-chain RWA position) — the common "run custom logic / route assets into a downstream protocol" request. In the async model the authority pulls assets out with `WithdrawAssets`, deploys them wherever the strategy lives, and reflects the result by updating NAV. Strategy execution sits outside the vault by design; a virtual `total_asset_balance` keeps accounting correct while assets are deployed.
- 🟢 **NAV is curator/fulfiller-set in authority-signed mode** — `UpdateNav` bumps `nav_version`, stores update slot/timestamp, and can enforce optional max delta and implied APY guards.
- 🟢 **Fresh NAV settlement** — `ApproveRequest` defaults to requiring `vault.nav_version > request.nav_update_version`, closing the upstream stale-settlement gap. Tests can disable this per vault when checking pure upstream parity.
- 🟡 **Oracle NAV modes** — `Oracle` and `Hybrid` are represented in config but rejected by `UpdateVault` until oracle account validation is implemented. They are layout scaffolding, not live controls.
- 🟡 **Lend/borrow looping, oracle verifiability** — integrator-side: built into whatever strategy the authority runs with the withdrawn assets, not in the template.

## Compliance / KYC / transfer control

Mostly integrator-side or composed with other standards.

- 🟡 **KYC'd tokens without bespoke programs** — compose with **[sRFC 37 (Token ACL)](https://forum.solana.com/t/srfc-37-efficient-block-allow-list-token-standard/4036)**, an efficient block/allow-list standard (improvement over transfer hooks).
- 🟡 **Allow/block-list at transfer time** — some teams found transfer-time lists insufficient for their needs and freeze/unfreeze instead; enshrining all verification on-chain doesn't scale (CU limits, off-chain data). An approach raised: a mint-level **"token movements verifiers"** extension — a configurable m-of-n signer list that must sign a transfer for it to be accepted, so verification happens off-chain and only the approval is recorded on-chain.
- 🟡 **Investor tiering** (retail/accredited/entity/individual) — one approach is on-chain user groups with per-group config, but persona-based tiers (e.g. regulator-defined categories) aren't a global standard. A team would either keep tiering off-chain or define generic on-chain user groups the vault reads.
- 🟡 **KYC/KYB** — off-chain today, keyed to on-chain groups. On-chain credentials (identity NFTs, ZK proofs) explored but not adopted — teams reported these did not meet their regulatory requirements. Long-term aim: a shared on-chain policy system so users aren't onboarded twice.
- 🟡 **Engineering direction for transfer/KYC/KYB tooling** — discussed with teams: modularize into low-granularity instructions; ship basic primitives, keep the rest off-chain.
- 🟡 **Transfer allowlist for assets leaving the vault** — an extension restricting which destinations the authority may withdraw assets to.
- 🟡 **On-chain metadata / prospectus-disclosure standard** (APY, liquidity, instrument data) — an acknowledged gap; would be a separate standard.

## Access control & admin

- 🟢 **Curator handoff** — two-step invite/accept still uses the legacy `authority` field as a curator alias and updates both fields on accept.
- 🟢 **Breaker asymmetry** — `PauseVault` lets the breaker set `paused = true`; only the curator can unpause through `UpdateVault`.
- 🟢 **Manager/hot-manager execution** — manager and routine-safe hot-manager deploy/pull remain available for primary and approved-secondary SPL token-account position stubs. An opt-in `StrategyPolicy` additionally lets those roles execute one external venue CPI only after reconstructing a curator-committed Merkle leaf; protocol-specific arbitrary-CPI position accounting remains future work.
- 🟢 **Manager, external-withdrawal, and redemption rolling limits** — implemented as simple epoch windows stored on the `Vault`. Once `rolling_limit_window_slots` is set, primary manager deploy/pull movement, external withdrawals, and gross redeem settlement consume per-window counters and reset when `current_slot >= window_start + window_slots`.
- 🟢 **Externally managed withdrawal opt-in** — `withdraw_assets` is disabled by default and can only be used by vaults that initialized the `ExternallyManagedWithdrawals` TLV extension before vault initialization. It requires an active vault/venue approval and can only send to a token account owned by the `VaultVenue.recipient_authority`, but it remains a curator-controlled pull, not a validated venue CPI boundary.
- 🟢 **Venue registry state** — `VenueEntry` PDAs are keyed by `(registry_authority, venue_id)` instead of a singleton first-initializer registry. This avoids a front-running-prone global bootstrap while preserving explicit registry-authority provenance. `VaultVenue` approvals bind the target program, routine-safe role status, and approved withdrawal recipient; managed CPI additionally requires a matching strategist policy proof.
- 🟢 **SPL token-account position stub** — Phase 3 deploy/pull currently moves primary assets only between vault-owned token accounts and verifies exact post-transfer balance deltas before changing `Position.amount`. This provides a safe test adapter without exposing arbitrary CPI or external custody.
- 🟢 **Per-asset manager rolling buckets** — primary manager deploy/pull uses the shared vault epoch window, while approved-secondary deploy/pull consumes a per-`VaultAsset` epoch bucket under the same configured limit/window.
- 🟢 **Vault config, extension, and strategy-policy timelocks** — `timelock_delay_slots` enables `PendingVaultUpdate`, `PendingFeeUpdate`, `PendingExtensionUpdate`, and `PendingStrategyPolicyUpdate` flows: curator queues serialized args, anyone can execute after `eta_slot`, and the current curator can cancel. Strategy updates also bind the expected current policy version. Pause/unpause and breaker rotation remain immediate, execution rejects updates queued by a stale curator after authority transfer, direct mutable TLV and policy-root updates are blocked while the timelock is active, and `PendingExtensionUpdate` is intentionally limited to min subscription, min redemption, pausable subscriptions, and pausable redemptions.
- 🟢 **Single-tranche performance fee** — `update_vault_nav` mints fee shares to the configured fee recipient when NAV rises above `high_water_mark`. The share-minting formula accounts for dilution by solving against post-mint supply, floors in favor of the pool, and requires SPL Token/Token-2022 share mint + fee-recipient token account as remaining accounts only when a fee is actually due. `performance_fee_crystallization_interval_seconds = 0` preserves immediate crystallization; nonzero values let NAV update while leaving the high-water mark unchanged until the interval elapses. Tranche vaults reject `performance_fee_bps > 0` until tranche-aware high-water marks exist.
- 🟢 **Protocol fee split with singleton recipient routing** — `protocol_fee_bps` remains vault-scoped and skims from already-computed deposit, withdrawal, primary instant-redemption, and single-tranche performance fees. `Vault.protocol_fee_recipient` remains the fallback recipient, while an optional singleton `ProtocolFeeConfig` PDA can override recipient routing when supplied immediately before the protocol-fee token account. This keeps fee-rate changes in the existing vault timelock while allowing program-level recipient rotation.
- 🟢 **Asset approval/async PDA slice** — `VaultAsset` records are keyed by `(vault, asset_mint)` and own per-asset reserve/pending token PDAs. Curators can add secondary approved assets up to `MAX_APPROVED_ASSETS = 8` including the primary asset and remove them only when stored balances and token balances are zero. Secondary request creation, cancel, and reject paths validate those PDAs; secondary approval/settlement fails closed until per-asset pricing is implemented.
- 🟡 **Tranche config, waterfall, and request binding** — `initialize_tranches` creates a `TrancheConfig` PDA keyed by vault before initialization and stores its address on `Vault.tranche_config`, so tranche-enabled NAV updates cannot silently omit waterfall accounting. One tranche mint must be the existing `Vault.share_mint`, and the second zero-supply tranche mint is transferred to vault mint authority. This avoids accidentally creating senior, junior, and unused legacy share mints while preserving the existing vault seed model. `update_vault_nav` applies senior-target gain allocation and junior-first loss allocation. Request lifecycle instructions keep the vault PDA derived from the base `Vault.share_mint` while storing `Request.share_mint_address` and minting/burning the selected base/senior/junior share mint after validating `TrancheConfig`. Request creation enforces configured per-direction tranche min/max amount bounds. Approval enforces `min_junior_ratio_bps` for senior deposits and junior redemptions, and tranche NAV snapshots preserve committed supply so approved-but-unclaimed deposits cannot be undercounted by later approvals.
- 🟡 **Primary instant settlement** — `InstantSettlement` is a creation-time TLV opt-in that enables one-instruction primary-asset deposits and redeems for non-tranche vaults. Initialization and execution require both nonzero `instant_redemption_fee_bps` and nonzero `max_nav_staleness_slots`; the path then uses current NAV, existing deposit/withdrawal fees, optional min/max per-transaction deposit and redeem bounds, optional per-user epoch-window limits through `InstantSettlementUser` when configured, reserve-liquidity checks, and immediate `Vault.total_asset_balance` updates. Secondary assets, tranche-aware settlement, and oracle-priced instant settlement remain future work.

## V2 delegated design decisions

- 🟢 **Rolling-limit accounting** — current Phase 1 uses in-vault epoch windows for the two implemented value-moving paths. Phase 2/3 per-asset manager accounting should move to epoch buckets keyed by `(vault, asset, limit_kind, bucket_start_slot)` once asset and venue state exists.
- 🟢 **Timelock queue representation** — implemented as one user-supplied pending account per queued mutation: `PendingVaultUpdate` for vault config, `PendingFeeUpdate` for deposit/withdrawal fees, and `PendingExtensionUpdate` for mutable non-fee TLV values. This keeps the first phase simple and mirrors upstream request accounts. Future venue/oracle queues can add typed pending accounts or a hashed payload account if account size becomes a concern.
- 🟢 **Async secondary lifecycle before full asset lifecycle** — secondary deposits and redemptions can enter the async request flow and unwind through cancel/reject, but approval is disabled until USD NAV aggregation and liquidity policy are explicit.
- 🟢 **Externally managed withdrawals are an extension, not a default** — this matches the plan's FR-VENUE-4 safety posture without pretending the CPI boundary is complete. Recipient authority binding prevents arbitrary token-account destinations for an approved venue; future venue adapters should still use validated registry entries and post-CPI balance checks instead of widening `withdraw_assets`.
- 🟢 **Merkle venue capability encoding** — `StrategyPolicy` is a separate `(vault, strategist)` PDA so existing vault/TLV layouts do not migrate. Canonical SHA-256 leaves always bind program, vault, strategist, version, target, discriminator, instruction length, and account count; bounded operators selectively commit instruction ranges and ordered account privileges or select one `u64` for the manager rolling limit. This follows Veda's proof-before-vault-signature model while using a small Solana-owned sanitizer DSL rather than external decoder contracts.
- 🟡 **Vault-in-vault cycle prevention** — not implemented. Preferred next step is registry-level parent DAG validation with bounded-depth checks at approval time.
- 🟡 **Tranche NAV storage** — partially implemented in `TrancheConfig` as per-tranche `u128` NAV fields and supply caches updated by `update_vault_nav` and protected approval paths. Request approvals use selected tranche NAV when nonzero and fall back to aggregate vault NAV for zero-supply bootstrap tranches. Per-direction request min/max bounds and lane-local FIFO counters are stored directly on `TrancheConfig`; preferred next step is tranche-aware high-water marks.
- 🟡 **Oracle adapter trait shape** — not implemented. Preferred next step is explicit adapter instructions per oracle provider; do not dynamically dispatch untrusted oracle programs from the vault.
- 🟡 **Instant path account model** — implemented for primary-asset, non-tranche vaults by bypassing `Request` accounts and sharing fee/freshness primitives where applicable. Configured per-user rolling limits use an `InstantSettlementUser` PDA keyed by `(vault, user)` and the existing `rolling_limit_window_slots` epoch length; zero-limit paths do not create that PDA. Preferred next step is extending the model only after secondary/tranche liquidity accounting is defined.
- 🟡 **Protocol fee governance shape** — partially implemented. The current slice adds a singleton recipient config but intentionally leaves protocol fee bps vault-scoped and enforced through the existing vault-config timelock. A future governance expansion should define global fee caps, authority transfer, migration semantics, and whether a program-level bps override can replace vault-level fields.

## Build & performance

- 🟢 **Anchor** for development speed; intentionally unoptimized. Codama (clients), LiteSVM (tests).
- 🟡 **Perf path** — the likely approaches are switching to Pinocchio or moving to zerocopy for program-owned accounts (and catching Anchor 2 gains).

## Explicitly out of scope

- 🟡 **Referrals** — excluded; adds significant complexity and is better served by existing providers.
- 🟡 **Swap-and-deposit in one transaction** — excluded.

---

## Disclaimer

The content herein is provided for educational and informational purposes only. It is not an offer to sell or a solicitation of an offer to buy any security or derivative, and it should not be relied on as investment, legal, tax, or financial advice.
