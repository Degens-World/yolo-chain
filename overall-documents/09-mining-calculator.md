# Handoff: Mining Profitability Calculator

**Checklist Reference**: Phase 8.4
**Owner**: You
**Blockers**: None
**Dependencies**: Final emission parameters (use placeholders until decided)
**Deliverable**: Deployed web app on Vercel, pulls live chain data post-launch

---

## Objective

Build a web-based mining profitability calculator. This is the first thing a miner looks at before pointing hardware at a new chain. If it doesn't exist, they don't mine. If the numbers look bad, they don't mine. If the UX is clunky, they go back to whattomine.com and pick something else.

Build it now with placeholder chain parameters. Swap to live data when the chain launches.

---

## Core Formula

```
Daily Revenue (coins) = (your_hashrate / network_hashrate) × block_reward × blocks_per_day
Daily Revenue (USD) = Daily Revenue (coins) × token_price
Daily Cost (USD) = power_consumption_watts × 24 × electricity_rate / 1000
Daily Profit (USD) = Daily Revenue (USD) - Daily Cost (USD)
```

Where:
- `blocks_per_day = 86400 / block_time_seconds`
- At 20s blocks: 4,320 blocks/day
- At 15s blocks: 5,760 blocks/day

---

## User Inputs

| Input | Unit | Default | Notes |
|---|---|---|---|
| Hashrate | MH/s | 100 | Common GPU range: 25-500 MH/s for Etchash |
| Power consumption | Watts | 200 | Per GPU, typical 100-350W |
| Electricity cost | $/kWh | 0.10 | Varies wildly by region |
| Token price | $ | 0.01 | Manual input pre-launch, API post-launch |

### Pre-Filled GPU Presets (Optional but High Value)

| GPU | Hashrate (MH/s) | Power (W) |
|---|---|---|
| RTX 3060 | ~35 | ~120 |
| RTX 3070 | ~55 | ~130 |
| RTX 3080 | ~85 | ~230 |
| RTX 4070 | ~55 | ~140 |
| RX 6800 XT | ~60 | ~170 |
| RX 6700 XT | ~40 | ~120 |

Source these from Etchash mining benchmarks (same as ETC benchmarks).

---

## Chain Data Inputs (Auto-Fetched When Chain is Live)

| Data | Source | Pre-Launch Fallback |
|---|---|---|
| Network hashrate | Chain API or pool stats | User input or assumption |
| Current difficulty | Chain API | Calculated from assumed hashrate |
| Block reward | Constant (from emission schedule) | Hardcoded |
| Block time (actual) | Chain API (recent block average) | Target block time |
| Token price | DEX API or CoinGecko (post-listing) | User input |

---

## Outputs to Display

| Output | Format |
|---|---|
| Daily / Monthly / Yearly coins mined | Number |
| Daily / Monthly / Yearly revenue (USD) | Dollar amount |
| Daily / Monthly / Yearly electricity cost | Dollar amount |
| Daily / Monthly / Yearly profit | Dollar amount (green if positive, red if negative) |
| Break-even electricity price | $/kWh where profit = 0 |
| Break-even token price | $ where profit = 0 at current electricity rate |
| Time to mine 1 full coin | Duration |
| Your % of network hashrate | Percentage |

---

## Tech Stack

- **React** (you're familiar with this from existing projects)
- **Tailwind** for styling
- **Vercel** for hosting
- No backend needed — pure client-side calculation
- Post-launch: fetch chain data from public node API (CORS-enabled) or a simple stats API

---

## Pre-Launch Mode vs. Live Mode

**Pre-Launch** (build now):
- All chain parameters hardcoded as constants
- Network hashrate is a slider (let users explore "what if hashrate is X")
- Token price is manual input
- Shows: "Estimated mining returns at various network sizes"
- Marketing value: lets miners model profitability before launch

**Live Mode** (switch at launch):
- Fetch network hashrate and difficulty from chain API
- Fetch token price from DEX or API
- Real-time calculations
- Add: difficulty trend chart (is it going up or down?)

---

## Comparison Feature (Optional but Powerful)

Show side-by-side: "What you'd earn mining ETC vs. mining this chain with the same GPU"

This requires fetching ETC's network stats (available from public APIs). If your chain is more profitable per MH/s than ETC, this comparison is the single most effective marketing tool.

---

## What to Deliver

1. **Deployed web app** at a public URL (Vercel)
2. **Source repo** (React + Tailwind)
3. **constants.ts** — All chain parameters in one file, easy to update
4. **API integration plan** — What endpoints to call post-launch, format expected
5. **GPU presets data** — Hashrate and power benchmarks for common GPUs on Etchash
