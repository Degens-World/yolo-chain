# EDGE_CASES.md — Emission Contract v1.1

**Contract:** `emission.es` v1.1 (Ergo-parity pass applied)
**Validated against:** `emission_model.py` (reference oracle) and `contract_sim.py` (faithful Python simulator of the ErgoScript contract)
**Test suite:** `test_emission_exhaustive.py` — **62 tests, all passing**
**Ergo cross-reference:** `ERGO_COMPARISON.md` — line-by-line comparison against `emissionBoxProp` in `ErgoScriptPredef.scala` (commit `bd1906e`)
**Pending:** sigma-rust on-chain evaluation (rustc 1.85+ local run)

---

## v1.1 changes from v1.0

Applied after cross-referencing against Ergo's canonical emission contract:

1. **[Finding 1 — matches Ergo]** Treasury and LP value checks tightened from `>=` to strict `==`. Ergo uses `EQ(coinsToIssue, Minus(ExtractAmount(Self), ExtractAmount(rewardOut)))` — strict equality on the emission-box delta. Our recipient-value checks now mirror that convention in both `normalPath` and `terminalPath`.

2. **[Gap A — added for Ergo parity]** `heightIncreased = HEIGHT > SELF.creationInfo._1` added as a contract-wide precondition. Mirrors Ergo's `GT(Height, boxCreationHeight(Self))`. Redundant with our singleton-NFT design, kept for defense-in-depth.

3. **[Gap B — added for Ergo parity]** `heightCorrect = nextBox.creationInfo._1 == HEIGHT` added to the normal path's `validEmissionBox` check. Mirrors Ergo's `EQ(boxCreationHeight(rewardOut), Height)`. Locks the successor emission box to the current block.

### Findings that did NOT trigger changes

4. **[Finding 2]** Proposed `MIN_BOX_VALUE` check on successor: **NOT ADDED.** Ergo doesn't include this either — they handle exhaustion via the `lastCoins` bypass, which is structurally equivalent to our terminal path.

5. **[Finding 3]** Dead-code floor clamp in reward lookup: **KEPT.** Ergo uses a different reward formula (linear subtraction with natural zero-floor), so the practice doesn't translate. Our lookup-table structure benefits from the defensive clamp.

6. **[Finding 4]** Zero-value terminal path acceptance: **NO CHANGE.** Ergo's `lastCoins` bypass allows equivalent "anything goes at the end" behavior. Consensus `MIN_BOX_VALUE` makes zero-value boxes unreachable in practice.

See `ERGO_COMPARISON.md` for the full analysis.

---

## Coverage summary (v1.1)

| Category | Coverage |
|---|---|
| Halving boundaries (H−1, H, H+1) | Epochs 0 through 19 — **59 distinct heights** exercised |
| Terminal path entry | 5 scenarios: `remaining = 0`, `remaining = reward−1`, `remaining = MIN_REWARD/2`, realistic walk, same-block-spend reject |
| Rounding at each epoch | Sum-to-reward invariant at every epoch 0–24; specific nanocoin values validated at epochs 0–6 |
| Normal-path rejects | 23 reject scenarios (includes 6 new for v1.1: overpay × 2, heightIncreased × 2, heightCorrect × 2) |
| Terminal-path rejects | 11 reject scenarios (includes 3 new for v1.1: overpay × 2, same-block-spend × 1) |
| Cross-oracle | Python simulator's `block_reward` matches `emission_model.block_reward` at every boundary and at 326 sampled heights spanning 25 epochs |

---

## Handled edge cases

### 1. Halving boundary transitions — handled, tested
Reward drops from `2r` to `r` exactly at `H = epoch × BLOCKS_PER_HALVING`. Tested epochs 0–19 at H−1, H, H+1.

### 2. Floor / min-reward clamp — handled, tested
Epoch 5: `INITIAL_REWARD / 32 = 1,562,500,000` (above `MIN_REWARD`). Epoch 6+: `else minReward` branch returns exactly `MIN_REWARD`. Outer clamp is dead code for enumerated branches (defensive).

### 3. Rounding preservation — handled, tested
Treasury: `reward × 10 / 100`, LP: `reward × 5 / 100`, miner absorbs residual. At every reward level (50, 25, 12.5, 6.25, 3.125, 1.5625, 1.0 coins), split is exact.

### 4. Strict equality on recipient values — **new in v1.1**
`OUTPUTS(1).value == treasuryReward` and `OUTPUTS(2).value == lpReward` (not `>=`). Same for terminal path. Matches Ergo's `EQ` convention. Miner cannot voluntarily overpay treasury/LP — any deviation rejects.

### 5. Terminal path — handled, tested
Triggers when `SELF.value < blockReward`. Strict `==` on `termTreasury` and `termLP`. Emission NFT must be burned (no output carries it) — `.forall` across all outputs. Same-block spend rejected via `heightIncreased`.

### 6. Terminal with zero remaining — handled, design decision documented
`termTreasury = 0`, `termLP = 0`, so `== 0` trivially passes for zero-value outputs. Consensus `MIN_BOX_VALUE` prevents reaching this state. Matches Ergo's equivalent behavior under `lastCoins`.

### 7. Normal / terminal mutual exclusion — handled, tested
`sufficientFunds` and `insufficient` are negations. Both paths cannot simultaneously succeed.

### 8. NFT preservation (normal) and burn (terminal) — handled, tested
Normal: `OUTPUTS(0)` preserves NFT. Terminal: no output may carry NFT anywhere (asymmetric `.forall` enforcement). Tested with NFT smuggled to 3 different output positions.

### 9. Register preservation — handled, tested
`OUTPUTS(0).R4` and `OUTPUTS(0).R5` must equal `SELF.R4` and `SELF.R5`.

### 10. Script preservation — handled, tested
`OUTPUTS(0).propositionBytes == SELF.propositionBytes`.

### 11. Output ordering — handled, tested
Normal: emission / treasury / LP. Terminal: treasury / LP. Swapping treasury ↔ LP rejects under strict `==`.

### 12. Height checks — **new in v1.1**
`heightIncreased`: `HEIGHT > SELF.creationInfo._1`. `heightCorrect`: `OUTPUTS(0).creationInfo._1 == HEIGHT`. Matches Ergo's belt-and-suspenders height enforcement.

### 13. Overflow — handled, verified
Largest intermediate: `reward × 85 = 4.25×10¹²` nanocoins. i64 headroom: 2.17M×. Genesis box value `1.774×10¹⁷`, headroom: 52×.

### 14. Monotonic, bounded rewards — handled, tested
Non-increasing in height. Always within `[MIN_REWARD, INITIAL_REWARD]`.

---

## Known limitations / design decisions

### L1. Miner output not enforced on-chain — by design, deliberate difference from Ergo

**Ergo does** enforce this via `correctMinerOutput`. **Our contract** delegates miner output to the consensus layer.

**Security implication:** a modified node ignoring consensus miner-reward rules could accept underpay/overpay to miner. Network consensus catches this at the block-validity layer. Belt-and-suspenders (Ergo) vs. single-layer (ours) trade-off.

**Recommendation:** document and defer. Adding in-contract miner enforcement would require hardcoding miner-address format and time-lock at genesis — more complexity for marginal benefit if consensus is trusted.

### L2. Zero-value terminal path accepts — acceptable
See handled-case #6.

### L3. New emission box at consensus min-box-value boundary — not checked by contract
When `SELF.value − blockReward < MIN_BOX_VALUE`, the new emission box is consensus-invalid on mainnet. Contract accepts the spend; consensus rejects it. Miner retries with terminal-shaped TX, which accepts. Ergo has the same property via `lastCoins` bypass.

### L4. HEIGHT semantics — standard
`HEIGHT` is the block being spent. Correct for coinbase TX. Double-spend prevented by UTXO model.

### L5. Block-time changes alter real-time halving schedule — by design, documented
Halvings in blocks, not seconds. Same as Bitcoin.

---

## Reject cases exhaustively tested

### Normal path rejects (23 tests, all passing)

| # | Mutation | Contract check that catches it |
|---|---|---|
| 1 | Treasury underpaid by 1 | `validTreasury: OUTPUTS(1).value == treasuryReward` |
| 2 | Treasury overpaid by 1 ✨NEW | same (strict `==`) |
| 3 | Treasury overpaid by 1 ERG ✨NEW | same |
| 4 | LP underpaid by 1 | `validLP: OUTPUTS(2).value == lpReward` |
| 5 | LP overpaid by 1 ✨NEW | same |
| 6 | Treasury to wrong script | `blake2b256(...) == treasuryScriptHash` |
| 7 | LP to wrong script | `blake2b256(...) == lpScriptHash` |
| 8 | Emission box value off by +1 | `valueCorrect` |
| 9 | Emission box value off by −1 | same |
| 10 | Emission NFT missing | `nftPreserved` |
| 11 | Emission NFT wrong ID | same |
| 12 | Emission NFT wrong amount | same |
| 13 | New emission box different script | `scriptPreserved` |
| 14 | R4 changed in new emission box | `registersPreserved` |
| 15 | R5 changed in new emission box | same |
| 16 | OUTPUTS(0) is not an emission box | `nftPreserved` + `scriptPreserved` fail |
| 17 | Treasury/LP outputs swapped | `validTreasury` (value mismatch) |
| 18 | Treasury output missing | out-of-bounds |
| 19 | LP output missing | out-of-bounds |
| 20 | Same-block spend ✨NEW | `heightIncreased` |
| 21 | HEIGHT < creation_height ✨NEW | same |
| 22 | Successor creation_height = H−1 ✨NEW | `heightCorrect` |
| 23 | Successor creation_height = H+1 ✨NEW | same |

### Terminal path rejects (11 tests, all passing)

| # | Mutation | Contract check |
|---|---|---|
| 1 | Terminal treasury underpaid | `== termTreasury` |
| 2 | Terminal treasury overpaid ✨NEW | same |
| 3 | Terminal LP underpaid | `== termLP` |
| 4 | Terminal LP overpaid ✨NEW | same |
| 5 | Terminal treasury wrong script | `blake2b256(...) == R4` |
| 6 | Terminal LP wrong script | `blake2b256(...) == R5` |
| 7 | NFT smuggled to OUTPUTS(0) | `nftBurned` |
| 8 | NFT smuggled to OUTPUTS(1) | same |
| 9 | NFT smuggled to appended output | same |
| 10 | Terminal-shape when normal valid | `insufficient` fails |
| 11 | Terminal same-block spend ✨NEW | `heightIncreased` |

---

## Remaining recommendations (optional, non-blocking)

1. **Simplify the lookup's dead-code floor clamp** — outer `if (computed > minReward) ...` is only reachable via `else minReward` branch. Can drop for ~3 ErgoTree bytes saved. Or keep as defense.

2. **Run sigma-rust harness on rustc 1.85+ machine** — see `emission_test.rs`. Python simulator tests same logic contract expresses; ErgoTree eval confirms deterministic compilation behaves identically.

3. **Document the miner-output design difference** (L1) in project security notes. Clear acknowledgment that consensus-layer enforcement is load-bearing for miner share.

---

## Verdict

Per reviewer's handoff-01 close-out criteria:
- [x] **Exhaustively tested at every halving boundary** — 59 boundary heights, epochs 0–19, 3 offsets each.
- [x] **Terminal path** — entry, steady state, 5 accept scenarios, 11 reject scenarios including NFT burn at 3 positions and same-block-spend.
- [x] **Rounding at each epoch** — sum-to-reward invariant across epochs 0–24.
- [x] **Ergo-parity review completed** — `ERGO_COMPARISON.md`. 3 adjustments applied (strict `==`, heightIncreased, heightCorrect).

Outstanding: on-chain sigma-rust evaluation on a developer machine with rustc 1.85+.
