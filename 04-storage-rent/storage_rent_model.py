#!/usr/bin/env python3
"""
storage_rent_model.py — Storage Rent Economic Model for $YOLO Chain
===================================================================
Fee rate is defined as an ANNUAL rate (nano/byte/year). The per-event fee
scales down proportionally with shorter rent cycles so that annual holder
cost is constant across cycle lengths at the same rate multiplier.

Ergo reference: 1,250,000 nano/byte per 4-year event = 312,500 nano/byte/year.

Chain parameters (from PARAMETERS.md):
  - Block time: 20 seconds
  - Initial reward: 50 coins (halves annually)
  - Miner share: 85% of block reward
  - Tail emission: 1 coin/block
  - Rent collection: 100% to miner (bypasses 85/10/5 split)
"""

import csv
from dataclasses import dataclass
from typing import List

# ──────────────────────────────────────────────────────────────
# Constants from PARAMETERS.md
# ──────────────────────────────────────────────────────────────
BLOCK_TIME_SECONDS = 20
BLOCKS_PER_HALVING = 1_577_880
INITIAL_REWARD = 50.0
MIN_REWARD = 1.0
MINER_PCT = 0.85
MAX_ACTIVE_HALVINGS = 6

BLOCKS_PER_MONTH = (30.44 * 24 * 3600) / BLOCK_TIME_SECONDS  # ~131,500.8
BLOCKS_PER_YEAR = BLOCKS_PER_MONTH * 12

# ──────────────────────────────────────────────────────────────
# Parameter grid
# ──────────────────────────────────────────────────────────────

# Annual fee rates in nano/byte/year (Ergo-equivalent basis)
# Ergo: 1,250,000 nano/byte per 4yr = 312,500 nano/byte/yr
ANNUAL_FEE_RATES = [156_250, 312_500, 625_000, 1_250_000]
ANNUAL_FEE_LABELS = {
    156_250:   "0.5x Ergo",
    312_500:   "1x Ergo",
    625_000:   "2x Ergo",
    1_250_000: "4x Ergo",
}

RENT_CYCLES_MONTHS = [6, 9, 12, 18, 24]
UTXO_COUNTS = [10_000, 50_000, 200_000, 500_000]

CHAIN_AGES = [
    {"label": "Month 6",  "month": 6,  "reward": 50.0},
    {"label": "Year 1",   "month": 12, "reward": 50.0},
    {"label": "Year 1.5", "month": 18, "reward": 25.0},
    {"label": "Year 2",   "month": 24, "reward": 25.0},
    {"label": "Year 3",   "month": 36, "reward": 12.5},
    {"label": "Year 5",   "month": 60, "reward": 6.25},
    {"label": "Year 7+",  "month": 84, "reward": 1.0},
]


def get_block_reward_at_month(month: int) -> float:
    """Calculate block reward based on halving schedule."""
    height = month * BLOCKS_PER_MONTH
    epoch = int(height // BLOCKS_PER_HALVING)
    if epoch >= MAX_ACTIVE_HALVINGS:
        return MIN_REWARD
    reward = INITIAL_REWARD / (2 ** epoch)
    return max(reward, MIN_REWARD)


@dataclass
class RentResult:
    # Inputs
    rent_cycle_months: int
    annual_fee_nano: int
    annual_fee_label: str
    per_event_fee_nano: int          # derived: annual * (cycle_months / 12)
    utxo_count: int
    avg_box_bytes: int
    dormancy_rate: float
    chain_age_label: str
    block_reward: float

    # Outputs
    miner_reward_per_block: float
    rent_revenue_per_block: float
    rent_percentage: float
    monthly_rent_coins: float
    monthly_block_reward_coins: float
    annual_cost_per_box_coins: float  # always = annual_fee * box_bytes / 1e9
    per_event_cost_coins: float       # per_event_fee * box_bytes / 1e9
    years_to_consume: dict            # {box_value: years} for reference values
    eligible_boxes_per_block: float


def model_rent(
    rent_cycle_months: int,
    annual_fee_nano: int,
    utxo_count: int,
    avg_box_bytes: int = 250,
    dormancy_rate: float = 0.30,
    chain_age_label: str = "",
    block_reward: float = 50.0,
) -> RentResult:
    """Core rent model. Fee rate is annual; per-event scales with cycle."""

    # Derive per-event fee from annual rate
    per_event_fee_nano = int(annual_fee_nano * rent_cycle_months / 12)

    rent_cycle_blocks = rent_cycle_months * BLOCKS_PER_MONTH

    # Dormant boxes uniformly distributed across rent cycle
    dormant_boxes = utxo_count * dormancy_rate
    eligible_per_block = dormant_boxes / rent_cycle_blocks

    # Revenue per eligible box (per event)
    per_event_cost_nano = per_event_fee_nano * avg_box_bytes
    per_event_cost_coins = per_event_cost_nano / 1e9

    # Miner income
    miner_reward_per_block = block_reward * MINER_PCT
    rent_revenue_per_block = eligible_per_block * per_event_cost_coins
    total_miner = miner_reward_per_block + rent_revenue_per_block
    rent_pct = (rent_revenue_per_block / total_miner * 100) if total_miner > 0 else 0

    # Monthly figures
    monthly_rent_coins = rent_revenue_per_block * BLOCKS_PER_MONTH
    monthly_block_reward_coins = miner_reward_per_block * BLOCKS_PER_MONTH

    # Annual cost to holder — constant across cycle lengths at same annual rate
    annual_cost_nano = annual_fee_nano * avg_box_bytes
    annual_cost_coins = annual_cost_nano / 1e9

    # Box consumption timelines
    consumption = {}
    for bv in [0.001, 0.01, 0.1, 1.0, 10.0, 100.0, 1000.0]:
        if annual_cost_coins > 0:
            consumption[bv] = bv / annual_cost_coins
        else:
            consumption[bv] = float('inf')

    return RentResult(
        rent_cycle_months=rent_cycle_months,
        annual_fee_nano=annual_fee_nano,
        annual_fee_label=ANNUAL_FEE_LABELS.get(annual_fee_nano, str(annual_fee_nano)),
        per_event_fee_nano=per_event_fee_nano,
        utxo_count=utxo_count,
        avg_box_bytes=avg_box_bytes,
        dormancy_rate=dormancy_rate,
        chain_age_label=chain_age_label,
        block_reward=block_reward,
        miner_reward_per_block=miner_reward_per_block,
        rent_revenue_per_block=rent_revenue_per_block,
        rent_percentage=rent_pct,
        monthly_rent_coins=monthly_rent_coins,
        monthly_block_reward_coins=monthly_block_reward_coins,
        annual_cost_per_box_coins=annual_cost_coins,
        per_event_cost_coins=per_event_cost_coins,
        years_to_consume=consumption,
        eligible_boxes_per_block=eligible_per_block,
    )


def run_focused_grid() -> List[RentResult]:
    """Focused grid: dormancy=0.30, box_size=250. 1 YOLO = 1 YOLO."""
    results = []
    for age in CHAIN_AGES:
        for cycle in RENT_CYCLES_MONTHS:
            for annual_fee in ANNUAL_FEE_RATES:
                for utxo in UTXO_COUNTS:
                    r = model_rent(
                        rent_cycle_months=cycle,
                        annual_fee_nano=annual_fee,
                        utxo_count=utxo,
                        chain_age_label=age["label"],
                        block_reward=age["reward"],
                    )
                    results.append(r)
    return results


def write_csv(results: List[RentResult], filename: str):
    """Write results to CSV — coin-denominated only."""
    fields = [
        'chain_age', 'block_reward', 'rent_cycle_months',
        'annual_fee_label', 'annual_fee_nano', 'per_event_fee_nano',
        'utxo_count', 'avg_box_bytes', 'dormancy_rate',
        'miner_reward_per_block', 'rent_revenue_per_block',
        'rent_percentage', 'monthly_rent_coins',
        'monthly_block_reward_coins',
        'annual_cost_per_box_coins', 'per_event_cost_coins',
        'eligible_boxes_per_block',
    ]

    with open(filename, 'w', newline='') as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        for r in results:
            writer.writerow({
                'chain_age': r.chain_age_label,
                'block_reward': r.block_reward,
                'rent_cycle_months': r.rent_cycle_months,
                'annual_fee_label': r.annual_fee_label,
                'annual_fee_nano': r.annual_fee_nano,
                'per_event_fee_nano': r.per_event_fee_nano,
                'utxo_count': r.utxo_count,
                'avg_box_bytes': r.avg_box_bytes,
                'dormancy_rate': r.dormancy_rate,
                'miner_reward_per_block': f"{r.miner_reward_per_block:.6f}",
                'rent_revenue_per_block': f"{r.rent_revenue_per_block:.9f}",
                'rent_percentage': f"{r.rent_percentage:.6f}",
                'monthly_rent_coins': f"{r.monthly_rent_coins:.4f}",
                'monthly_block_reward_coins': f"{r.monthly_block_reward_coins:.2f}",
                'annual_cost_per_box_coins': f"{r.annual_cost_per_box_coins:.9f}",
                'per_event_cost_coins': f"{r.per_event_cost_coins:.9f}",
                'eligible_boxes_per_block': f"{r.eligible_boxes_per_block:.6f}",
            })


def print_key_scenarios():
    """Key scenario analysis. 1 YOLO = 1 YOLO."""
    print("=" * 80)
    print("KEY SCENARIO ANALYSIS — 1 YOLO = 1 YOLO")
    print("Annual fee rate model: per-event fee scales with cycle length")
    print("=" * 80)

    # Verify: annual cost is constant across cycles
    print("\n--- VERIFICATION: Annual cost per box is constant across cycle lengths ---")
    print(f"{'Cycle':>8} {'Annual Fee':>12} {'Per-Event Fee':>15} {'Annual Cost':>14} {'Per-Event Cost':>16}")
    for cycle in RENT_CYCLES_MONTHS:
        r = model_rent(cycle, 312_500, 50000, chain_age_label="Year 1", block_reward=50.0)
        print(f"{cycle:>5} mo {'1x Ergo':>12} {r.per_event_fee_nano:>13,} n  {r.annual_cost_per_box_coins:>12.6f} c  {r.per_event_cost_coins:>14.6f} c")

    # Q1: Rent as % of miner income
    print("\n--- Q1: Rent as % of miner income (1x Ergo annual, 30% dormancy) ---")
    print(f"{'UTXO Count':>12} {'Cycle':>8} {'Rent %':>10} {'Monthly Rent':>15} {'Chain Age':>12}")
    for age in [CHAIN_AGES[1], CHAIN_AGES[4], CHAIN_AGES[6]]:
        for utxo in [50_000, 200_000, 500_000]:
            for cycle in [6, 12]:
                r = model_rent(cycle, 312_500, utxo, chain_age_label=age["label"], block_reward=age["reward"])
                print(f"{utxo:>12,} {cycle:>5} mo {r.rent_percentage:>9.4f}% {r.monthly_rent_coins:>12.2f} c  {age['label']:>12}")

    # Q3: Annual cost per box
    print("\n--- Q3: Annual rent cost per box (coins, 250B box) ---")
    print(f"{'Annual Rate':>14} {'Annual coins/box':>18}")
    for annual_fee in ANNUAL_FEE_RATES:
        r = model_rent(12, annual_fee, 50000, chain_age_label="Year 1", block_reward=50.0)
        print(f"{r.annual_fee_label:>14} {r.annual_cost_per_box_coins:>18.6f}")

    # Q5: Consumption timelines at 1x Ergo
    print("\n--- Q5: Years to consume box (1x Ergo annual, 250B) ---")
    print(f"{'Box Value':>12} {'Years':>12}")
    r = model_rent(12, 312_500, 50000, chain_age_label="Year 1", block_reward=50.0)
    for bv, yrs in r.years_to_consume.items():
        print(f"{bv:>12.3f} c {yrs:>10.2f} yr")


if __name__ == "__main__":
    print("Running focused parameter grid...")
    results = run_focused_grid()
    write_csv(results, "results.csv")
    print(f"Wrote {len(results)} rows to results.csv")

    print_key_scenarios()

    print(f"\nBlocks per month: {BLOCKS_PER_MONTH:,.1f}")
    print(f"Blocks per year: {BLOCKS_PER_YEAR:,.1f}")
    print(f"Ergo annual equivalent: 312,500 nano/byte/year")
    print(f"At 12mo cycle: per-event = 312,500 nano/byte")
    print(f"At 6mo cycle:  per-event = 156,250 nano/byte")
