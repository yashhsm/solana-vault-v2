# Audit Status

Last updated: 2026-07-04

> **Note**: Solana Vault V2 is an unaudited community fork. The upstream audit
> is provenance evidence only; it does not audit the V2 changes.

## Current Baseline

- Auditor: Cantina (APEX)
- Report: `audits/apex-scan-june-22-2026.pdf`
- Audited-through commit: `ce2b5483de53cd015efbbdea70ecec75d976bb08`
- Fork base commit: `c667cf8079d90f79fc0daf32d332a3693b37e6c6`
- V2 program ID: `3Y4rpSYqrW9JRuiS3YosEXSSYHai4sFH3XMzAQmkNzFg`
- Compare audited baseline delta: https://github.com/solana-foundation/vault/compare/ce2b5483de53cd015efbbdea70ecec75d976bb08...main

Audit scope is commit-based. The audited baseline is the audited-through commit.
All V2 changes after the fork base are outside the Cantina APEX audit scope.

## Branch and Release Model

- `main` is the integration branch and may contain audited and unaudited commits.
- Stable production releases are immutable tags/releases (for example `v1.0.0`).
- Audited baselines are tracked by commit SHA plus immutable tags/releases, not by long-lived release branches.

## Verification Commands

```bash
# Count commits after the audited baseline
git rev-list --count ce2b5483de53cd015efbbdea70ecec75d976bb08..main

# Inspect commit list since audited baseline
git log --oneline ce2b5483de53cd015efbbdea70ecec75d976bb08..main

# Inspect file-level diff since audited baseline
git diff --name-status ce2b5483de53cd015efbbdea70ecec75d976bb08..main
```

## Maintenance Rules

When a new audit is completed:

1. Add the new report to `audits/`.
2. Update `Audited-through commit`, `Audit fixes implemented/verified through commit`, and compare links.
3. Tag audited release commit(s) (for example `vX.Y.Z`).
4. Update README and release notes links if needed.
