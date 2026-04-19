# Handoff: Storage Rent Economic Model

**Checklist Reference**: Phase 5 (design inputs), Phase 0.2
**Owner**: You (modeling), Rust dev (implementation)
**Blockers**: None — modeling starts now
**Dependencies**: None
**Deliverable**: Python model + parameter recommendations for rent cycle, fee rate, and expected miner revenue

---

## Objective

Model the economics of aggressive storage rent to determine optimal parameters. The model must answer: at what rent cycle and fee rate do miners earn meaningful supplemental income without making casual holding punitive?

This is not contract work. This is a spreadsheet/Python exercise that produces the numbers the Rust dev hardcodes into consensus and that you encode into contract parameters.

---

## Parameters to Determine

| Parameter | Ergo Value | Range to Model | Notes |
|---|---|---|---|
| Rent cycle | 4 years (~1,051,200 blocks) | 6 months to 2 years | How long before a dormant box owes rent |
| Fee rate | 1,250,000 nanoERG/byte | TBD per your token value | Cost per byte of box storage per rent cycle |
| Minimum box value | 360,000 nanoERG (Ergo) | TBD | Below this, miner takes entire box |
| Rent collection | Anyone can collect (Ergo) | Miner-only (consensus rule) | Already decided — just model the revenue |

---

## Model Inputs

### UTXO Set Assumptions

Estimate how many boxes exist and their size distribution at various chain ages:

| Chain Age | Estimated UTXO Count | Avg Box Size (bytes) | Source Analogy |
|---|---|---|---|
| Month 1 | 1,000 - 5,000 | ~200 bytes | Early miners + first txns |
| Month 6 | 10,000 - 50,000 | ~250 bytes | Growing user base |
| Year 1 | 50,000 - 200,000 | ~300 bytes | Active DeFi usage |
| Year 2 | 200,000 - 500,000 | ~300 bytes | Mature ecosystem |

Ergo reference: ~5M boxes in UTXO set after ~6 years. Your chain will grow much faster if it has DeFi activity and faster block times.

### Dormancy Assumptions

Not all boxes go dormant. Estimate what percentage of the UTXO set is dormant at any time:

- Active DeFi boxes (AMM pools, lending contracts, options): constantly transacting, never rent-eligible
- Active wallet boxes: users who transact monthly, rent clock resets
- Passive holders: buy and hold for 6+ months — these pay rent
- Lost/abandoned: wallets where keys are lost — these get fully consumed

**Estimate**: 20-40% of UTXO set dormant past the rent cycle at any given time

### Token Price Assumptions

Model across a range: $0.001, $0.01, $0.10, $1.00, $10.00 per coin. The fee rate in nanocoins is constant but the USD cost of rent varies with price.

---

## Model Outputs

For each combination of (rent_cycle, fee_rate, utxo_count, dormancy_rate, token_price):

1. **Monthly miner rent revenue** (in coins and USD)
2. **Rent as % of total miner revenue** (rent / (block rewards + rent))
3. **Annual rent cost per average box** (in coins and USD) — this is what a holder "pays" for dormancy
4. **Break-even token price**: at what price does rent become "annoying" for small holders?
5. **UTXO set shrinkage rate**: how fast do fully-consumed boxes clean up state?

---

## Python Model Structure

```python
# storage_rent_model.py

def model_rent_revenue(
    rent_cycle_months: int,      # 6, 12, 18, 24
    fee_rate_nano: int,          # nanocoins per byte
    utxo_count: int,             # total boxes in UTXO set
    avg_box_bytes: int,          # average box size
    dormancy_rate: float,        # fraction of UTXO set that's dormant
    block_time_seconds: int,     # 15 or 20
    block_reward: float,         # coins per block
    token_price_usd: float       # USD per coin
):
    blocks_per_month = (30.44 * 24 * 3600) / block_time_seconds
    rent_cycle_blocks = rent_cycle_months * blocks_per_month
    
    # How many boxes become rent-eligible per block?
    # Assume dormant boxes are uniformly distributed across the rent cycle
    # So each block, (dormant_boxes / rent_cycle_blocks) boxes become eligible
    dormant_boxes = utxo_count * dormancy_rate
    eligible_per_block = dormant_boxes / rent_cycle_blocks
    
    # Revenue per eligible box
    rent_per_box = fee_rate_nano * avg_box_bytes  # nanocoins
    rent_per_box_coins = rent_per_box / 1e9
    
    # Miner rent revenue per block
    rent_revenue_per_block = eligible_per_block * rent_per_box_coins
    
    # Total miner revenue per block
    total_per_block = block_reward + rent_revenue_per_block
    rent_percentage = rent_revenue_per_block / total_per_block * 100
    
    # Monthly figures
    monthly_rent_coins = rent_revenue_per_block * blocks_per_month
    monthly_rent_usd = monthly_rent_coins * token_price_usd
    monthly_block_reward_coins = block_reward * blocks_per_month
    
    # Cost to holder per box per year
    rent_events_per_year = 12 / rent_cycle_months
    annual_cost_per_box_nano = fee_rate_nano * avg_box_bytes * rent_events_per_year
    annual_cost_per_box_usd = (annual_cost_per_box_nano / 1e9) * token_price_usd
    
    return {
        'rent_revenue_per_block': rent_revenue_per_block,
        'rent_percentage': rent_percentage,
        'monthly_rent_coins': monthly_rent_coins,
        'monthly_rent_usd': monthly_rent_usd,
        'monthly_block_rewards_coins': monthly_block_reward_coins,
        'annual_cost_per_box_usd': annual_cost_per_box_usd,
    }

# Run across parameter grid
for cycle in [6, 12, 18, 24]:
    for price in [0.001, 0.01, 0.1, 1.0, 10.0]:
        for utxo in [10000, 50000, 200000, 500000]:
            result = model_rent_revenue(
                rent_cycle_months=cycle,
                fee_rate_nano=1250000,  # start with Ergo's rate
                utxo_count=utxo,
                avg_box_bytes=250,
                dormancy_rate=0.30,
                block_time_seconds=20,
                block_reward=50,
                token_price_usd=price
            )
            # Output to CSV or print
```

---

## Key Questions the Model Should Answer

1. **At 6 months / Ergo's fee rate**: is rent revenue meaningful (>5% of miner income) at realistic UTXO sizes?
2. **At what UTXO set size does rent become a significant income stream?** This tells you when the "use it or lose it" economy kicks in.
3. **At $0.01/coin, does a casual holder notice?** If holding 1000 coins in one box costs $0.003/year in rent, nobody cares. If it costs $3/year, some people care. Find the threshold.
4. **Should the fee rate scale with box value or just box size?** Ergo charges per byte (box storage size). An alternative: charge percentage of box value. This is more aggressive toward large dormant holdings but departs from Ergo's model. Decide.
5. **What happens to fully consumed boxes?** If a box has 100 coins and rent is 5 coins per cycle, after 20 cycles it's gone. At 6 months, that's 10 years. At 12 months, 20 years. Model the UTXO cleanup rate.

---

## What to Deliver

1. **storage_rent_model.py** — The model script
2. **results.csv** — Full parameter grid output
3. **RECOMMENDATIONS.md** — Recommended rent cycle and fee rate with justification
4. **sensitivity_analysis** — Charts showing how revenue changes across the parameter space
5. **holder_impact.md** — "What does rent cost a typical user?" in plain language (for community communication)
