# Solana Vault V2

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built with Anchor](https://img.shields.io/badge/Built%20with-Anchor-blue)](https://www.anchor-lang.com/)

> Reference implementation only. Solana Vault V2 is unaudited, not deployed to
> mainnet-beta, and not for production funds.

## Attribution

Solana Vault V2 is a community extension of
[solana-foundation/vault](https://github.com/solana-foundation/vault), pinned at
commit `c667cf8079d90f79fc0daf32d332a3693b37e6c6`.

Upstream provides the audited single-asset async vault lifecycle, Token-2022
guards, TLV extension system, request queues, deposit/withdrawal fees, and
authority-controlled NAV update flow. The upstream Cantina APEX report is copied
at [audits/apex-scan-june-22-2026.pdf](audits/apex-scan-june-22-2026.pdf), and
the audited-through upstream commit is
`ce2b5483de53cd015efbbdea70ecec75d976bb08`.

V2 adds a fresh program ID, crate/client/package rename, five role fields,
curator/fulfiller settlement permissions, breaker-only pause, NAV freshness and
bounded-update guards, deposit-cap reservation accounting, a partial asset-PDA
approval and async secondary-asset surface, a creation-time externally managed withdrawal opt-in,
and generated clients for the renamed IDL.
The first Phase 4 tranche slice adds a creation-time `TrancheConfig` scaffold
for senior/junior share mints, waterfall accounting on NAV updates, and
tranche-scoped async request settlement with junior-ratio floor checks.
Tranche request creation also supports per-direction min/max amount bounds for
senior deposit, senior redeem, junior deposit, and junior redeem.
The first Phase 5 instant-settlement slice adds a creation-time
`InstantSettlement` opt-in for primary-asset, non-tranche instant deposit and
redeem flows, with optional min/max per-transaction deposit/redeem bounds and
per-user rolling limits.

This fork is not an official Solana Foundation release and does not imply
Solana Foundation endorsement.

## Status

Implemented:

- Phase 0 fork baseline: `async_vault_v2`, fresh program ID
  `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg`, renamed Rust/TypeScript
  clients, and committed `idl/async_vault_v2.json`.
- Phase 1 subset: role fields, curator-only config mutation, fulfiller request
  approval/rejection, breaker-only pause, strict fresh-NAV settlement by default,
  optional NAV delta/APY/staleness guards, aggregate deposit cap reservations,
  external-withdrawal/redemption and primary manager deploy/pull rolling limits,
  a vault-config timelock queue, deposit/withdrawal fee timelock queue, queued
  mutable non-fee TLV extension updates, and
  single-tranche performance-fee/HWM crystallization on NAV updates, including
  a configurable crystallization interval.
- Phase 2 asset-PDA async lifecycle slice: curator-only secondary asset add/remove,
  per-asset reserve and pending token PDAs, max-approved-asset guard,
  zero-balance removal guard, secondary-asset deposit and redemption lifecycle,
  and per-asset deposit cap enforcement.
- Phase 3 withdrawal-gating slice: `withdraw_assets` is disabled by default and
  requires the `ExternallyManagedWithdrawals` TLV extension initialized before
  vault initialization.
- Phase 3 venue registry state slice: registry-authority `VenueEntry` PDAs and
  curator-controlled `VaultVenue` approvals exist as metadata only. Validated
  venue CPI execution remains future work.
- Phase 3 position stub: primary and approved-secondary `Position` PDAs can
  deploy/pull between vault reserves and vault-owned SPL token accounts with
  post-transfer balance-delta verification and manager rolling-limit
  enforcement. This is not arbitrary protocol execution.
- Phase 4 tranche scaffold/waterfall: `initialize_tranches` records
  senior/junior share mints before vault initialization, reuses the existing
  `Vault.share_mint` as one tranche mint, transfers the second tranche mint to
  vault authority, stores `Vault.tranche_config`, and `update_vault_nav` applies
  senior-target gain allocation plus junior-first loss allocation when tranche
  accounts are supplied. Async request lifecycle instructions now bind requests
  to the selected base/senior/junior share mint and mint/burn that selected
  mint during settlement, claim, cancellation, and rejection flows. Approval
  rejects senior deposits and junior redemptions that would breach
  `min_junior_ratio_bps`, and request creation enforces configured per-tranche
  min/max amount bounds. Optional subscription/redemption queues use
  senior/junior lane-local counters in tranche mode.
- Phase 5 primary instant settlement: `initialize_instant_settlement` enables
  `instant_deposit` and `instant_redeem` for initialized, unpaused, non-tranche
  vaults using the primary asset and base share mint. The path requires a set
  NAV, honors NAV staleness guards, applies existing deposit/withdrawal fees,
  charges `instant_redemption_fee_bps` on instant redeems, checks reserve
  liquidity, enforces optional creation-time min/max instant deposit and redeem
  bounds plus per-user rolling limits, and updates `Vault.total_asset_balance`
  immediately.
- Vault-level protocol fee split: `protocol_fee_bps` and
  `protocol_fee_recipient` split already-computed deposit, withdrawal, primary
  instant-redemption, and single-tranche performance fees before the remaining
  fee is paid to the regular fee recipient.
- Upstream async vault lifecycle and TLV extensions remain ported to the renamed
  program.

Not implemented:

- USD-normalized multi-asset NAV, validated venue CPI execution,
  vault-in-vault cycle prevention, tranche-aware or secondary-asset instant
  settlement, oracle adapters, tranche-aware performance fees, and
  program-wide protocol fee governance/configuration.

See [docs/SPEC_COVERAGE.md](docs/SPEC_COVERAGE.md) and [REPORT.md](REPORT.md)
for the exact coverage table and known limitations.

## Programs

| Network | Program ID                                     | Status                     |
| ------- | ---------------------------------------------- | -------------------------- |
| Local   | `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg` | Build/test target          |
| Devnet  | `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg` | Configured, not verified   |
| Mainnet | N/A                                            | Not deployed / unsupported |

## Documentation

- [Design Decisions](DESIGN_DECISIONS.md)
- [Account Layout](docs/ACCOUNTS.md)
- [Integration Mapping](docs/INTEGRATION.md)
- [Spec Coverage](docs/SPEC_COVERAGE.md)
- [Upstream Test Map](docs/UPSTREAM_TEST_MAP.md)
- [Sequence Diagrams](programs/async_vault_v2/docs/SEQUENCES.md)
- [Subscription Queue](programs/async_vault_v2/docs/extensions/SubscriptionQueue.md)

## Local Development

### Prerequisites

- Rust, using [rust-toolchain.toml](rust-toolchain.toml)
- Node.js and pnpm
- Solana CLI 3.1.14
- Anchor CLI 1.0.2

### Build And Test

```bash
just install
just build
just test
just check
```

Useful direct commands:

```bash
anchor build --ignore-keys
pnpm run generate-clients
cargo test -p async_vault_v2 -p vault_common
cargo test -p integration-tests
```

## Security

The upstream audit does not cover V2 changes. Before any production use, audit
the exact deployment commit and program ID. See [AUDIT_STATUS.md](AUDIT_STATUS.md)
and [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
