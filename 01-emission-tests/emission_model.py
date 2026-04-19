"""
emission_model.py — Reference emission model for SigmaChain

This is the source of truth. The ErgoScript contract and all tests
are validated against this model. If this model and the contract
disagree, investigate both — but this model defines intent.

All values in nanocoins (1 coin = 1_000_000_000 nanocoins).
"""

# ============================================================
# PARAMETERS — change these, everything else recalculates
# ============================================================

NANOCOIN = 1_000_000_000

BLOCK_TIME_SECONDS = 20
BLOCKS_PER_YEAR = int(365.25 * 24 * 3600 / BLOCK_TIME_SECONDS)  # 1,577,880

INITIAL_REWARD = 50 * NANOCOIN          # 50 coins/block in nanocoins
BLOCKS_PER_HALVING = BLOCKS_PER_YEAR    # halves every ~1 year
MAX_HALVINGS = 20                        # after this, minimum reward applies
MIN_REWARD = 1 * NANOCOIN               # 1 coin — floor reward

# Split percentages (must sum to 100)
MINER_PCT = 85
TREASURY_PCT = 10
# LP gets remainder: 100 - 85 - 10 = 5

# ============================================================
# CORE FUNCTIONS
# ============================================================

def block_reward(height: int) -> int:
    """Block reward in nanocoins at a given height."""
    halvings = height // BLOCKS_PER_HALVING
    if halvings >= MAX_HALVINGS:
        return MIN_REWARD
    reward = INITIAL_REWARD >> halvings  # integer right-shift = divide by 2^n
    return max(reward, MIN_REWARD)


def split_reward(reward: int) -> tuple[int, int, int]:
    """
    Split block reward into (miner, treasury, lp) in nanocoins.
    LP gets remainder to avoid rounding loss.
    """
    miner = reward * MINER_PCT // 100
    treasury = reward * TREASURY_PCT // 100
    lp = reward - miner - treasury
    return miner, treasury, lp


def total_supply() -> int:
    """
    Calculate exact total supply by summing every halving epoch.
    Does NOT simulate block-by-block — uses epoch math.
    """
    total = 0
    for h in range(MAX_HALVINGS):
        reward = INITIAL_REWARD >> h
        if reward < MIN_REWARD:
            # remaining epochs all pay MIN_REWARD
            remaining_epochs = MAX_HALVINGS - h
            # assume chain runs ~100 years max for tail emission
            # (in practice, tail emission is open-ended)
            total += MIN_REWARD * BLOCKS_PER_HALVING * remaining_epochs
            break
        total += reward * BLOCKS_PER_HALVING
    # Tail emission after MAX_HALVINGS (open-ended, but bound for display)
    # Not included — total_supply() returns the sum of the first MAX_HALVINGS epochs only
    return total


def genesis_box_value() -> int:
    """
    How many nanocoins the genesis emission box must hold.
    Equal to total supply across all halving epochs.
    """
    return total_supply()


def emission_box_value_at(height: int) -> int:
    """
    Expected emission box value at a given height.
    Starts at genesis_box_value(), decreases by block_reward each block.
    """
    value = genesis_box_value()
    # Sum rewards for all complete epochs before current
    epoch = height // BLOCKS_PER_HALVING
    blocks_in_partial = height % BLOCKS_PER_HALVING

    for h in range(min(epoch, MAX_HALVINGS)):
        reward = INITIAL_REWARD >> h
        reward = max(reward, MIN_REWARD)
        value -= reward * BLOCKS_PER_HALVING

    # Partial epoch
    if epoch < MAX_HALVINGS:
        reward = INITIAL_REWARD >> epoch
        reward = max(reward, MIN_REWARD)
    else:
        reward = MIN_REWARD
    value -= reward * blocks_in_partial

    return value


def final_emission_height() -> int:
    """
    Height at which emission box reaches 0 or below.
    After this, no more emission (or tail emission from fees/rent).
    """
    remaining = genesis_box_value()
    height = 0
    for h in range(MAX_HALVINGS):
        reward = INITIAL_REWARD >> h
        if reward < MIN_REWARD:
            reward = MIN_REWARD
        epoch_total = reward * BLOCKS_PER_HALVING
        if remaining <= epoch_total:
            # exhaustion happens in this epoch
            blocks_left = remaining // reward
            return height + blocks_left
        remaining -= epoch_total
        height += BLOCKS_PER_HALVING
    # tail emission — will exhaust eventually if MIN_REWARD > 0
    if MIN_REWARD > 0:
        blocks_left = remaining // MIN_REWARD
        return height + blocks_left
    return height


# ============================================================
# VALIDATION HELPERS
# ============================================================

def validate_split(height: int) -> bool:
    """Verify miner + treasury + lp == block_reward at height."""
    reward = block_reward(height)
    miner, treasury, lp = split_reward(reward)
    return miner + treasury + lp == reward


def validate_no_overflow(height: int) -> bool:
    """Verify no intermediate calculation exceeds i64 range."""
    reward = block_reward(height)
    max_i64 = (2**63) - 1
    # Worst case intermediate: reward * 85
    return (reward * MINER_PCT) < max_i64


# ============================================================
# REPORTING
# ============================================================

def print_schedule():
    """Print the full emission schedule."""
    print("=" * 72)
    print("EMISSION SCHEDULE")
    print("=" * 72)
    print(f"Block time:          {BLOCK_TIME_SECONDS}s")
    print(f"Blocks per year:     {BLOCKS_PER_YEAR:,}")
    print(f"Blocks per halving:  {BLOCKS_PER_HALVING:,}")
    print(f"Initial reward:      {INITIAL_REWARD / NANOCOIN} coins")
    print(f"Min reward:          {MIN_REWARD / NANOCOIN} coins")
    print(f"Max halvings:        {MAX_HALVINGS}")
    print(f"Split:               {MINER_PCT}% miner / {TREASURY_PCT}% treasury / {100-MINER_PCT-TREASURY_PCT}% LP")
    print()

    print(f"{'Epoch':<6} {'Year':<6} {'Height Range':<28} {'Reward/Block':<16} {'Epoch Supply':<20} {'Cumulative':<20}")
    print("-" * 100)

    cumulative = 0
    for h in range(MAX_HALVINGS):
        reward = INITIAL_REWARD >> h
        if reward < MIN_REWARD:
            reward = MIN_REWARD
        start = h * BLOCKS_PER_HALVING
        end = (h + 1) * BLOCKS_PER_HALVING - 1
        epoch_supply = reward * BLOCKS_PER_HALVING
        cumulative += epoch_supply
        coins = reward / NANOCOIN
        epoch_coins = epoch_supply / NANOCOIN
        cum_coins = cumulative / NANOCOIN

        print(f"{h:<6} {h+1:<6} {start:>12,} - {end:>12,}  {coins:>12.4f}    {epoch_coins:>16,.1f}  {cum_coins:>16,.1f}")

        if reward == MIN_REWARD:
            print(f"  ... (minimum reward reached, tail emission continues)")
            break

    print()
    print(f"Genesis box value:   {genesis_box_value() / NANOCOIN:,.1f} coins")
    print(f"Final emission ~height: {final_emission_height():,}")
    print(f"Final emission ~year:   {final_emission_height() / BLOCKS_PER_YEAR:.1f}")
    print()

    # Validate splits at every halving boundary
    print("SPLIT VALIDATION")
    print("-" * 72)
    for h in range(min(MAX_HALVINGS + 1, 22)):
        height = h * BLOCKS_PER_HALVING
        reward = block_reward(height)
        miner, treasury, lp = split_reward(reward)
        ok = "OK" if miner + treasury + lp == reward else "FAIL"
        print(f"  Height {height:>12,}: reward={reward/NANOCOIN:>10.4f}  "
              f"miner={miner/NANOCOIN:>10.4f}  treasury={treasury/NANOCOIN:>10.4f}  "
              f"lp={lp/NANOCOIN:>10.4f}  sum_check={ok}")

    # Overflow check
    print()
    print("OVERFLOW VALIDATION")
    print("-" * 72)
    ok = validate_no_overflow(0)
    print(f"  Height 0 (max reward): {'OK' if ok else 'FAIL'}")
    print(f"  Max intermediate value: {INITIAL_REWARD * MINER_PCT:,} (i64 max: {(2**63)-1:,})")


def print_halving_boundaries():
    """Print detailed values at every halving boundary ±1 block."""
    print()
    print("HALVING BOUNDARY DETAIL")
    print("=" * 72)
    for h in range(min(6, MAX_HALVINGS)):
        boundary = h * BLOCKS_PER_HALVING
        for offset in [-1, 0, 1]:
            height = boundary + offset
            if height < 0:
                continue
            reward = block_reward(height)
            miner, treasury, lp = split_reward(reward)
            box_val = emission_box_value_at(height)
            print(f"  H={height:>12,}  reward={reward/NANOCOIN:>10.4f}  "
                  f"miner={miner/NANOCOIN:>10.4f}  treasury={treasury/NANOCOIN:>10.4f}  "
                  f"lp={lp/NANOCOIN:>10.4f}  box={box_val/NANOCOIN:>16,.1f}")
        print()


if __name__ == "__main__":
    print_schedule()
    print_halving_boundaries()
