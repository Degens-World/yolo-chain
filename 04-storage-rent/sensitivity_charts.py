#!/usr/bin/env python3
"""
sensitivity_charts.py — Sensitivity analysis for $YOLO storage rent.
Annual fee rate model. 1 YOLO = 1 YOLO.
"""
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.colors import LogNorm
from storage_rent_model import (
    model_rent, RENT_CYCLES_MONTHS, ANNUAL_FEE_RATES, ANNUAL_FEE_LABELS,
    UTXO_COUNTS, CHAIN_AGES, BLOCKS_PER_MONTH,
    get_block_reward_at_month, MINER_PCT
)

plt.rcParams.update({
    'figure.facecolor': '#0f1117',
    'axes.facecolor': '#0f1117',
    'text.color': '#e0e0e0',
    'axes.labelcolor': '#e0e0e0',
    'xtick.color': '#a0a0a0',
    'ytick.color': '#a0a0a0',
    'axes.edgecolor': '#333333',
    'grid.color': '#222222',
    'font.family': 'monospace',
    'font.size': 10,
})

COLORS = ['#00d4ff', '#ff6b6b', '#51cf66', '#ffd43b', '#cc5de8']
FEE_COLORS = {'0.5x Ergo': '#51cf66', '1x Ergo': '#00d4ff', '2x Ergo': '#ffd43b', '4x Ergo': '#ff6b6b'}


def chart1_rent_pct_vs_utxo():
    """Rent % of miner income vs UTXO count. Since annual rate is constant
    across cycles, cycle length doesn't affect this — show fee rate multipliers instead."""
    fig, axes = plt.subplots(1, 3, figsize=(18, 6), sharey=True)
    ages = [
        {"label": "Year 1 (50c)", "reward": 50.0},
        {"label": "Year 3 (12.5c)", "reward": 12.5},
        {"label": "Tail (1c)", "reward": 1.0},
    ]
    utxo_range = np.linspace(10_000, 1_000_000, 50)
    for ax_idx, age in enumerate(ages):
        ax = axes[ax_idx]
        for f_idx, (fee, label) in enumerate(ANNUAL_FEE_LABELS.items()):
            pcts = []
            for utxo in utxo_range:
                r = model_rent(12, fee, int(utxo),
                               chain_age_label=age["label"], block_reward=age["reward"])
                pcts.append(r.rent_percentage)
            ax.plot(utxo_range / 1000, pcts, label=label,
                    color=FEE_COLORS[label], linewidth=2)
        ax.set_title(age["label"], fontsize=11, fontweight='bold')
        ax.set_xlabel('UTXO Count (thousands)')
        ax.grid(True, alpha=0.3)
        if ax_idx == 0:
            ax.set_ylabel('Rent as % of Miner Income')
        ax.legend(title='Annual Fee Rate', loc='upper left', fontsize=8)
    fig.suptitle('$YOLO — Rent % of Miner Income vs UTXO Set Size\n'
                 '(annual rate model, 30% dormancy, 250B box)',
                 fontsize=13, fontweight='bold', y=1.02)
    plt.tight_layout()
    fig.savefig('chart1_rent_pct_vs_utxo.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart1 — rent % vs UTXO (by fee rate)")


def chart2_annual_cost_heatmap():
    """Heatmap: annual cost per box across fee rates. Cycle length
    doesn't change annual cost, so show fee rate vs box size."""
    fig, ax = plt.subplots(figsize=(12, 7))
    box_sizes = [120, 200, 250, 300, 400]
    fees = list(ANNUAL_FEE_LABELS.keys())
    fee_labels = list(ANNUAL_FEE_LABELS.values())
    data = np.zeros((len(fees), len(box_sizes)))
    for i, fee in enumerate(fees):
        for j, bs in enumerate(box_sizes):
            r = model_rent(12, fee, 50000, avg_box_bytes=bs,
                           chain_age_label="Year 1", block_reward=50.0)
            data[i][j] = r.annual_cost_per_box_coins
    im = ax.imshow(data, cmap='YlOrRd', aspect='auto',
                   norm=LogNorm(vmin=data.min(), vmax=data.max()))
    ax.set_xticks(range(len(box_sizes)))
    ax.set_xticklabels([f'{bs}B' for bs in box_sizes])
    ax.set_yticks(range(len(fees)))
    ax.set_yticklabels(fee_labels)
    ax.set_xlabel('Box Size (bytes)')
    ax.set_ylabel('Annual Fee Rate')
    for i in range(len(fees)):
        for j in range(len(box_sizes)):
            val = data[i][j]
            color = 'white' if val > 0.2 else 'black'
            ax.text(j, i, f'{val:.4f}', ha='center', va='center',
                    fontsize=11, color=color, fontweight='bold')
    ax.set_title('$YOLO — Annual Rent Cost per Box (coins)\n'
                 '(constant across all cycle lengths)',
                 fontsize=13, fontweight='bold')
    fig.colorbar(im, ax=ax, label='Coins per box per year')
    plt.tight_layout()
    fig.savefig('chart2_annual_cost_heatmap.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart2 — annual cost heatmap (fee rate × box size)")


def chart3_rent_over_chain_life():
    """Rent % over chain lifetime. Since cycle doesn't affect annual revenue,
    show different (UTXO count × fee rate) combos."""
    fig, ax = plt.subplots(figsize=(14, 7))
    months = list(range(1, 121))
    scenarios = [
        (200_000, 312_500,   "200K UTXO, 1x Ergo"),
        (500_000, 312_500,   "500K UTXO, 1x Ergo"),
        (500_000, 625_000,   "500K UTXO, 2x Ergo"),
        (1_000_000, 625_000, "1M UTXO, 2x Ergo"),
    ]
    for s_idx, (utxo, fee, label) in enumerate(scenarios):
        pcts = []
        for m in months:
            reward = get_block_reward_at_month(m)
            r = model_rent(12, fee, utxo,
                           chain_age_label=f"M{m}", block_reward=reward)
            pcts.append(r.rent_percentage)
        ax.plot(months, pcts, label=label, color=COLORS[s_idx], linewidth=2)
    for year in range(1, 8):
        ax.axvline(x=year * 12, color='#555555', linestyle='--', alpha=0.5)
        ax.text(year * 12 + 0.5, ax.get_ylim()[1] * 0.95 if ax.get_ylim()[1] > 0 else 0.5,
                f'H{year}', va='top', fontsize=8, color='#777777')
    ax.set_xlabel('Chain Age (months)')
    ax.set_ylabel('Rent as % of Miner Income')
    ax.set_title('$YOLO — Rent Revenue % Over Chain Lifetime\n'
                 '(annual rate model, 30% dormancy, 250B box)',
                 fontsize=13, fontweight='bold')
    ax.legend(loc='upper left')
    ax.grid(True, alpha=0.3)
    ax.set_xlim(1, 120)
    plt.tight_layout()
    fig.savefig('chart3_rent_over_time.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart3 — rent % over chain lifetime")


def chart4_monthly_rent_coins():
    """Monthly rent revenue in coins with miner block reward reference."""
    fig, axes = plt.subplots(1, 3, figsize=(18, 6))
    ages = [
        {"label": "Year 1", "reward": 50.0},
        {"label": "Year 3", "reward": 12.5},
        {"label": "Tail", "reward": 1.0},
    ]
    utxo_range = [10_000, 50_000, 100_000, 200_000, 500_000, 1_000_000]
    for ax_idx, age in enumerate(ages):
        ax = axes[ax_idx]
        x = np.arange(len(utxo_range))
        width = 0.2
        for f_idx, (fee, label) in enumerate(ANNUAL_FEE_LABELS.items()):
            monthly = []
            for utxo in utxo_range:
                r = model_rent(12, fee, utxo,
                               chain_age_label=age["label"], block_reward=age["reward"])
                monthly.append(r.monthly_rent_coins)
            ax.bar(x + f_idx * width - 1.5 * width, monthly, width,
                   label=label, color=FEE_COLORS[label], alpha=0.85)
        ax.set_title(f'{age["label"]} ({age["reward"]}c/block)', fontsize=11, fontweight='bold')
        ax.set_xlabel('UTXO Count')
        ax.set_xticks(x)
        ax.set_xticklabels([f'{u//1000}K' for u in utxo_range], fontsize=8)
        ax.set_yscale('log')
        ax.grid(True, alpha=0.3, axis='y')
        if ax_idx == 0:
            ax.set_ylabel('Monthly Rent Revenue (coins)')
            ax.legend(title='Annual Fee', fontsize=7)
        monthly_miner = age["reward"] * MINER_PCT * BLOCKS_PER_MONTH
        ax.axhline(y=monthly_miner, color='#ffffff', linestyle=':', alpha=0.4)
        ax.text(0.02, monthly_miner * 1.15, f'Block reward: {monthly_miner:,.0f}c/mo',
                transform=ax.get_yaxis_transform(), fontsize=7, color='#aaaaaa', ha='left')
    fig.suptitle('$YOLO — Monthly Rent Revenue to Miners (coins)\n'
                 '(12mo cycle, 30% dormancy, 250B box)',
                 fontsize=13, fontweight='bold', y=1.02)
    plt.tight_layout()
    fig.savefig('chart4_monthly_rent_coins.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart4 — monthly rent revenue")


def chart5_box_consumption():
    """Years until box is fully consumed, by box value and annual fee rate."""
    fig, ax = plt.subplots(figsize=(14, 7))
    box_values = [0.01, 0.1, 1.0, 10.0, 100.0, 1000.0]
    x = np.arange(len(box_values))
    width = 0.2
    for f_idx, (fee, label) in enumerate(ANNUAL_FEE_LABELS.items()):
        years = []
        annual_cost = fee * 250 / 1e9
        for bv in box_values:
            y = bv / annual_cost if annual_cost > 0 else float('inf')
            years.append(min(y, 100_000))
        ax.bar(x + f_idx * width - 1.5 * width, years, width,
               label=label, color=FEE_COLORS[label], alpha=0.85)
    ax.set_xlabel('Box Value (coins)')
    ax.set_ylabel('Years Until Fully Consumed')
    ax.set_title('$YOLO — Box Consumption Timeline\n'
                 '(250B box, dormant from creation, same across all cycle lengths)',
                 fontsize=13, fontweight='bold')
    ax.set_xticks(x)
    ax.set_xticklabels([f'{v}' for v in box_values])
    ax.set_yscale('log')
    ax.axhline(y=50, color='#ff6b6b', linestyle='--', alpha=0.7)
    ax.text(0.02, 55, '50-year horizon', color='#ff6b6b', fontsize=9)
    ax.axhline(y=1, color='#ffd43b', linestyle='--', alpha=0.5)
    ax.text(0.02, 1.15, '1-year mark', color='#ffd43b', fontsize=9)
    ax.legend(title='Annual Fee Rate', loc='upper left')
    ax.grid(True, alpha=0.3, axis='y')
    plt.tight_layout()
    fig.savefig('chart5_box_consumption.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart5 — box consumption timeline")


def chart6_cycle_effect():
    """What cycle length DOES affect: per-event fee and cleanup speed.
    Show per-event cost and eligible boxes per block across cycles."""
    fig, axes = plt.subplots(1, 2, figsize=(16, 7))

    cycles = RENT_CYCLES_MONTHS

    # Left: Per-event cost across cycles (at 1x Ergo annual)
    ax = axes[0]
    per_event = []
    for c in cycles:
        r = model_rent(c, 312_500, 200_000, chain_age_label="Year 1", block_reward=50.0)
        per_event.append(r.per_event_cost_coins)
    bars = ax.bar(range(len(cycles)), per_event, color='#00d4ff', alpha=0.85)
    ax.set_xlabel('Rent Cycle')
    ax.set_ylabel('Per-Event Rent Cost (coins)')
    ax.set_title('Per-Event Fee Scales with Cycle\n(1x Ergo annual, 250B box)',
                 fontsize=11, fontweight='bold')
    ax.set_xticks(range(len(cycles)))
    ax.set_xticklabels([f'{c}mo' for c in cycles])
    ax.grid(True, alpha=0.3, axis='y')
    for bar, val in zip(bars, per_event):
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.001,
                f'{val:.6f}', ha='center', fontsize=9, color='#e0e0e0')

    # Right: Eligible boxes per block (how frequently cleanup fires)
    ax = axes[1]
    utxo_scenarios = [50_000, 200_000, 500_000]
    width = 0.25
    x = np.arange(len(cycles))
    for u_idx, utxo in enumerate(utxo_scenarios):
        eligible = []
        for c in cycles:
            r = model_rent(c, 312_500, utxo, chain_age_label="Year 1", block_reward=50.0)
            eligible.append(r.eligible_boxes_per_block)
        ax.bar(x + u_idx * width - width, eligible, width,
               label=f'{utxo//1000}K UTXOs', color=COLORS[u_idx], alpha=0.85)
    ax.set_xlabel('Rent Cycle')
    ax.set_ylabel('Eligible Boxes per Block')
    ax.set_title('Shorter Cycles = More Frequent Cleanup\n(1x Ergo annual, 30% dormancy)',
                 fontsize=11, fontweight='bold')
    ax.set_xticks(x)
    ax.set_xticklabels([f'{c}mo' for c in cycles])
    ax.legend(title='UTXO Count')
    ax.grid(True, alpha=0.3, axis='y')

    fig.suptitle('$YOLO — What Cycle Length Controls\n'
                 '(annual cost stays constant; cycle affects per-event fee and cleanup frequency)',
                 fontsize=13, fontweight='bold', y=1.02)
    plt.tight_layout()
    fig.savefig('chart6_cycle_effect.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart6 — cycle length effects")


def chart7_byte_vs_value_rent():
    """Byte-based vs value-based rent comparison."""
    fig, axes = plt.subplots(1, 2, figsize=(16, 7))
    profiles = [
        (0.01,  120, "Dust\n0.01c, 120B"),
        (1.0,   200, "Simple\n1c, 200B"),
        (10.0,  250, "Normal\n10c, 250B"),
        (100.0, 300, "Rich\n100c, 300B"),
        (1000.0,400, "Whale\n1000c, 400B"),
    ]
    x = np.arange(len(profiles))

    # Left: Byte-based at 1x Ergo annual
    ax = axes[0]
    costs = [312_500 * size / 1e9 for _, size, _ in profiles]
    bars = ax.bar(x, costs, color='#00d4ff', alpha=0.85)
    ax.set_ylabel('Annual Rent (coins)')
    ax.set_title('Byte-Based Rent\n(1x Ergo annual rate)', fontsize=11, fontweight='bold')
    ax.set_xticks(x)
    ax.set_xticklabels([p[2] for p in profiles], fontsize=8)
    ax.grid(True, alpha=0.3, axis='y')
    for bar, cost in zip(bars, costs):
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() + 0.001,
                f'{cost:.4f}', ha='center', fontsize=9, color='#e0e0e0')

    # Right: Value-based (0.1% of value per year)
    ax = axes[1]
    costs_v = [val * 0.001 for val, _, _ in profiles]
    bars = ax.bar(x, costs_v, color='#ff6b6b', alpha=0.85)
    ax.set_ylabel('Annual Rent (coins)')
    ax.set_title('Value-Based Rent\n(0.1% of value per year)', fontsize=11, fontweight='bold')
    ax.set_xticks(x)
    ax.set_xticklabels([p[2] for p in profiles], fontsize=8)
    ax.set_yscale('log')
    ax.grid(True, alpha=0.3, axis='y')
    for bar, cost in zip(bars, costs_v):
        ax.text(bar.get_x() + bar.get_width()/2, bar.get_height() * 1.3,
                f'{cost:.4f}', ha='center', fontsize=9, color='#e0e0e0')

    fig.suptitle('$YOLO — Byte-Based vs Value-Based Rent',
                 fontsize=13, fontweight='bold', y=1.02)
    plt.tight_layout()
    fig.savefig('chart7_byte_vs_value_rent.png', dpi=150, bbox_inches='tight')
    plt.close()
    print("  ✓ chart7 — byte vs value rent")


if __name__ == "__main__":
    print("Generating sensitivity charts (annual rate model)...")
    chart1_rent_pct_vs_utxo()
    chart2_annual_cost_heatmap()
    chart3_rent_over_chain_life()
    chart4_monthly_rent_coins()
    chart5_box_consumption()
    chart6_cycle_effect()
    chart7_byte_vs_value_rent()
    print("\nAll charts generated.")
