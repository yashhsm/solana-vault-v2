# Solana Vault V2

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Built with Anchor](https://img.shields.io/badge/Built%20with-Anchor-blue)](https://www.anchor-lang.com/)

Solana Vault V2 is an experimental, MIT-licensed async vault program for Solana.
It implements a tokenized vault with a NAV-based async deposit/redeem lifecycle
and layers on the control surface a real fund needs:
role-separated operations, stricter NAV-based settlement, multi-asset
accounting, venue approval metadata, tranche accounting, instant settlement, and
protocol fee routing with an optional program-level recipient config.

> Reference implementation only. Unaudited, not deployed to mainnet-beta, and
> not ready for production funds.

## What It Is

At its core, V2 is an operational async-vault stack. Users subscribe and redeem
against a net asset value that operators update, while capital is managed off
the hot path. It starts from a compact async-vault model — Token-2022 support,
TLV extensions, request queues, deposit/withdrawal fees, and authority-signed
NAV updates — and extends it into a role-separated, multi-asset, tranche-aware
system.

The current implementation focuses on testable on-chain primitives:

- separate curator, manager, hot-manager, fulfiller, and breaker roles
- fresh-NAV settlement and bounded NAV update checks
- deposit caps and rolling-limit buckets
- timelocked vault, fee, and mutable TLV extension changes
- approved secondary-asset records and per-asset ledgers
- externally managed withdrawal opt-in and venue approval metadata
- constrained SPL token-account position stubs
- senior/junior tranche accounting and tranche-scoped async requests
- primary-asset instant deposit/redeem flows
- vault-level protocol fee bps with optional program-level recipient routing

## Architecture

At a high level, V2 is still an Anchor program centered on one `Vault` PDA per
share mint. Optional capabilities are layered through typed PDAs and TLV
extensions instead of one large always-on account graph.

```mermaid
flowchart TD
    User[Users and operators] --> Requests[Async deposit/redeem requests]
    User --> Instant[Instant settlement]

    Curator[Curator] --> Config[Vault config and timelock queues]
    Fulfiller[Fulfiller] --> Requests
    Fulfiller --> NAV[NAV updates]
    Breaker[Breaker] --> Pause[Pause only]
    Manager[Manager and hot manager] --> Positions[Venue position stubs]

    Requests --> Vault[Vault PDA]
    Instant --> Vault
    NAV --> Vault
    Config --> Vault

    Vault --> TLV[Vault TLV extensions]
    Vault --> Assets[VaultAsset PDAs]
    Vault --> Venues[VenueEntry and VaultVenue PDAs]
    Vault --> Tranches[TrancheConfig PDA]
    Vault --> Fees[Fee recipients]
    ProtocolConfig[ProtocolFeeConfig PDA] --> Fees

    Assets --> Positions
    Venues --> Positions
```

### Core Accounts

- `Vault`: primary configuration, roles, NAV state, accounting counters, fee
  settings, rolling-limit buckets, and links to optional state.
- `Request`: async deposit/redeem lifecycle account, now asset and share-mint
  scoped for V2 flows.
- `VaultAsset`: approved secondary asset metadata plus per-asset reserve,
  pending, idle, deployed, and cap accounting.
- `VenueEntry` and `VaultVenue`: venue registry metadata, per-vault approval
  state, and approved withdrawal recipient authority. These do not execute
  arbitrary CPI yet.
- `Position`: constrained SPL token-account custody stub for manager deploy/pull
  tests.
- `TrancheConfig`: senior/junior share-mint config, NAV snapshots, request
  limits, FIFO lane counters, and junior-ratio guard data.
- `PendingVaultUpdate`, `PendingFeeUpdate`, `PendingExtensionUpdate`: typed
  timelock queues for delayed config changes.
- `InstantSettlementUser`: optional per-user instant settlement rolling-limit
  bucket.
- `ProtocolFeeConfig`: singleton program-level recipient override for protocol
  fee token accounts. Vault-level `protocol_fee_recipient` remains the fallback.

### Program Shape

- Program name: `async_vault_v2`
- Local/test program ID: `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg`
- IDL: [idl/async_vault_v2.json](idl/async_vault_v2.json)
- Rust client: [clients/rust/async_vault_v2](clients/rust/async_vault_v2)

## Implementation Status

Implemented or partially implemented:

- Phase 0 fork baseline, rename, clients, IDL, program ID, and provenance docs.
- Phase 1 roles, fresh NAV, NAV bounds, deposit caps, rolling limits,
  timelocks, fee queues, performance fees, protocol fee splits, and singleton
  protocol fee recipient routing.
- Phase 2 approved secondary-asset PDAs, request unwind paths, and fail-closed
  secondary approvals until USD-normalized pricing exists.
- Phase 3 externally managed withdrawal opt-in, venue metadata, per-vault venue
  approvals, and constrained position stubs.
- Phase 4 tranche config, waterfall accounting, tranche-scoped requests,
  request bounds, junior-ratio guards, lane-local FIFO queue counters, and
  tranche/performance-fee incompatibility guards.
- Phase 5 primary-asset, non-tranche instant deposit/redeem with mandatory NAV
  staleness plus instant-redemption-fee guards and optional per-transaction and
  per-user limits.

Not implemented yet:

- USD-normalized multi-asset NAV
- oracle adapters and oracle-backed NAV validation
- validated arbitrary venue CPI execution
- external protocol custody accounting
- vault-in-vault cycle prevention
- secondary-asset or tranche-aware instant settlement
- tranche-aware performance fees and high-water marks
- program-wide protocol fee bps override and authority-transfer policy

See [docs/SPEC_COVERAGE.md](docs/SPEC_COVERAGE.md) for the requirement-by-
requirement coverage table.

## Quickstart

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

## Documentation

- [Build Report](REPORT.md)
- [Design Decisions](DESIGN_DECISIONS.md)
- [Account Layout](docs/ACCOUNTS.md)
- [Integration Mapping](docs/INTEGRATION.md)
- [Spec Coverage](docs/SPEC_COVERAGE.md)
- [Upstream Test Map](docs/UPSTREAM_TEST_MAP.md)
- [Sequence Diagrams](programs/async_vault_v2/docs/SEQUENCES.md)
- [Security Policy](SECURITY.md)

## Verification Snapshot

The latest local verification pass for this snapshot included:

- `anchor build --ignore-keys`
- `cargo test -p async_vault_v2 -p vault_common`
- `cargo test -p integration-tests`
- `cargo clippy -p async_vault_v2 -p vault_common -p integration-tests -- -D warnings`
- `cargo +nightly fmt -p async_vault_v2 -p vault_common -p integration-tests -p async-vault-v2-client -- --check`
- `pnpm run format:check`
- `pnpm lint`
- `pnpm run typecheck`

The upstream audit does not cover V2 changes. See [AUDIT_STATUS.md](AUDIT_STATUS.md)
and [REPORT.md](REPORT.md) for details and known gaps.

## Attribution

Originally derived from
[solana-foundation/vault](https://github.com/solana-foundation/vault) (MIT); see
[NOTICE](NOTICE) for the upstream commit and license details. The V2 program is
substantially new work and is not covered by the upstream audit — see
[AUDIT_STATUS.md](AUDIT_STATUS.md).

## License

MIT. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
