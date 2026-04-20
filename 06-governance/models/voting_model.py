"""
voting_model.py — Reference model for YOLO governance voting lifecycle

Independent reimplementation of the voting logic (NOT a translation of
the ErgoScript contracts or Rust tests). If this model and the contracts
disagree, investigate both.

Simulates the full lifecycle:
  deposit → propose → vote → count → validate → execute → redeem

All values in nanocoins (1 coin = 1_000_000_000 nanocoins).
"""

from dataclasses import dataclass, field
from typing import Optional
from enum import Enum

NANOCOIN = 1_000_000_000
DENOM = 10_000_000           # proportion denominator
VOTING_WINDOW = 12960        # 3 days at 20s blocks
COUNTING_PHASE = 1080        # 6 hours
EXECUTION_GRACE = 4320       # 24 hours
QUORUM_FLOOR = 1_000_000 * NANOCOIN    # 1M vYOLO
INITIATION_HURDLE = 100_000 * NANOCOIN  # 100k vYOLO
ELEVATED_PROPORTION = 1_000_000         # >10% of treasury
MINIMUM_SUPPORT = 500        # 50% (out of 1000)
ELEVATED_SUPPORT = 900       # 90% (out of 1000)


class Phase(Enum):
    BEFORE_COUNTING = "before_counting"
    COUNTING = "counting"
    VALIDATION = "validation"
    NEW_PROPOSAL = "new_proposal"


@dataclass
class Vote:
    voter_id: str
    vyolo_amount: int
    direction: int  # 1=yes, 0=no
    counted: bool = False


@dataclass
class Proposal:
    proposal_id: str
    proportion: int          # out of DENOM
    recipient: str
    proposer_id: str
    stake: int               # vYOLO staked
    state: int = 1           # 1=pending, 2=passed
    cancelled: bool = False


@dataclass
class CounterState:
    vote_deadline: int = 0
    proportion: int = 0
    votes_for: int = 0
    recipient_hash: str = ""
    total_votes: int = 0
    initiation_stake: int = 0
    validation_votes: int = 0

    def phase_at(self, height: int) -> Phase:
        counting_end = self.vote_deadline + COUNTING_PHASE
        validation_end = counting_end + EXECUTION_GRACE
        if height < self.vote_deadline:
            return Phase.BEFORE_COUNTING
        elif height < counting_end:
            return Phase.COUNTING
        elif height < validation_end:
            return Phase.VALIDATION
        else:
            return Phase.NEW_PROPOSAL

    @property
    def has_active_proposal(self) -> bool:
        return self.total_votes > 0 or self.validation_votes > 0


@dataclass
class GovernanceState:
    """Full governance system state."""
    height: int = 0
    treasury_value: int = 0
    counter: CounterState = field(default_factory=CounterState)
    proposals: list = field(default_factory=list)
    votes: list = field(default_factory=list)
    executed_disbursements: list = field(default_factory=list)

    def advance_height(self, new_height: int):
        assert new_height > self.height, "height must advance"
        self.height = new_height


# ============================================================
# OPERATIONS
# ============================================================

def initiate_proposal(
    state: GovernanceState,
    proposal_id: str,
    proportion: int,
    recipient: str,
    proposer_id: str,
    stake: int,
) -> Optional[str]:
    """Phase 1: initiate a new proposal."""
    phase = state.counter.phase_at(state.height)
    if phase != Phase.BEFORE_COUNTING:
        return f"wrong phase: {phase.value} (need before_counting)"
    if state.counter.has_active_proposal:
        return "active proposal exists (re-initiation blocked)"
    if stake < INITIATION_HURDLE:
        return f"insufficient stake: {stake} < {INITIATION_HURDLE}"
    if proportion < 1 or proportion > DENOM:
        return f"proportion out of range: {proportion}"

    state.counter.vote_deadline = state.height + VOTING_WINDOW
    state.counter.proportion = proportion
    state.counter.votes_for = 0
    state.counter.recipient_hash = recipient
    state.counter.total_votes = 0
    state.counter.initiation_stake = stake
    state.counter.validation_votes = 0

    proposal = Proposal(
        proposal_id=proposal_id,
        proportion=proportion,
        recipient=recipient,
        proposer_id=proposer_id,
        stake=stake,
    )
    state.proposals.append(proposal)
    return None


def cast_vote(state: GovernanceState, voter_id: str, vyolo_amount: int, direction: int) -> Optional[str]:
    """Create a voter box (during voting window)."""
    if direction not in (0, 1):
        return f"invalid direction: {direction}"
    if vyolo_amount <= 0:
        return "vote amount must be positive"

    # Voting window: [vote_deadline - VOTING_WINDOW, vote_deadline)
    voting_start = state.counter.vote_deadline - VOTING_WINDOW
    if state.height < voting_start or state.height >= state.counter.vote_deadline:
        return f"outside voting window [{voting_start}, {state.counter.vote_deadline})"

    vote = Vote(voter_id=voter_id, vyolo_amount=vyolo_amount, direction=direction)
    state.votes.append(vote)
    return None


def count_vote(state: GovernanceState, vote_idx: int) -> Optional[str]:
    """Phase 2: count one vote (burn vote NFT, update tallies)."""
    phase = state.counter.phase_at(state.height)
    if phase != Phase.COUNTING:
        return f"wrong phase: {phase.value} (need counting)"

    if vote_idx >= len(state.votes):
        return f"invalid vote index: {vote_idx}"

    vote = state.votes[vote_idx]
    if vote.counted:
        return "vote already counted (NFT burned)"

    vote.counted = True
    state.counter.total_votes += vote.vyolo_amount
    if vote.direction == 1:
        state.counter.votes_for += vote.vyolo_amount
        state.counter.validation_votes += vote.vyolo_amount

    return None


def validate_proposal(state: GovernanceState) -> tuple[bool, str]:
    """Phase 3: check thresholds, return (passed, reason)."""
    phase = state.counter.phase_at(state.height)
    if phase != Phase.VALIDATION:
        return False, f"wrong phase: {phase.value} (need validation)"

    meets_quorum = state.counter.total_votes >= QUORUM_FLOOR
    if not meets_quorum:
        return False, f"quorum not met: {state.counter.total_votes} < {QUORUM_FLOOR}"

    required = ELEVATED_SUPPORT if state.counter.proportion > ELEVATED_PROPORTION else MINIMUM_SUPPORT
    actual = (state.counter.validation_votes * 1000 // state.counter.total_votes) if state.counter.total_votes > 0 else 0

    if actual < required:
        return False, f"support not met: {actual}/1000 < {required}/1000"

    # Advance proposal state 1→2
    active = [p for p in state.proposals if p.state == 1 and not p.cancelled]
    if active:
        active[-1].state = 2

    return True, f"passed with {actual}/1000 support ({state.counter.total_votes} total votes)"


def execute_proposal(state: GovernanceState, proposal_idx: int) -> Optional[str]:
    """Execute a passed proposal (treasury withdrawal)."""
    proposal = state.proposals[proposal_idx]
    if proposal.state != 2:
        return f"proposal not passed (state={proposal.state})"

    awarded = split_math(state.treasury_value, proposal.proportion)
    if awarded > state.treasury_value:
        return f"awarded ({awarded}) exceeds treasury ({state.treasury_value})"

    state.treasury_value -= awarded
    state.executed_disbursements.append({
        "proposal_id": proposal.proposal_id,
        "recipient": proposal.recipient,
        "proportion": proposal.proportion,
        "awarded": awarded,
    })

    # Reset counter
    state.counter.total_votes = 0
    state.counter.validation_votes = 0

    return None


def reset_counter(state: GovernanceState) -> Optional[str]:
    """Phase 4: reset counter for next round."""
    phase = state.counter.phase_at(state.height)
    if phase != Phase.NEW_PROPOSAL:
        return f"wrong phase: {phase.value} (need new_proposal)"

    state.counter.total_votes = 0
    state.counter.validation_votes = 0
    return None


# ============================================================
# SPLIT-MATH (mirrors ErgoScript implementation)
# ============================================================

def split_math(value: int, proportion: int) -> int:
    """Overflow-safe proportional calculation matching treasury.es."""
    whole = (value // DENOM) * proportion
    remainder = ((value % DENOM) * proportion) // DENOM
    return whole + remainder


# ============================================================
# FULL LIFECYCLE SIMULATION
# ============================================================

def run_lifecycle():
    """Simulate a complete governance cycle."""
    # Initialize with counter deadline far in the future (fresh genesis counter)
    counter = CounterState(vote_deadline=100_000)
    state = GovernanceState(height=100, treasury_value=1_000_000 * NANOCOIN, counter=counter)

    print("=" * 60)
    print("GOVERNANCE LIFECYCLE SIMULATION")
    print("=" * 60)
    print(f"Initial treasury: {state.treasury_value / NANOCOIN:,.1f} YOLO")
    print()

    # Phase 1: Initiate proposal (5% of treasury)
    print("--- Phase 1: Initiate proposal ---")
    err = initiate_proposal(state, "PROP-001", 500_000, "recipient_addr", "proposer_1", INITIATION_HURDLE)
    print(f"  Result: {'OK' if not err else err}")
    print(f"  Vote deadline: {state.counter.vote_deadline}")
    print()

    # Voting window
    print("--- Voting window ---")
    state.advance_height(state.counter.vote_deadline - VOTING_WINDOW + 100)

    voters = [
        ("alice", 500_000 * NANOCOIN, 1),   # yes
        ("bob", 300_000 * NANOCOIN, 1),      # yes
        ("carol", 200_000 * NANOCOIN, 0),    # no
        ("dave", 100_000 * NANOCOIN, 1),     # yes
    ]
    for voter_id, amount, direction in voters:
        err = cast_vote(state, voter_id, amount, direction)
        print(f"  {voter_id}: {amount/NANOCOIN:,.0f} vYOLO {'yes' if direction else 'no'} — {'OK' if not err else err}")
    print()

    # Phase 2: Counting
    print("--- Phase 2: Counting ---")
    state.advance_height(state.counter.vote_deadline)
    for i in range(len(state.votes)):
        err = count_vote(state, i)
        v = state.votes[i]
        print(f"  Count {v.voter_id}: {'OK' if not err else err}")
    print(f"  Total: {state.counter.total_votes/NANOCOIN:,.0f} vYOLO")
    print(f"  Yes:   {state.counter.validation_votes/NANOCOIN:,.0f} vYOLO")
    print()

    # Phase 3: Validation
    print("--- Phase 3: Validation ---")
    state.advance_height(state.counter.vote_deadline + COUNTING_PHASE)
    passed, reason = validate_proposal(state)
    print(f"  Result: {'PASSED' if passed else 'FAILED'} — {reason}")
    print()

    # Execute
    if passed:
        print("--- Execute proposal ---")
        awarded = split_math(state.treasury_value, 500_000)
        err = execute_proposal(state, 0)
        print(f"  Result: {'OK' if not err else err}")
        print(f"  Awarded: {awarded/NANOCOIN:,.1f} YOLO (5% of {(state.treasury_value + awarded)/NANOCOIN:,.1f})")
        print(f"  Treasury after: {state.treasury_value/NANOCOIN:,.1f} YOLO")
        print()

    # Phase 4: Reset for next round
    print("--- Phase 4: Reset counter ---")
    state.advance_height(state.counter.vote_deadline + COUNTING_PHASE + EXECUTION_GRACE)
    err = reset_counter(state)
    print(f"  Result: {'OK' if not err else err}")
    print()

    # Summary
    print("=" * 60)
    print("LIFECYCLE COMPLETE")
    print(f"  Proposals:     {len(state.proposals)}")
    print(f"  Votes cast:    {len(state.votes)}")
    print(f"  Disbursements: {len(state.executed_disbursements)}")
    print(f"  Treasury:      {state.treasury_value/NANOCOIN:,.1f} YOLO")

    # Validate split-math
    print()
    print("--- Split-math validation ---")
    test_cases = [
        (1_000_000 * NANOCOIN, 500_000),   # 5% of 1M
        (177_412_882 * NANOCOIN, 5_000_000),  # 50% of max supply
        (17_741_288 * NANOCOIN, 10_000_000),  # 100% (new-treasury-mode)
        (1 * NANOCOIN, 1),                     # minimum proportion
    ]
    for value, proportion in test_cases:
        awarded = split_math(value, proportion)
        pct = proportion / DENOM * 100
        print(f"  {value/NANOCOIN:>15,.1f} YOLO × {pct:>7.3f}% = {awarded/NANOCOIN:>15,.4f} YOLO")

    # Validate tiered thresholds
    print()
    print("--- Tiered threshold validation ---")
    threshold_cases = [
        (100_000, "normal", MINIMUM_SUPPORT),
        (500_000, "normal", MINIMUM_SUPPORT),
        (1_000_000, "normal", MINIMUM_SUPPORT),
        (1_000_001, "elevated", ELEVATED_SUPPORT),
        (5_000_000, "elevated", ELEVATED_SUPPORT),
        (10_000_000, "elevated (new-treasury)", ELEVATED_SUPPORT),
    ]
    for proportion, label, expected in threshold_cases:
        actual = ELEVATED_SUPPORT if proportion > ELEVATED_PROPORTION else MINIMUM_SUPPORT
        ok = "OK" if actual == expected else "FAIL"
        print(f"  proportion={proportion:>10,} ({label:>20s}) → {actual}/1000 required [{ok}]")


if __name__ == "__main__":
    run_lifecycle()
