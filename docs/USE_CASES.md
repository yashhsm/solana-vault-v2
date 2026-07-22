# Use Cases

This program is a shell for pooled capital: it mints/burns shares, settles
deposits and redemptions against a net asset value (NAV), and enforces roles,
caps, pauses, and fees. It is **not** a strategy engine. The strategy runs
off-chain and drives the vault through its operator roles, and **NAV is signed by
the fulfiller, not proven on-chain**.

Every example below is checked against what the program can actually settle
today. Where an idea needs a capability the program doesn't have yet, it's listed
under [Not yet possible](#not-yet-possible), not dressed up as shippable.

## What the program settles today

| Shell                   | Deposits/redemptions                            | Status                     |
| ----------------------- | ----------------------------------------------- | -------------------------- |
| Instant single-asset    | On demand vs current NAV, reserve-backed        | ✅ shippable               |
| Async single-share      | Request → approve/reject → claim, vs signed NAV | ✅ shippable               |
| Tranche (senior/junior) | Async, with an on-chain loss/gain waterfall     | ✅ shippable               |
| Multi-asset deposits    | Deposit several assets, get one share           | ⛔ fail-closed (see below) |

Secondary-asset approvals currently return `UnsupportedPhaseConfig` — a deposit
in a non-primary asset can be created and refunded but **cannot settle into
shares** until off-chain-priced NAV lands. So no example here depends on
multi-asset deposits, oracle NAV, or the vault calling other protocols directly.

## Read the trust model first

Three properties shape what is honest to build:

1. **NAV is authority-signed.** A fulfiller posts NAV; the program bounds how far
   and how fast it can move, but does not prove it. Consumers trust the operator's
   marks.
2. **Custody leaves the vault when capital is deployed.** The vault PDA custodies
   the reserve. To run any strategy the vault can't reach on-chain, the operator
   pulls capital out through the externally-managed-withdrawal path — which is
   opt-in, restricted to a curator-approved recipient, and rolling-limited, but
   still hands custody to the operator while deployed. Reserve-held funds stay in
   the program; deployed funds don't.
3. **"Instant" is only instant while the reserve covers it.** Instant redemption
   checks reserve liquidity and reverts if it's short, so users fall back to the
   async queue. (This is the Auction-Rate Securities lesson of 2008: if you call
   something cash-equivalent, redemption-on-demand has to be enforceable — here it
   is, by reverting rather than pretending.)

## Examples by shell

Protocol names are real venues that were healthy as of July 2026; treat them as
swappable — the shells are the durable part (see the note at the end).

### 1. Instant single-asset — cash and liquid yield

Deposit and redeem on demand against current NAV, backed by a liquid reserve. The
program now requires a nonzero instant-redemption fee and a NAV-staleness bound
before this can be enabled, which closes the deposit-ahead-of-NAV timing game.

- **On-chain cash / yield sweep** — idle stablecoins earn from a lending market
  (e.g. Kamino Lend, Jupiter Lend) while a reserve covers instant exits.
- **Popular analogues:** a money-market fund or bank cash-sweep account; on
  Solana, the familiar "deposit one asset and earn" shape of JLP.
- **From history:** the money-market fund (1970s). Its failure mode was "breaking
  the buck" — NAV silently assumed to be 1.00. Here NAV floats and the reserve
  check makes redemption honest.

### 2. Async managed fund — active or illiquid strategies

Entries and exits settle as requests against a freshly signed NAV, so the vault
can hold illiquid or external-venue positions. This is also the home for
index/basket products: users deposit **one** asset, the operator holds the basket
off-program, and NAV reflects it.

- **Yield router** — rotate deposits into the best approved lending/LP venue
  (Kamino, Orca, Jupiter Lend) under concentration and risk limits.
- **Delta-neutral carry** — hedged spot/perp positions harvesting funding (perps
  LP such as JLP on Jupiter). _Analogue: Ethena's sUSDe — recognizable, but note it
  has had brief depegs and carries centralized-exchange counterparty risk._
- **Staking-yield vault** — deposit SOL, hold liquid-staking tokens
  (JitoSOL, mSOL, Sanctum) for yield; or make the yield-bearing LST the vault's
  primary asset so NAV simply tracks it.
- **Blue-chip index** — deposit USDC, get exposure to a rebalanced basket of
  ecosystem majors, marked by signed NAV.
- **Prediction-market fund** — a curated basket of event positions on a major
  prediction market.
- **Popular analogues:** an actively managed ETF, a hedge-fund LP share, Ondo's
  USDY (tokenized T-bill yield), a Pendle yield token.
- **From history (novel):** the Victorian **investment trust** — Foreign &
  Colonial, 1868 — was literally "deposit into a professionally managed basket and
  hold shares." It is this shell, 150 years early. Its cautionary descendant, the
  1929 investment-trust pyramid, died of stacked leverage (a trust holding a trust
  holding a company); keep vaults single-layer, because the program does not
  prevent vault-of-vault cycles.

### 3. Tranche (senior/junior) — structured risk

One strategy, two share classes. On each NAV update a waterfall routes first
losses to junior and target gains to senior, with a configurable junior-ratio
floor. The waterfall is on-chain and property-tested.

- **Protected yield note** — senior targets a steady return; junior absorbs first
  losses.
- **First-loss boost** — junior takes the downside for leveraged upside on the
  same strategy.
- **Insurance-buffer vault** — junior capital backstops senior depositors.
- **Popular analogues:** structured notes and principal-protected notes; the
  senior/junior tranching of a CLO (minus the 2008 opacity); Pendle's split of a
  yield-bearing asset into a fixed principal token and a variable yield token.
- **From history (novel):**
    - **RateSetter's Provision Fund** (UK P2P lending) — a mutualized reserve, fed
      by borrower fees, that absorbed defaults before lenders took a loss. Junior is
      that provision fund. It failed when the operator could bypass the fund by
      decree; here the waterfall is program-enforced, not discretionary.
    - **Lloyd's of London "Names"** — capital providers who were paid to stand
      behind policyholders and bear the tail. That is exactly junior: it sells
      protection and can lose principal so senior gets the smoother return.
    - **The Mediterranean _commenda_** (medieval sea-loan partnership) — the
      template that split passive capital from an active operator on an agreed
      profit share. Its weak point was the operator's thin skin in the game; a
      curator who stakes the junior tranche under public senior capital adds exactly
      that first-loss commitment.

## History → V2, filtered to what's feasible

Drawn from _Speculation, Mechanically_ (the historical-instruments field guide),
keeping only instruments that map to a shell the program actually settles.

| Instrument (era)                           | Shell         | On-chain product                  | Failure mode the shell avoids                                 |
| ------------------------------------------ | ------------- | --------------------------------- | ------------------------------------------------------------- |
| Money-market fund (1970s)                  | Instant       | Cash/yield sweep                  | "Breaking the buck" — NAV floats, redemption checks reserve   |
| Investment trust (1868)                    | Async managed | Managed basket fund               | Stacked leverage — keep single-layer (no vault-of-vault)      |
| RateSetter provision fund (2010s)          | Tranche       | Provision-fund yield note         | Operator bypassing the buffer — waterfall is program-enforced |
| Lloyd's "Names" (1688→)                    | Tranche       | Insurance syndicate               | Uncapped liability — junior loss is capped at its stake       |
| _Commenda_ (medieval)                      | Tranche       | Curator-staked first-loss fund    | Manager with no skin in the game — curator holds junior       |
| Prize-linked savings (Premium Bonds, 1956) | Async         | Principal-preserved prize vault\* | Principal at risk — only yield funds the prize                |
| Step-down autocallable (2000s)             | Tranche       | Senior "step-down" coupon note    | Mis-sold as a deposit — cap the barrier, disclose tail loss   |

\* The prize _draw_ is off-chain — the program preserves principal and yield but
has no on-chain randomness. The savings structure is what's feasible here.

## Not yet possible

Do not market these as current guarantees — they need program work first:

- **Multi-asset deposits** — depositing several different assets for one share.
  Secondary-asset settlement is fail-closed until off-chain-priced NAV exists.
- **Oracle-proven NAV / on-chain AUM proof** — NAV is signed by the operator.
- **Fully accounted external-protocol strategies** — generic Merkle-authorized
  CPI exists, but it cannot write vault-owned token accounts and does not infer
  protocol economics. The typed token-balance reference adapter only moves
  assets between canonical vault-owned reserve/position accounts. Lending,
  swap, LP, and external-custody adapters still require protocol-specific code.
- **Vault-of-vault / fund-of-funds on-chain** — no cycle prevention. The 1929
  investment-trust pyramid, and the modern JLP → lending-market liquidation
  cascade, are the cautionary tales.
- **Instant exits from tranche or multi-asset vaults**, **tranche-specific
  performance fees**, and **on-chain lottery/VRF draws**.

---

_Protocol references were verified healthy as of July 2026. DeFi moves fast — for
example, Drift was exploited for ~$285M in April 2026 — so treat any specific
venue as swappable and re-check it before relying on it. The four shells, and
their limits, are the durable part of this page._
