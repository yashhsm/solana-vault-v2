# Audit Status

Last updated: 2026-07-04

Solana Vault V2 is an unaudited community fork of
`solana-foundation/vault`. Do not use it with production funds without an
independent audit.

## Provenance

- Upstream repository: https://github.com/solana-foundation/vault
- Fork base commit: `c667cf8079d90f79fc0daf32d332a3693b37e6c6`
- Upstream audited-through commit: `ce2b5483de53cd015efbbdea70ecec75d976bb08`
- Upstream audit report copied at: `audits/apex-scan-june-22-2026.pdf`
- Upstream program ID: `vaLtx8Su1t5P1CZG5GFEMc94sN4K7A4AUUiciadtvUi`
- V2 program ID: `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg`

## Scope

The upstream audit applies only to the audited upstream commit. V2 changes,
including role separation, NAV guards, breaker pause, deposit-cap accounting,
new account size, generated clients, and package/CI renames, are unaudited.

## Local Review Status

- Code review lens: local high-risk diff review complete; sidecar reviewer timed
  out before final verification cutoff.
- Security review lens: complete; blocking findings fixed and recorded in
  `REPORT.md`.
- External audit: not performed.

## Required Before Production

1. Complete the unimplemented Phase 2-6 product requirements or explicitly
   remove them from the product scope.
2. Run a fresh external audit over the exact deployment commit and program ID.
3. Re-run full CI, SBF build, client generation, cargo audit, pnpm audit, and CU
   benchmarking on the audited commit.
