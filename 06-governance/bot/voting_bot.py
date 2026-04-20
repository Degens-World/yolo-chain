"""
voting_bot.py — Off-chain governance voting bot (DuckDAO pattern)

Stateless: reads ALL state from chain. No local database.
Resumable: if bot restarts mid-operation, queries current state and continues.
Multi-bot safe: first valid TX wins mempool. Other bots' attempts fail on
double-spend — harmless.

Responsibilities:
  1. Monitor chain for new proposals (counter box state)
  2. Transition proposals through lifecycle (before-counting → counting →
     validation → new-proposal)
  3. Submit counting TXs for each voter box during counting phase
  4. Handle proposer stake refund (on completion) or forfeit (on cancel)
  5. Execute passed proposals (treasury withdrawal)
  6. Vault heartbeat (reset storage rent clock on idle vault pairs)

Requires:
  - Ergo-compatible node REST API (SigmaChain node)
  - Wallet with YOLO for TX fees
  - Config: node URL, API key, polling interval

Usage:
  python voting_bot.py --config config.yaml
  python voting_bot.py --config config.yaml --once  # single pass, no loop
"""

import argparse
import json
import logging
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

import yaml

# Lazy import — requests not needed for model/tests
try:
    import requests
except ImportError:
    requests = None

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
)
log = logging.getLogger("voting_bot")

# ============================================================
# CONSTANTS (must match counting.es compile-time values)
# ============================================================

VOTING_WINDOW = 12960      # 3 days at 20s blocks
COUNTING_PHASE = 1080      # 6 hours
EXECUTION_GRACE = 4320     # 24 hours
DENOM = 10_000_000         # proportion denominator


# ============================================================
# CONFIG
# ============================================================

@dataclass
class BotConfig:
    node_url: str = "http://localhost:9053"
    api_key: str = ""
    poll_interval_seconds: int = 60
    # Token IDs (populated from PARAMETERS.md after genesis)
    counter_nft_id: str = ""
    treasury_nft_id: str = ""
    proposal_token_id: str = ""
    valid_vote_id: str = ""
    vyolo_id: str = ""
    # Vault pairs for heartbeat
    vault_state_nft_ids: list = field(default_factory=list)
    reserve_nft_ids: list = field(default_factory=list)

    @classmethod
    def from_yaml(cls, path: str) -> "BotConfig":
        with open(path) as f:
            data = yaml.safe_load(f)
        return cls(**{k: v for k, v in data.items() if k in cls.__dataclass_fields__})


# ============================================================
# NODE CLIENT
# ============================================================

class NodeClient:
    """Thin wrapper around Ergo node REST API."""

    def __init__(self, config: BotConfig):
        self.url = config.node_url.rstrip("/")
        self.api_key = config.api_key
        self.session = requests.Session() if requests else None
        if self.session:
            self.session.headers.update({
                "Content-Type": "application/json",
                "api_key": self.api_key,
            })

    def get(self, path: str) -> dict:
        r = self.session.get(f"{self.url}{path}")
        r.raise_for_status()
        return r.json()

    def post(self, path: str, data: dict) -> dict:
        r = self.session.post(f"{self.url}{path}", json=data)
        r.raise_for_status()
        return r.json()

    def current_height(self) -> int:
        info = self.get("/info")
        return info["fullHeight"]

    def get_box_by_token(self, token_id: str) -> Optional[dict]:
        """Find the current unspent box containing a specific token."""
        try:
            boxes = self.get(f"/blockchain/box/unspent/byTokenId/{token_id}")
            if boxes:
                return boxes[0]  # singleton — should be exactly 1
        except Exception as e:
            log.warning(f"Failed to find box for token {token_id[:8]}...: {e}")
        return None

    def get_boxes_by_token(self, token_id: str) -> list:
        """Find all unspent boxes containing a specific token."""
        try:
            return self.get(f"/blockchain/box/unspent/byTokenId/{token_id}")
        except Exception:
            return []

    def submit_tx(self, signed_tx: dict) -> str:
        """Submit a signed transaction. Returns TX ID."""
        result = self.post("/transactions", signed_tx)
        return result if isinstance(result, str) else result.get("id", "")

    def sign_tx(self, unsigned_tx: dict) -> dict:
        """Sign a transaction using the node's wallet."""
        return self.post("/wallet/transaction/sign", unsigned_tx)


# ============================================================
# STATE READER (stateless — all from chain)
# ============================================================

@dataclass
class CounterState:
    """Current state of the counting box, read from chain."""
    box_id: str
    vote_deadline: int       # R4
    proportion: int          # R5._1
    votes_for: int           # R5._2
    recipient_hash: str      # R6 (hex)
    total_votes: int         # R7
    initiation_stake: int    # R8
    validation_votes: int    # R9

    @property
    def counting_end(self) -> int:
        return self.vote_deadline + COUNTING_PHASE

    @property
    def validation_end(self) -> int:
        return self.counting_end + EXECUTION_GRACE

    @property
    def has_active_proposal(self) -> bool:
        return self.total_votes > 0 or self.validation_votes > 0

    def phase_at(self, height: int) -> str:
        if height < self.vote_deadline:
            return "before_counting"
        elif height < self.counting_end:
            return "counting"
        elif height < self.validation_end:
            return "validation"
        else:
            return "new_proposal"


def read_counter_state(client: NodeClient, config: BotConfig) -> Optional[CounterState]:
    """Read the current counter box state from chain."""
    box = client.get_box_by_token(config.counter_nft_id)
    if not box:
        log.error("Counter box not found on chain")
        return None

    try:
        regs = box.get("additionalRegisters", {})
        # Parse registers (node returns serialized values)
        # Actual parsing depends on node API format — placeholder
        return CounterState(
            box_id=box["boxId"],
            vote_deadline=_parse_long(regs.get("R4", "")),
            proportion=_parse_tuple_first(regs.get("R5", "")),
            votes_for=_parse_tuple_second(regs.get("R5", "")),
            recipient_hash=_parse_bytes(regs.get("R6", "")),
            total_votes=_parse_long(regs.get("R7", "")),
            initiation_stake=_parse_long(regs.get("R8", "")),
            validation_votes=_parse_long(regs.get("R9", "")),
        )
    except Exception as e:
        log.error(f"Failed to parse counter state: {e}")
        return None


# ============================================================
# REGISTER PARSING HELPERS (node-specific format)
# ============================================================

def _parse_long(hex_val: str) -> int:
    """Parse a serialized SLong register value. Placeholder — format depends on node."""
    if not hex_val:
        return 0
    # Node returns rendered values; actual parsing needs sigma serialization
    # For now, return 0 as placeholder
    return 0

def _parse_tuple_first(hex_val: str) -> int:
    return 0

def _parse_tuple_second(hex_val: str) -> int:
    return 0

def _parse_bytes(hex_val: str) -> str:
    return hex_val


# ============================================================
# PHASE HANDLERS
# ============================================================

def handle_counting_phase(client: NodeClient, config: BotConfig, state: CounterState, height: int):
    """Process voter boxes during counting phase."""
    log.info(f"Counting phase active. Deadline: {state.counting_end}, current height: {height}")

    # Find all unspent voter boxes (carrying valid_vote_id)
    voter_boxes = client.get_boxes_by_token(config.valid_vote_id)
    if not voter_boxes:
        log.info("No voter boxes to count")
        return

    log.info(f"Found {len(voter_boxes)} voter boxes to count")

    # Process one voter box at a time (sequential counting per DuckDAO pattern)
    for voter_box in voter_boxes:
        try:
            _submit_counting_tx(client, config, state, voter_box)
            log.info(f"Counted voter box {voter_box['boxId'][:8]}...")
            # Re-read state after each count (tallies changed)
            new_state = read_counter_state(client, config)
            if new_state:
                state = new_state
        except Exception as e:
            log.warning(f"Failed to count voter box {voter_box['boxId'][:8]}...: {e}")
            # Continue with next voter box — don't stall on one failure


def handle_validation_phase(client: NodeClient, config: BotConfig, state: CounterState, height: int):
    """Check thresholds and advance proposal if passed."""
    log.info(f"Validation phase. Total votes: {state.total_votes}, yes: {state.validation_votes}")

    quorum_floor = 1_000_000_000_000_000  # 1M vYOLO in nanocoins
    elevated_proportion = 1_000_000

    meets_quorum = state.total_votes >= quorum_floor

    required_support = 900 if state.proportion > elevated_proportion else 500
    actual_support = (state.validation_votes * 1000 // state.total_votes) if state.total_votes > 0 else 0
    meets_support = actual_support >= required_support

    passed = meets_quorum and meets_support

    log.info(f"Quorum: {'MET' if meets_quorum else 'NOT MET'} "
             f"({state.total_votes}/{quorum_floor})")
    log.info(f"Support: {actual_support}/1000 (need {required_support}) — "
             f"{'PASSED' if meets_support else 'FAILED'}")

    if passed:
        log.info("PROPOSAL PASSED — advancing state token 1→2")
        try:
            _submit_validation_tx(client, config, state, passed=True)
        except Exception as e:
            log.error(f"Failed to submit validation TX: {e}")
    else:
        log.info("PROPOSAL FAILED — resetting counter")
        try:
            _submit_validation_tx(client, config, state, passed=False)
        except Exception as e:
            log.error(f"Failed to submit reset TX: {e}")


def handle_new_proposal_phase(client: NodeClient, config: BotConfig, state: CounterState, height: int):
    """Reset counter for next round if tallies are stale."""
    if state.has_active_proposal:
        log.info("New proposal period — resetting stale tallies")
        try:
            _submit_reset_tx(client, config, state)
        except Exception as e:
            log.warning(f"Failed to reset counter: {e}")
    else:
        log.debug("Counter already idle, waiting for new proposal")


def handle_vault_heartbeat(client: NodeClient, config: BotConfig, height: int):
    """Touch idle vault/reserve pairs to reset storage rent clock."""
    for i, nft_id in enumerate(config.vault_state_nft_ids):
        box = client.get_box_by_token(nft_id)
        if not box:
            continue
        creation_height = box.get("creationHeight", 0)
        age_blocks = height - creation_height
        # Touch if older than 6 months (~788,940 blocks at 20s)
        if age_blocks > 788_940:
            log.info(f"Vault pair {i+1} idle for {age_blocks} blocks — touching")
            try:
                _submit_heartbeat_tx(client, config, box)
            except Exception as e:
                log.warning(f"Failed vault heartbeat for pair {i+1}: {e}")


# ============================================================
# TX BUILDERS (stubs — full implementation needs sigma-rust or node signing)
# ============================================================

def _submit_counting_tx(client: NodeClient, config: BotConfig, state: CounterState, voter_box: dict):
    """Build and submit a counting TX for one voter box."""
    # TODO: Build unsigned TX:
    #   INPUTS: counter_box, voter_box
    #   OUTPUTS: updated_counter_box (tallies incremented), vYOLO return to voter
    #   Vote NFT burned (not in any output)
    # Sign via node wallet, submit
    raise NotImplementedError("Counting TX builder — needs node signing integration")


def _submit_validation_tx(client: NodeClient, config: BotConfig, state: CounterState, passed: bool):
    """Build and submit validation TX (advance proposal or reset)."""
    # TODO: Build unsigned TX:
    #   If passed: INPUTS: counter_box, proposal_box
    #              OUTPUTS: reset_counter, advanced_proposal (token qty=2)
    #   If failed: INPUTS: counter_box
    #              OUTPUTS: reset_counter
    raise NotImplementedError("Validation TX builder — needs node signing integration")


def _submit_reset_tx(client: NodeClient, config: BotConfig, state: CounterState):
    """Build and submit counter reset TX (Phase 4)."""
    # INPUTS: counter_box. OUTPUTS: counter_box with R7=0, R9=0.
    raise NotImplementedError("Reset TX builder — needs node signing integration")


def _submit_heartbeat_tx(client: NodeClient, config: BotConfig, vault_box: dict):
    """Build and submit a trivial deposit to reset vault rent clock."""
    # INPUTS: vault_box, reserve_box, fee_box. OUTPUTS: same but recreated.
    raise NotImplementedError("Heartbeat TX builder — needs node signing integration")


# ============================================================
# MAIN LOOP
# ============================================================

def run_once(client: NodeClient, config: BotConfig):
    """Single pass — read state, take action if needed."""
    height = client.current_height()
    log.info(f"Height: {height}")

    state = read_counter_state(client, config)
    if not state:
        log.warning("Could not read counter state — skipping cycle")
        return

    phase = state.phase_at(height)
    log.info(f"Counter phase: {phase} (deadline={state.vote_deadline})")

    if phase == "before_counting":
        log.debug("Before counting — waiting for voting window to close")
    elif phase == "counting":
        handle_counting_phase(client, config, state, height)
    elif phase == "validation":
        handle_validation_phase(client, config, state, height)
    elif phase == "new_proposal":
        handle_new_proposal_phase(client, config, state, height)

    # Vault heartbeat (runs every cycle, checks age internally)
    handle_vault_heartbeat(client, config, height)


def main():
    parser = argparse.ArgumentParser(description="YOLO Governance Voting Bot")
    parser.add_argument("--config", required=True, help="Path to config.yaml")
    parser.add_argument("--once", action="store_true", help="Single pass, no loop")
    args = parser.parse_args()

    if not requests:
        log.error("requests library not installed: pip install requests pyyaml")
        sys.exit(1)

    config = BotConfig.from_yaml(args.config)
    client = NodeClient(config)

    if args.once:
        run_once(client, config)
    else:
        log.info("Starting voting bot (Ctrl+C to stop)")
        while True:
            try:
                run_once(client, config)
            except KeyboardInterrupt:
                log.info("Shutting down")
                break
            except Exception as e:
                log.error(f"Cycle error: {e}")
            time.sleep(config.poll_interval_seconds)


if __name__ == "__main__":
    main()
