# Handoff: Emission Contract

**Checklist Reference**: Phase 4.2, 4.1 (partial)
**Owner**: ErgoScript developer (you)
**Blockers**: None — can start immediately
**Dependencies**: Final parameter decisions (can use placeholders)
**Deliverable**: Tested, verified emission contract ready for genesis deployment

---

## Objective

Write the contract that controls how every coin on the chain comes into existence. This is the coinbase logic — the first transaction in every block spends the emission box and distributes rewards to the miner, treasury, and LP fund according to a fixed schedule.

This is the single most important contract on the chain. If it's wrong, the chain's monetary policy is broken. Ergo's emission was formally verified with Stainless. Ours needs equivalent rigor.

---

## How Ergo Does It (Reference)

Ergo's emission contract lives in the genesis box. Each block, the coinbase transaction:

1. Spends the current emission box (input)
2. Creates a new emission box with reduced value (output — the "change")
3. Creates a miner reward output
4. The contract enforces: output emission box value = input emission box value - block reward
5. Block reward follows a schedule encoded in the contract based on HEIGHT

Ergo's emission source code: `src/main/scala/org/ergoplatform/mining/emission/EmissionRules.scala` in the ergo repo. The ErgoScript contract itself is compiled from the emission rules.

Key Ergo constants for reference:
- Total supply: 97,739,925 ERG
- Initial block reward: 75 ERG (first 2 years)
- Reward reduction: -3 ERG every 3 months after first 2 years
- Foundation allocation: 10% for first 2.5 years (from block reward, not separate)

---

## Our Design

### Emission Split (Per Block)

| Recipient | Share | Example at 50 coins/block |
|---|---|---|
| Miner | 85% | 42.5 coins |
| Dev Treasury | 10% | 5.0 coins |
| LP Fund | 5% | 2.5 coins |

### Emission Schedule (Placeholder — Finalize Before Genesis)

```
Blocks 0 - H1:        50 coins/block    (Year ~1)
Blocks H1 - H2:       25 coins/block    (Year ~2)  
Blocks H2 - H3:       12.5 coins/block  (Year ~3)
Blocks H3 - H4:       6.25 coins/block  (Year ~4)
...halving continues until minimum reward

Where H1 = blocks_per_year = (365.25 * 24 * 3600) / block_time_seconds
At 20s block time: H1 ≈ 1,577,880 blocks
At 15s block time: H1 ≈ 2,103,840 blocks
```

Total supply calculation at 20s blocks:
- Year 1: 1,577,880 × 50 = 78,894,000
- Year 2: 1,577,880 × 25 = 39,447,000
- Year 3: 1,577,880 × 12.5 = 19,723,500
- Year 4+: geometric series converging
- Approximate total: ~157,788,000 coins (adjust as desired)

**Decision needed**: Pick supply target and work backward to block reward and halving schedule. Simple is better. "50 coins per block, halves every year" is one sentence anyone can understand.

---

## Contract Structure (ErgoScript)

```ergoscript
{
  // Emission box contract
  // SELF = current emission box
  // OUTPUTS(0) = new emission box (with reduced coins)
  // OUTPUTS(1) = miner reward
  // OUTPUTS(2) = treasury reward  
  // OUTPUTS(3) = LP fund reward

  val currentHeight = HEIGHT
  
  // Determine current block reward based on height
  val blocksPerHalving = 1577880L  // ~1 year at 20s blocks — PLACEHOLDER
  val initialReward = 50000000000L // 50 coins in nanocoins — PLACEHOLDER
  
  // Calculate halvings elapsed (integer division)
  val halvings = currentHeight / blocksPerHalving
  
  // Block reward = initialReward / 2^halvings
  // ErgoScript doesn't have native exponentiation, so implement via lookup or iterative
  // For simplicity, use a capped lookup table (e.g., max 20 halvings)
  val blockReward = if (halvings == 0L) initialReward
    else if (halvings == 1L) initialReward / 2
    else if (halvings == 2L) initialReward / 4
    else if (halvings == 3L) initialReward / 8
    else if (halvings == 4L) initialReward / 16
    // ... continue to minimum reward
    else 1000000L  // minimum reward (dust level)
  
  val minerReward = blockReward * 85 / 100
  val treasuryReward = blockReward * 10 / 100
  val lpReward = blockReward - minerReward - treasuryReward  // remainder to avoid rounding loss
  
  // Verify emission box is recreated correctly
  val correctEmissionBox = {
    val out = OUTPUTS(0)
    out.propositionBytes == SELF.propositionBytes &&
    out.value == SELF.value - blockReward &&
    out.tokens == SELF.tokens  // preserve any emission tracking tokens
  }
  
  // Verify miner gets correct share
  val correctMinerReward = OUTPUTS(1).value >= minerReward
  
  // Verify treasury gets correct share to correct address
  val treasuryScriptHash = SELF.R4[Coll[Byte]].get  // treasury script hash stored in register
  val correctTreasury = {
    OUTPUTS(2).value >= treasuryReward &&
    blake2b256(OUTPUTS(2).propositionBytes) == treasuryScriptHash
  }
  
  // Verify LP fund gets correct share to correct address
  val lpScriptHash = SELF.R5[Coll[Byte]].get  // LP script hash stored in register
  val correctLP = {
    OUTPUTS(3).value >= lpReward &&
    blake2b256(OUTPUTS(3).propositionBytes) == lpScriptHash
  }
  
  sigmaProp(
    correctEmissionBox &&
    correctMinerReward &&
    correctTreasury &&
    correctLP
  )
}
```

**NOTE**: This is a starting sketch, not production code. The actual contract needs:
- Proper handling of the final emission (when box value < blockReward)
- Edge case when reward hits minimum dust threshold
- Rounding precision — integer division means 85% + 10% + remainder avoids losing nanocoins
- The treasury and LP script hashes baked into registers at genesis, not hardcoded

---

## Testing Plan

### Unit Tests (ErgoScript on Ergo testnet or via AppKit/sigma-rust)

1. **Normal block at height 0**: verify correct reward, correct split, correct emission box change
2. **Block at halving boundary (H1 - 1, H1, H1 + 1)**: verify reward transitions cleanly
3. **Block at each subsequent halving**: verify reward halves correctly
4. **Final emission blocks**: what happens when emission box runs out?
5. **Rounding tests**: at every reward level, verify miner + treasury + LP = blockReward exactly
6. **Reject tests**: attempt to overpay miner (underpay treasury) — contract must reject
7. **Reject tests**: attempt to send treasury to wrong address — contract must reject
8. **Reject tests**: attempt to not recreate emission box — contract must reject

### Differential Testing

Deploy the contract on Ergo testnet. Simulate a full emission lifecycle by constructing transactions at various heights. Compare outputs against a Python/spreadsheet model of expected values at each height.

### Formal Verification (Stretch Goal)

Ergo used Stainless (Scala formal verification tool). For Rust/sigma-rust, explore equivalent tooling or rely on exhaustive property-based testing (e.g., proptest crate style: generate random heights, verify invariants hold).

---

## What to Deliver

1. **emission.es** — The ErgoScript contract source
2. **emission_test.rs** or **emission_test.scala** — Comprehensive test suite
3. **emission_model.py** — Python model that calculates expected values at any height (reference for testing)
4. **PARAMETERS.md** — Document listing all constants with rationale for each choice
5. **EDGE_CASES.md** — Document listing every edge case considered and how it's handled

---

## Known Risks

- **Integer overflow**: nanocoins are big numbers multiplied by percentages. Ensure no overflow in intermediate calculations.
- **Halving implementation**: ErgoScript doesn't have exponentiation. The lookup table approach caps at N halvings. Make N large enough that the remaining supply is negligible.
- **Block time changes**: if miners vote to change block time later, the halving schedule (in blocks) stays the same but real-time duration changes. This is fine — Bitcoin has the same property. Document it.
