# What Does Storage Rent Cost You?

**A plain-language guide for $YOLO holders**

---

## The Short Version

If you use your wallet at least once a year, storage rent costs you **nothing**. Every transaction resets the clock.

If you hold $YOLO without transacting for over a year, you'll pay **0.078 coins per box per year** in rent — and your coins stay exactly where they are.

---

## How It Works

Every box (UTXO) on $YOLO has a creation height. If a box sits untouched for **12 months**, miners can collect a small rent fee from it. The fee is based on the box's **size in bytes** — not the value inside it.

After collecting the fee, the miner recreates your box with the remaining value. The 12-month clock resets.

---

## What It Costs

For a typical 250-byte box: **0.078125 coins per year.**

That number does not change with token price, market conditions, or how much $YOLO is in the box. A box holding 1 coin pays the same rent as a box holding 10,000 coins.

The annual rate matches Ergo's — same cost per byte per year, proven in production since 2023.

---

## How Long Your Box Survives

If you go completely dormant:

| Box Value | Survives For |
|---|---|
| 0.01 coins | ~7 weeks |
| 0.1 coins | 1.3 years |
| 1 coin | 12.8 years |
| 10 coins | 128 years |
| 100 coins | 1,280 years |
| 1,000 coins | 12,800 years |

Any box with 1+ coins in it is safe for over a decade of total neglect. A box with 10+ coins outlasts you.

---

## When Rent Adds Up

Rent is per-box. One box = one annual fee. Fifty boxes = fifty annual fees.

| Boxes in Wallet | Annual Rent (coins) |
|---|---|
| 1 | 0.078 |
| 5 | 0.391 |
| 10 | 0.781 |
| 50 | 3.906 |

If you have lots of small boxes from receiving many transactions, consolidate. One self-send merges your UTXOs and resets all clocks.

---

## Who Pays Nothing

- Anyone who transacts at least once per year
- Coins in active DeFi contracts (AMM pools, lending, options — constantly transacting)
- Anyone who periodically consolidates their wallet

---

## Who Gets Cleaned Up

The rent mechanism targets three things:

**Dust**: Tiny leftover amounts from old transactions. A 0.01-coin box is consumed within weeks. State bloat removed.

**Lost wallets**: Keys gone forever. A 10-coin box takes 128 years to fully consume. The coins recirculate very slowly — no one is harmed.

**Spam**: Creating millions of tiny boxes to bloat the chain costs an ongoing maintenance fee that makes the attack uneconomical.

---

## Compared to Ergo

$YOLO matches Ergo's annual cost per byte. The difference is cleanup frequency:

| | Ergo | $YOLO |
|---|---|---|
| Annual cost (250B box) | 0.078 coins | 0.078 coins |
| Rent cycle | Every 4 years | Every 12 months |
| Per-event fee | 0.3125 coins | 0.078 coins |
| Collection | Anyone | Miner-only |

Same annual burden, 4x more frequent sweeps, smaller individual deductions. The shorter cycle keeps state leaner on a chain with faster block times and higher expected DeFi throughput.

---

## The Bottom Line

Storage rent is a maintenance fee that keeps the chain lean. For active users the cost is zero. For inactive holders it is 0.078 coins per box per year — a rounding error on any meaningful position. The only boxes consumed are dust and lost wallets, which is exactly the point.

1 YOLO = 1 YOLO. Rent is 0.078 of them per box per year.
