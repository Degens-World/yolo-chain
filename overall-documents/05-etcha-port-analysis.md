# Handoff: Etcha Contract Port Analysis

**Checklist Reference**: Phase 10.4, Phase 10 generally
**Owner**: You
**Blockers**: None — analysis starts now, deployment waits for chain
**Dependencies**: Final block time decision (affects HEIGHT calculations)
**Deliverable**: Audit of every HEIGHT-dependent value in Etcha contracts, modified constants documented

---

## Objective

Identify every place in the Etcha contract suite where block time assumptions affect contract behavior. Ergo has ~120 second blocks. The new chain has 15-20 second blocks. Every HEIGHT-based calculation for time durations needs recalculation.

This is not a rewrite. The contracts are identical ErgoScript. Only numeric constants change.

---

## Contracts to Audit

1. **EtchaP2P option contracts** — maturity dates, exercise windows
2. **EtchaPool contracts** — pool epoch durations, settlement windows
3. **FixedPriceSell market contracts** — listing expiration, cancellation delays
4. **Settlement bot timing** — off-chain but references block heights for deadlines
5. **MM Bot timing** — listing refresh intervals

---

## What Changes

### HEIGHT = Time Conversion

| Duration | Ergo Blocks (120s) | New Chain Blocks (20s) | New Chain Blocks (15s) |
|---|---|---|---|
| 1 hour | 30 | 180 | 240 |
| 24 hours | 720 | 4,320 | 5,760 |
| 7 days | 5,040 | 30,240 | 40,320 |
| 30 days | 21,600 | 129,600 | 172,800 |
| 90 days | 64,800 | 388,800 | 518,400 |
| 1 year | 262,800 | 1,577,880 | 2,103,840 |

### What to Search For in Contract Code

Grep every `.es` file for:
- Hardcoded block height constants (any large integer that represents a time duration)
- `HEIGHT` comparisons with constants (e.g., `HEIGHT > SELF.R4[Long].get + 720`)
- Maturity date calculations
- Expiration window calculations
- Any comment referencing "blocks" or "hours" or "days"

### Specific Etcha Patterns to Check

1. **Option maturity**: stored as absolute HEIGHT in a register. The contract checks `HEIGHT >= maturityHeight`. No contract change needed — but the frontend that calculates maturityHeight from a user-selected date needs the new blocks-per-day constant.

2. **Exercise window**: if there's a post-maturity window for exercising (e.g., "exercise within 720 blocks of maturity"), that constant needs to change to 4,320 blocks (20s) or 5,760 blocks (15s).

3. **Settlement bot deadlines**: the bot checks when settlement is possible. Its polling interval and deadline calculations need recalibration.

4. **FixedPriceSell listing duration**: if listings expire after N blocks, recalculate N.

5. **Black-Scholes pricing**: time to expiry is calculated from (maturityHeight - currentHeight) × blockTimeSeconds. The blockTimeSeconds constant in the frontend needs updating. The pricing math itself is unchanged.

---

## Process

1. Clone the Etcha contract files locally
2. For each `.es` file, search for all numeric constants > 100
3. For each constant, determine: is this a time duration in blocks?
4. If yes, document: current value, what it represents in real time, new value at 20s blocks
5. Create a constants file mapping old → new for each contract
6. For non-contract code (bots, frontend), grep for `blockTime`, `BLOCK_TIME`, `120`, `120000`, `120_000` and similar

---

## What to Deliver

1. **AUDIT_RESULTS.md** — Table of every HEIGHT-dependent constant in every Etcha contract
2. **constants_mapping.json** — Machine-readable old → new constant mapping
3. **FRONTEND_CHANGES.md** — List of frontend/bot constants that need updating
4. **NO_CHANGE_CONFIRMATION.md** — Explicit statement that the ErgoScript logic itself is unchanged, only constants differ

---

## Also Apply To

Run the same analysis on any other contracts planned for deployment:
- Spectrum AMM contracts (check any time-based parameters)
- SigmaFi lending contracts (loan duration encoding)
- Oracle pool contracts (epoch duration, posting intervals)
- Any governance contracts with voting periods
