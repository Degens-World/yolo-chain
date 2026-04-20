"""
vault_model.py — Reference model for YOLO governance peg layer

This is an INDEPENDENT reimplementation of the vault/reserve peg logic.
It is NOT a translation of the ErgoScript contracts or Rust tests.
If this model and the contracts disagree, investigate both.

The model validates the core invariant:
    sum(circulating_vYOLO) == sum(locked_YOLO_across_all_vaults)

All values in nanocoins (1 coin = 1_000_000_000 nanocoins).
"""

from dataclasses import dataclass, field
from typing import Optional

# ============================================================
# PARAMETERS — from emission_model.py
# ============================================================

NANOCOIN = 1_000_000_000
TOTAL_SUPPLY = 177_412_882_500_000_000  # nanocoins (from emission_model.py)
NUM_VAULT_PAIRS = 5
INITIAL_VYOLO_PER_RESERVE = TOTAL_SUPPLY // NUM_VAULT_PAIRS
MIN_BOX_VALUE = 360_000  # nanocoins (from storage rent spec)

I64_MAX = (2**63) - 1
I64_MIN = -(2**63)


# ============================================================
# STATE
# ============================================================

@dataclass
class VaultBox:
    """Represents a single vault box holding locked YOLO."""
    pair_id: int          # 1-5
    yolo_locked: int      # nanocoins of YOLO locked
    state_nft_id: str     # unique per pair

    def __post_init__(self):
        assert 1 <= self.pair_id <= NUM_VAULT_PAIRS
        assert self.yolo_locked >= 0


@dataclass
class ReserveBox:
    """Represents a single reserve box holding available vYOLO."""
    pair_id: int           # 1-5
    vyolo_available: int   # nanocoins of vYOLO available to dispense
    reserve_nft_id: str    # unique per pair
    erg_value: int = MIN_BOX_VALUE  # fixed ERG value (rent buffer)

    def __post_init__(self):
        assert 1 <= self.pair_id <= NUM_VAULT_PAIRS
        assert self.vyolo_available >= 0


@dataclass
class PegState:
    """Full state of the 5-pair peg layer."""
    vaults: list[VaultBox] = field(default_factory=list)
    reserves: list[ReserveBox] = field(default_factory=list)

    def total_locked_yolo(self) -> int:
        return sum(v.yolo_locked for v in self.vaults)

    def total_available_vyolo(self) -> int:
        return sum(r.vyolo_available for r in self.reserves)

    def total_circulating_vyolo(self) -> int:
        """vYOLO in circulation = total minted - available in reserves."""
        return TOTAL_SUPPLY - self.total_available_vyolo()

    def peg_invariant(self) -> bool:
        """The fundamental invariant: circulating vYOLO == locked YOLO."""
        return self.total_circulating_vyolo() == self.total_locked_yolo()

    def i64_safe(self) -> bool:
        """Check all values fit in i64 (ErgoScript Long)."""
        for v in self.vaults:
            if not (I64_MIN <= v.yolo_locked <= I64_MAX):
                return False
        for r in self.reserves:
            if not (I64_MIN <= r.vyolo_available <= I64_MAX):
                return False
        return True


# ============================================================
# GENESIS
# ============================================================

def create_genesis_state() -> PegState:
    """Create the initial peg state with 5 vault/reserve pairs."""
    state = PegState()
    for i in range(1, NUM_VAULT_PAIRS + 1):
        state.vaults.append(VaultBox(
            pair_id=i,
            yolo_locked=0,
            state_nft_id=f"STATE_NFT_{i}",
        ))
        state.reserves.append(ReserveBox(
            pair_id=i,
            vyolo_available=INITIAL_VYOLO_PER_RESERVE,
            reserve_nft_id=f"RESERVE_NFT_{i}",
        ))
    # Handle remainder from integer division
    remainder = TOTAL_SUPPLY - (INITIAL_VYOLO_PER_RESERVE * NUM_VAULT_PAIRS)
    if remainder > 0:
        state.reserves[0].vyolo_available += remainder
    assert state.peg_invariant(), "Genesis state must satisfy peg invariant"
    assert state.total_available_vyolo() == TOTAL_SUPPLY
    assert state.total_locked_yolo() == 0
    return state


# ============================================================
# OPERATIONS
# ============================================================

def deposit(state: PegState, pair_id: int, amount: int) -> Optional[str]:
    """
    User deposits `amount` nanocoins of YOLO into vault pair `pair_id`,
    receives `amount` vYOLO from the paired reserve.

    Returns None on success, error message on failure.
    """
    if amount <= 0:
        return "deposit amount must be positive (nonTrivial check)"

    vault = state.vaults[pair_id - 1]
    reserve = state.reserves[pair_id - 1]

    if reserve.vyolo_available < amount:
        return f"insufficient vYOLO in reserve {pair_id}: have {reserve.vyolo_available}, need {amount}"

    # Conservation check: deltaVault + deltaReserve == 0
    # deltaVault = +amount (vault gains YOLO)
    # deltaReserve = -amount (reserve loses vYOLO)
    # +amount + (-amount) == 0 ✓
    delta_vault = amount
    delta_reserve = -amount
    assert delta_vault + delta_reserve == 0, "conservation violated"

    vault.yolo_locked += amount
    reserve.vyolo_available -= amount

    assert state.peg_invariant(), "peg invariant violated after deposit"
    return None


def redeem(state: PegState, pair_id: int, amount: int) -> Optional[str]:
    """
    User redeems `amount` vYOLO at reserve pair `pair_id`,
    receives `amount` YOLO from the paired vault.

    Returns None on success, error message on failure.
    """
    if amount <= 0:
        return "redeem amount must be positive (nonTrivial check)"

    vault = state.vaults[pair_id - 1]
    reserve = state.reserves[pair_id - 1]

    if vault.yolo_locked < amount:
        return f"insufficient YOLO in vault {pair_id}: have {vault.yolo_locked}, need {amount}"

    # Conservation check: deltaVault + deltaReserve == 0
    # deltaVault = -amount (vault loses YOLO)
    # deltaReserve = +amount (reserve gains vYOLO)
    delta_vault = -amount
    delta_reserve = amount
    assert delta_vault + delta_reserve == 0, "conservation violated"

    vault.yolo_locked -= amount
    reserve.vyolo_available += amount

    assert state.peg_invariant(), "peg invariant violated after redeem"
    return None


# ============================================================
# VALIDATION HELPERS
# ============================================================

def validate_conservation(
    vault_before: int, vault_after: int,
    reserve_before: int, reserve_after: int,
) -> bool:
    """Validate the conservation law as checked by both contracts."""
    delta_vault = vault_after - vault_before
    delta_reserve = reserve_after - reserve_before
    return delta_vault + delta_reserve == 0 and delta_vault != 0


def validate_self_integrity_vault(tokens_size: int, token0_id: str, token0_qty: int,
                                   expected_nft: str) -> bool:
    """Mirror of vault.es selfValid check."""
    return (tokens_size == 1 and
            token0_id == expected_nft and
            token0_qty == 1)


def validate_self_integrity_reserve(tokens_size: int, token0_id: str, token0_qty: int,
                                     token1_id: str, expected_nft: str,
                                     expected_vyolo: str) -> bool:
    """Mirror of reserve.es selfValid check."""
    return (tokens_size == 2 and
            token0_id == expected_nft and
            token0_qty == 1 and
            token1_id == expected_vyolo)


# ============================================================
# PROPERTY-BASED TEST SUPPORT
# ============================================================

def random_operation_sequence(state: PegState, ops: list[tuple[str, int, int]]) -> list[str]:
    """
    Execute a sequence of (op_type, pair_id, amount) operations.
    Returns list of error messages (empty = all succeeded).

    Used by property-based tests to verify peg invariant holds
    after arbitrary sequences of deposits and redeems.
    """
    errors = []
    for op_type, pair_id, amount in ops:
        if op_type == "deposit":
            err = deposit(state, pair_id, amount)
        elif op_type == "redeem":
            err = redeem(state, pair_id, amount)
        else:
            errors.append(f"unknown op: {op_type}")
            continue
        if err:
            errors.append(f"{op_type}(pair={pair_id}, amt={amount}): {err}")
    return errors


# ============================================================
# REPORTING
# ============================================================

def print_state(state: PegState):
    """Print current peg layer state."""
    print("=" * 60)
    print("PEG LAYER STATE")
    print("=" * 60)
    for i in range(NUM_VAULT_PAIRS):
        v = state.vaults[i]
        r = state.reserves[i]
        print(f"  Pair {v.pair_id}: vault={v.yolo_locked / NANOCOIN:>14,.4f} YOLO"
              f"  reserve={r.vyolo_available / NANOCOIN:>14,.4f} vYOLO")
    print(f"\n  Total locked YOLO:      {state.total_locked_yolo() / NANOCOIN:>14,.4f}")
    print(f"  Total available vYOLO:  {state.total_available_vyolo() / NANOCOIN:>14,.4f}")
    print(f"  Circulating vYOLO:      {state.total_circulating_vyolo() / NANOCOIN:>14,.4f}")
    print(f"  Peg invariant:          {'OK' if state.peg_invariant() else 'VIOLATED'}")
    print(f"  i64 safe:               {'OK' if state.i64_safe() else 'OVERFLOW'}")


def run_demo():
    """Demonstrate deposit/redeem cycle with invariant checks."""
    state = create_genesis_state()
    print("=== GENESIS ===")
    print_state(state)

    # 3 users deposit different amounts into different pairs
    ops = [
        ("deposit", 1, 100 * NANOCOIN),
        ("deposit", 2, 50 * NANOCOIN),
        ("deposit", 3, 25 * NANOCOIN),
    ]
    print("\n=== AFTER DEPOSITS ===")
    errors = random_operation_sequence(state, ops)
    for e in errors:
        print(f"  ERROR: {e}")
    print_state(state)

    # User redeems from pair 1
    ops = [("redeem", 1, 30 * NANOCOIN)]
    print("\n=== AFTER PARTIAL REDEEM ===")
    errors = random_operation_sequence(state, ops)
    for e in errors:
        print(f"  ERROR: {e}")
    print_state(state)

    # Edge case: try to redeem more than locked
    print("\n=== EDGE CASE: OVER-REDEEM ===")
    err = redeem(state, 1, 200 * NANOCOIN)
    print(f"  Result: {err}")

    # Edge case: try zero deposit
    print("\n=== EDGE CASE: ZERO DEPOSIT ===")
    err = deposit(state, 1, 0)
    print(f"  Result: {err}")

    # Verify conservation validation helper
    print("\n=== CONSERVATION VALIDATION ===")
    print(f"  Valid (+10, -10): {validate_conservation(100, 110, 50, 40)}")
    print(f"  Valid (-10, +10): {validate_conservation(110, 100, 40, 50)}")
    print(f"  Invalid (+10, -5): {validate_conservation(100, 110, 50, 45)}")
    print(f"  Invalid (0, 0):   {validate_conservation(100, 100, 50, 50)}")


if __name__ == "__main__":
    run_demo()
