# Handoff: Coinbase Integration Test Suite

## Context

Five contracts are complete and individually tested:
- `emission.es` — 19/19 tests passing (`01-emission-tests/`)
- `treasury_governance.es` — 45 tests passing (`02-treasury-tests/`)
- `treasury_accumulation.es` — tested within `02-treasury-tests/`
- `lp_fund.es` — 40+ tests passing (`03-lp-fund-tests/`)
- `lp_accumulation.es` — tested within `03-lp-fund-tests/`

This task proves they work together as a single coinbase transaction.

## Toolchain

Same as emission tests. Follow the existing emission test file as the pattern:
- Load pre-compiled ErgoTree bytes in Rust (all contracts already compiled)
- Evaluate with `ergo-lib 0.28`
- `cargo test`, no Scala, no node needed

Project location: `05-integration-tests/`

## Setup: Why Accumulation, Not Governance

The emission contract creates bare output boxes — just value + script, no NFT,
no registers. Both governance contracts (`treasury_governance.es`, `lp_fund.es`)
read `SELF.tokens(0)._1` and `SELF.R4`–`R8` at the top level via eager ValDef
evaluation. A bare box at a governance address would crash on register access
before any path logic runs.

The accumulation contracts (`treasury_accumulation.es`, `lp_accumulation.es`)
are the designed intermediaries:
- They are `sigmaProp(bool)` — no NFT or registers required
- They have two paths: consolidation (self → self) and transfer to governance (hash-locked)
- Their existing tests prove the transfer path works

Therefore R4/R5 store accumulation hashes, not governance hashes.

## Setup: ErgoTree Hex Sources (No Compilation Needed)

All contracts are pre-compiled. Reuse existing hex constants:

| Constant | Source File | Variable Name |
|---|---|---|
| Emission | `01-emission-tests/tests/emission_test.rs` | `EMISSION_TREE_HEX` |
| Treasury accumulation | `02-treasury-tests/tests/treasury_test.rs` | `ACCUMULATION_TREE_HEX` |
| LP accumulation | `03-lp-fund-tests/tests/lp_fund_test.rs` | `ACCUMULATION_TREE_HEX` |
| Treasury governance | `02-treasury-tests/tests/treasury_test.rs` | `GOVERNANCE_TREE_HEX` (for test 3c) |
| LP governance | `03-lp-fund-tests/tests/lp_fund_test.rs` | `GOVERNANCE_TREE_HEX` (for test 4c) |

**IMPORTANT:** The two accumulation tree hexes are different — treasury accumulation
embeds governance hash `ecce6bc5...`, LP accumulation embeds `f7e52722...`. They are
not interchangeable.

R4/R5 are register values (`SELF.R4[Coll[Byte]].get`), not compile-time constants.
The emission ErgoTree hex does not change regardless of which scripts go in R4/R5.

## Setup: Hash Computation

1. Load treasury accumulation ErgoTree → `blake2b256(sigma_serialize_bytes)` → this is R4
2. Load LP accumulation ErgoTree → `blake2b256(sigma_serialize_bytes)` → this is R5
3. Build the emission box with R4 and R5 set to the real hashes from steps 1-2
4. All outputs in test transactions must use the actual compiled ErgoTree bytes, not mocks

If the hashes in R4/R5 don't match the actual contract bytes in the outputs, the
emission contract rejects. That's the integration point being tested.

## Setup: Verification Test (Phase 0)

Before evaluation tests, include a setup phase that:
- Loads all ErgoTree hex constants and verifies round-trip serialization
- Computes `blake2b256` of both accumulation trees and prints the hashes
- Verifies `proposition()` parses for each tree

This catches hex corruption early (the "all reject tests pass for the wrong reason"
trap documented in the SKILL).

## NFT IDs

Three distinct test NFTs (from existing test suites):
- Emission NFT: `[0xEE; 32]`
- Treasury governance NFT: `[0xAA; 32]`
- LP governance NFT: `[0xBB; 32]`

## Prover Requirements

| Tests | Contract | Prover |
|---|---|---|
| 1, 2, 5, 6, 7, 8 | Emission (`sigmaProp(bool)`) | `TestProver { secrets: vec![] }` — no keys |
| 3a, 3b, 4a, 4b | Accumulation (`sigmaProp(bool)`) | `TestProver { secrets: vec![] }` — no keys |
| 3c (optional), 4c (optional) | Governance (`atLeast(2, signers)`) | `prover_2_of_3()` with deterministic keys |

Deterministic signer keys are identical to those in `02-treasury-tests` and
`03-lp-fund-tests` (same `SIGNER_SECRET_BYTES`).

## Eager ValDef: Dummy Outputs Required

The emission contract's normal path defines shared `ValDef` bindings that access
`OUTPUTS(0)`, `OUTPUTS(1)`, and `OUTPUTS(2)`. These are evaluated eagerly before
path selection. Any test scenario — including terminal path tests — must provide
at least 3 output boxes to avoid `ByIndex: index out of bounds` crashes. Add dummy
outputs for indices not relevant to the path being tested.

## Test Cases (8 tests)

| # | Test | What It Proves |
|---|---|---|
| 1 | Happy path at height 100 | Full coinbase TX evaluates true. Emission box decreases by blockReward. Treasury accumulation gets 10%, LP accumulation gets 5%. Values sum correctly. Hash binding works with real contract scripts. |
| 2 | Happy path at halving boundary (h=1,577,880) | Block reward transitions to 25 coins. Split amounts recalculate correctly at new reward level. Accumulation outputs receive correct halved amounts. |
| 3 | Treasury accumulation output is independently spendable | (a) Take treasury accumulation box from test 1, verify it can consolidate with itself (accum → accum). (b) Verify it can transfer to treasury governance (accum → governance, hash-locked). These prove the emission → accumulation → governance pipeline works end-to-end. |
| 4 | LP accumulation output is independently spendable | (a) Take LP accumulation box from test 1, verify consolidation. (b) Verify transfer to LP governance. Same pipeline proof as test 3 for the LP side. |
| 5 | Wrong treasury script → reject | Swap OUTPUTS(1) to a different ErgoTree. blake2b256 hash won't match R4. Emission contract must reject. |
| 6 | Wrong LP script → reject | Swap OUTPUTS(2) to a different ErgoTree. Hash won't match R5. Must reject. |
| 7 | Swapped output positions → reject | Put treasury accumulation script in slot 2 and LP accumulation script in slot 1. Hashes don't match their respective registers. Must reject. (Works because the two accumulation scripts have different embedded governance hashes, so their blake2b256 hashes differ.) |
| 8 | Multi-block sequence across halving | Construct consecutive coinbase TXs: block 1,577,879 → 1,577,880 → 1,577,881. Block 1's emission output becomes block 2's input. Verify: (a) creation_height threading (heightCorrect), (b) value decrease chain across reward transition (50→25 coins), (c) R4/R5 register preservation across chained boxes. |

### Test 8 Chaining Detail

```
emission_box[h0-1] → TX@h0 → emission_box[h0] → TX@h0+1 → emission_box[h0+1]
```

Constraints satisfied at each step:
- `heightCorrect`: output `creation_height == HEIGHT`
- `heightIncreased`: next TX's `HEIGHT > prev output's creation_height`
- `valueCorrect`: `output_value == input_value - block_reward(HEIGHT)`
- `registersPreserved`: R4/R5 copied from input to output

Values across the halving (h0 = 1,577,879):
- Block 1,577,879: epoch 0, reward = 50 coins
- Block 1,577,880: epoch 1, reward = 25 coins
- Block 1,577,881: epoch 1, reward = 25 coins

## Deliverable

One Cargo project (`05-integration-tests/`). One test file. All 8 tests plus
setup verification. `cargo test` passes. Brief summary table of results (same
format as emission test results).
