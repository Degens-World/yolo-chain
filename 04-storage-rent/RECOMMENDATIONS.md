# RECOMMENDATIONS.md — $YOLO Storage Rent Parameters

## Recommended Parameters

| Parameter | Recommended | Ergo Reference | Derivation |
|---|---|---|---|
| Annual fee rate | **312,500 nano/byte/year** (1x Ergo) | 1,250,000 nano/byte per 4yr = 312,500/yr | Match Ergo's annual holder burden |
| Rent cycle | **12 months** (~1,577,880 blocks) | 4 years (~1,051,200 blocks) | Aggressive cleanup, same annual cost |
| Per-event fee | **312,500 nano/byte** (derived) | 1,250,000 nano/byte | = annual_rate × (cycle_months / 12) |
| Minimum box value | **360,000 nanocoins** | 360,000 nanoERG | Match Ergo |
| Collection model | **Miner-only** (100% to miner) | Anyone can collect | Bypasses 85/10/5 split |

---

## The Core Design Principle

The annual fee rate is the constant. The per-event fee scales proportionally with cycle length so that **holders pay the same annual rent regardless of how frequently rent is collected.**

| Cycle | Per-Event Fee (nano/byte) | Events/Year | Annual Cost (250B box) |
|---|---|---|---|
| 6 months | 156,250 | 2 | 0.078125 coins |
| 9 months | 234,375 | 1.33 | 0.078125 coins |
| 12 months | 312,500 | 1 | 0.078125 coins |
| 18 months | 468,750 | 0.67 | 0.078125 coins |
| 24 months | 625,000 | 0.5 | 0.078125 coins |

This means the cycle length decision is purely about **cleanup frequency** — how often the chain sweeps dormant boxes — not about how much holders pay.

---

## Rationale

### Annual Rate: 1x Ergo (312,500 nano/byte/year)

Ergo charges 1,250,000 nano/byte once every 4 years. That is 312,500 nano/byte/year in annual terms. Matching this rate gives $YOLO holders the same annual cost as Ergo holders — a proven, non-punitive level that has run in production since July 2023 with no community complaints.

At 1x Ergo for a standard 250-byte box:

| | Coins |
|---|---|
| Annual cost per box | 0.078125 |
| Per-event cost (12mo cycle) | 0.078125 |
| A 1-coin box survives | 12.8 years |
| A 10-coin box survives | 128 years |
| A 100-coin box survives | 1,280 years |

Higher multipliers were modeled. At 2x Ergo the numbers double (0.15625 coins/year, 1-coin box survives 6.4 years). At 4x, 1-coin box survives 3.2 years. The 1x rate was selected because the shorter cycle already provides 4x Ergo's cleanup frequency — there is no need to also increase the annual cost.

### Cycle: 12 Months

The cycle controls two things: how often each dormant box is visited, and how quickly dust is cleaned up.

At 6 months with 200K UTXOs, the chain processes ~0.076 eligible boxes per block (one every ~13 blocks). At 12 months, it is ~0.038 (one every ~26 blocks). Both are manageable. The 6-month cycle is twice as responsive but exposes holders to rent events after just 6 months of inactivity. For a chain targeting DeFi users who may step away seasonally, 12 months is a more defensible threshold.

The 24-month cycle was rejected because it delays cleanup too long on a chain with 20-second blocks and fast UTXO growth.

### Minimum Box Value: 360,000 nanocoins

At 1x Ergo annual with a 12-month cycle, the per-event cost for a minimal ~100-byte box is 31,250,000 nanocoins (0.03125 coins). A 360,000-nanocoin box (0.00036 coins) is consumed on the first rent event. This is correct behavior — dust should be cleaned immediately.

### Miner-Only Collection

Rent bypasses the 85/10/5 emission split. 100% goes to the miner who includes the rent transaction. This is compensation for state maintenance, not protocol revenue. Miner-only also avoids rent-sniping MEV present in Ergo's "anyone can collect" model.

---

## Key Model Findings

### 1. Rent is a rounding error for miners in the early years

At 1x Ergo annual with 500K UTXOs, rent is 0.018% of miner income during Year 1 (50c/block reward). It only becomes notable during tail emission (1c/block), where it reaches 0.87% of miner income. Even at 4x Ergo with 1M UTXOs at tail emission, rent is ~7% of miner income.

**Rent is a state-hygiene mechanism, not a miner revenue stream.** Market it accordingly.

### 2. Box consumption timelines are generous

At 1x Ergo annual (250-byte box):

| Box Value | Years to Consume |
|---|---|
| 0.001 coins | ~5 days |
| 0.01 coins | ~7 weeks |
| 0.1 coins | 1.3 years |
| 1 coin | 12.8 years |
| 10 coins | 128 years |

Any meaningful holding survives decades. Only dust and lost wallets get cleaned.

### 3. Cycle length is a pure cleanup-speed knob

Because annual cost is constant, changing the cycle from 12 to 6 months does not increase what holders pay. It does:
- Double the eligible-boxes-per-block rate (faster state cleanup)
- Halve the per-event fee (smaller individual deductions)
- Cut the time-to-first-rent-event in half (holder friction)

Chart 6 illustrates this tradeoff. The 12-month cycle is the recommended balance.

### 4. Byte-based > value-based rent

Byte-based rent (charge for storage size) is recommended over value-based rent (charge % of holdings) because:
- It charges for what rent compensates: node state storage
- Value-based creates an incentive to split holdings across many small boxes, increasing UTXO count
- Value-based is a wealth tax that attracts political opposition
- Ergo's byte-based model is proven

---

## Consensus-Level Implementation Notes

For the Rust dev:

1. **Store the annual rate, not the per-event rate.** The per-event fee is derived at collection time: `per_event_fee = annual_fee_nano * rent_cycle_blocks / blocks_per_year`.

2. **Fee rate is consensus-hardcoded**, not miner-voteable. Miners have an incentive to increase fees. If adjustment is ever needed, it goes through a governance hard fork.

3. **Rent collection check**: a box is eligible when `currentHeight - box.creationHeight >= rent_cycle_blocks`. The miner deducts `per_event_fee * box.sizeInBytes` from the box value and recreates it (or fully consumes it if value < fee).

4. **The 12-month cycle in blocks**: `12 × 131,500.8 = 1,578,009.6 → round to 1,577,880` (same as BLOCKS_PER_HALVING, which is a clean coincidence).
