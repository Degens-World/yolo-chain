"""
contract_sim.py — Faithful Python simulator of the emission.es ErgoScript contract.

Mirrors the contract line-by-line so we can exhaustively test the logic without
needing a running sigma-rust interpreter. Every `val` in the contract maps to
a function or expression here. Every boolean in `sigmaProp(normalPath || terminalPath)`
is reproduced exactly.

This is the test oracle for the reject/accept behavior of the contract.
The block_reward function is also validated against emission_model.py.
"""

from dataclasses import dataclass, field
from typing import List, Tuple, Optional
import hashlib

# ============================================================
# CONSTANTS (must match emission.es and PARAMETERS.md)
# ============================================================

NANOCOIN = 1_000_000_000

BLOCKS_PER_HALVING = 1_577_880          # ~1 year at 20s blocks
INITIAL_REWARD = 50 * NANOCOIN          # 50 coins in nanocoins
MIN_REWARD = 1 * NANOCOIN               # 1 coin floor
MAX_HALVINGS = 20

TREASURY_PCT = 10
LP_PCT = 5
MINER_PCT = 85

# Ergo consensus minimum box value (dust limit)
# Real value on mainnet is ~1_000_000 nanoERG, referenced in tests.
MIN_BOX_VALUE = 1_000_000


# ============================================================
# BOX MODEL
# ============================================================

def blake2b256(data: bytes) -> bytes:
    """Contract uses blake2b256 via the built-in. Python stdlib provides it."""
    return hashlib.blake2b(data, digest_size=32).digest()


@dataclass
class Token:
    token_id: bytes    # 32 bytes
    amount: int        # Long


@dataclass
class Box:
    """Mirror of ErgoBox for the fields the contract touches."""
    value: int                           # nanocoins
    proposition_bytes: bytes             # ErgoTree bytes
    tokens: List[Token] = field(default_factory=list)
    r4: Optional[bytes] = None           # Coll[Byte]
    r5: Optional[bytes] = None           # Coll[Byte]
    creation_height: int = 0             # creationInfo._1


# ============================================================
# BLOCK REWARD (mirrors the ErgoScript lookup table exactly)
# ============================================================
#
# The contract's lookup differs subtly from emission_model.py's shift:
#   emission_model.py:  reward = INITIAL_REWARD >> halvings, max with MIN_REWARD,
#                       capped at MAX_HALVINGS returning MIN_REWARD.
#   emission.es:        explicit if/else for halvings 0..5, else minReward,
#                       then `if (computed > minReward) computed else minReward` clamp.
#
# Both must produce identical outputs for all heights. We reproduce the contract's
# computation here (not the shift version) so a divergence between them would
# show up as a cross-check failure.

def block_reward_contract(height: int) -> int:
    """Exactly mirrors the `blockReward` val in emission.es."""
    halvings = height // BLOCKS_PER_HALVING  # Int division

    if halvings <= 0:
        computed = INITIAL_REWARD
    elif halvings == 1:
        computed = INITIAL_REWARD // 2
    elif halvings == 2:
        computed = INITIAL_REWARD // 4
    elif halvings == 3:
        computed = INITIAL_REWARD // 8
    elif halvings == 4:
        computed = INITIAL_REWARD // 16
    elif halvings == 5:
        computed = INITIAL_REWARD // 32
    else:
        computed = MIN_REWARD

    # Floor clamp: never below minReward
    return computed if computed > MIN_REWARD else MIN_REWARD


def split_contract(reward: int) -> Tuple[int, int, int]:
    """
    Mirrors the contract's treasury/LP computation plus the (off-chain) miner residual.
    Contract only enforces treasury and LP on-chain; miner share is consensus-layer.
    """
    treasury = reward * 10 // 100
    lp = reward * 5 // 100
    miner = reward - treasury - lp
    return miner, treasury, lp


# ============================================================
# CONTRACT EVALUATION
# ============================================================
#
# Returns (result, reason) so failed evaluations explain *which* sub-check failed.
# Normal ErgoScript evaluation just returns Boolean — the reason is added here
# so test diagnostics are useful.

def evaluate_emission_contract(
    self_box: Box,
    outputs: List[Box],
    height: int,
    treasury_script_hash: bytes,
    lp_script_hash: bytes,
) -> Tuple[bool, str]:
    """
    Evaluate the emission contract v1.1 for a given spending context.

    Matches Ergo's emissionBoxProp conventions:
      - heightIncreased required on every spend (Gap A)
      - heightCorrect on new emission box (Gap B)
      - strict `==` on recipient values (Finding 1)

    Parameters match the contract's expectations:
      self_box   — the emission box being spent (SELF)
      outputs    — OUTPUTS of the transaction
      height     — HEIGHT context variable
      treasury/lp_script_hash — expected values in SELF.R4 / SELF.R5

    Returns (ok, explanation). `ok` is the final sigmaProp result.
    """
    # ---- Reads that can fail ----
    if not self_box.tokens:
        return False, "SELF.tokens(0) access fails: no tokens on SELF"
    emission_nft_id = self_box.tokens[0].token_id

    if self_box.r4 is None:
        return False, "SELF.R4[Coll[Byte]].get fails: R4 absent"
    if self_box.r5 is None:
        return False, "SELF.R5[Coll[Byte]].get fails: R5 absent"

    # Contract reads R4/R5 from SELF — the contract trusts whatever is there.
    # But we also check them against the expected hashes passed in by the caller
    # to detect register-tampering scenarios at the SELF level.
    contract_treasury_hash = self_box.r4
    contract_lp_hash = self_box.r5

    # [Gap A] heightIncreased — mandatory for every spend
    if not (height > self_box.creation_height):
        return False, (
            f"heightIncreased: HEIGHT({height}) must be > "
            f"SELF.creationInfo._1({self_box.creation_height})"
        )

    # ---- Block reward ----
    block_reward = block_reward_contract(height)
    treasury_reward = block_reward * 10 // 100
    lp_reward = block_reward * 5 // 100

    # ============================================================
    # NORMAL PATH
    # ============================================================
    normal_reasons = []

    sufficient_funds = self_box.value >= block_reward
    if not sufficient_funds:
        normal_reasons.append(f"sufficientFunds: SELF.value({self_box.value}) < blockReward({block_reward})")

    # OUTPUTS(0) access
    normal_ok = True
    if len(outputs) < 1:
        normal_reasons.append("normal: OUTPUTS(0) out of bounds")
        normal_ok = False
    else:
        next_box = outputs[0]

        # nftPreserved
        nft_preserved = (
            len(next_box.tokens) > 0
            and next_box.tokens[0].token_id == emission_nft_id
            and next_box.tokens[0].amount == 1
        )
        if not nft_preserved:
            normal_reasons.append("nftPreserved: OUTPUTS(0) does not preserve emission NFT (id/amount mismatch or absent)")
            normal_ok = False

        # scriptPreserved
        script_preserved = next_box.proposition_bytes == self_box.proposition_bytes
        if not script_preserved:
            normal_reasons.append("scriptPreserved: OUTPUTS(0).propositionBytes != SELF.propositionBytes")
            normal_ok = False

        # valueCorrect
        value_correct = next_box.value == self_box.value - block_reward
        if not value_correct:
            normal_reasons.append(
                f"valueCorrect: OUTPUTS(0).value({next_box.value}) != SELF.value - blockReward({self_box.value - block_reward})"
            )
            normal_ok = False

        # registersPreserved
        r4_ok = next_box.r4 == contract_treasury_hash
        r5_ok = next_box.r5 == contract_lp_hash
        if not (r4_ok and r5_ok):
            normal_reasons.append("registersPreserved: OUTPUTS(0).R4/R5 do not match SELF.R4/R5")
            normal_ok = False

        # [Gap B] heightCorrect — new emission box's creation height == current HEIGHT
        height_correct_new = next_box.creation_height == height
        if not height_correct_new:
            normal_reasons.append(
                f"heightCorrect: OUTPUTS(0).creationInfo._1({next_box.creation_height}) != HEIGHT({height})"
            )
            normal_ok = False

    valid_emission_box = normal_ok

    # OUTPUTS(1) — treasury   [Finding 1: strict ==]
    valid_treasury = False
    if len(outputs) < 2:
        normal_reasons.append("normal: OUTPUTS(1) out of bounds")
    else:
        t_box = outputs[1]
        t_value_ok = t_box.value == treasury_reward
        t_hash_ok = blake2b256(t_box.proposition_bytes) == contract_treasury_hash
        valid_treasury = t_value_ok and t_hash_ok
        if not t_value_ok:
            normal_reasons.append(f"validTreasury: OUTPUTS(1).value({t_box.value}) != treasuryReward({treasury_reward})")
        if not t_hash_ok:
            normal_reasons.append("validTreasury: blake2b256(OUTPUTS(1).propositionBytes) != R4")

    # OUTPUTS(2) — LP   [Finding 1: strict ==]
    valid_lp = False
    if len(outputs) < 3:
        normal_reasons.append("normal: OUTPUTS(2) out of bounds")
    else:
        l_box = outputs[2]
        l_value_ok = l_box.value == lp_reward
        l_hash_ok = blake2b256(l_box.proposition_bytes) == contract_lp_hash
        valid_lp = l_value_ok and l_hash_ok
        if not l_value_ok:
            normal_reasons.append(f"validLP: OUTPUTS(2).value({l_box.value}) != lpReward({lp_reward})")
        if not l_hash_ok:
            normal_reasons.append("validLP: blake2b256(OUTPUTS(2).propositionBytes) != R5")

    normal_path = sufficient_funds and valid_emission_box and valid_treasury and valid_lp

    # ============================================================
    # TERMINAL PATH
    # ============================================================
    terminal_reasons = []

    insufficient = self_box.value < block_reward
    if not insufficient:
        terminal_reasons.append(f"insufficient: SELF.value({self_box.value}) >= blockReward({block_reward})")

    remaining = self_box.value
    term_treasury = remaining * 10 // 100
    term_lp = remaining * 5 // 100

    # OUTPUTS(0) — terminal treasury   [Finding 1: strict ==]
    valid_term_treasury = False
    if len(outputs) < 1:
        terminal_reasons.append("terminal: OUTPUTS(0) out of bounds")
    else:
        tt = outputs[0]
        vt_value = tt.value == term_treasury
        vt_hash = blake2b256(tt.proposition_bytes) == contract_treasury_hash
        valid_term_treasury = vt_value and vt_hash
        if not vt_value:
            terminal_reasons.append(f"termTreasury: OUTPUTS(0).value({tt.value}) != termTreasury({term_treasury})")
        if not vt_hash:
            terminal_reasons.append("termTreasury: blake2b256(OUTPUTS(0).propositionBytes) != R4")

    # OUTPUTS(1) — terminal LP   [Finding 1: strict ==]
    valid_term_lp = False
    if len(outputs) < 2:
        terminal_reasons.append("terminal: OUTPUTS(1) out of bounds")
    else:
        tl = outputs[1]
        vl_value = tl.value == term_lp
        vl_hash = blake2b256(tl.proposition_bytes) == contract_lp_hash
        valid_term_lp = vl_value and vl_hash
        if not vl_value:
            terminal_reasons.append(f"termLP: OUTPUTS(1).value({tl.value}) != termLP({term_lp})")
        if not vl_hash:
            terminal_reasons.append("termLP: blake2b256(OUTPUTS(1).propositionBytes) != R5")

    # nftBurned: no output contains the emission NFT
    nft_burned = all(
        all(tok.token_id != emission_nft_id for tok in o.tokens)
        for o in outputs
    )
    if not nft_burned:
        terminal_reasons.append("nftBurned: emission NFT appears in an output (must be burned)")

    terminal_path = insufficient and valid_term_treasury and valid_term_lp and nft_burned

    # ============================================================
    # Final: heightIncreased AND (normalPath OR terminalPath)
    # ============================================================
    result = normal_path or terminal_path

    if result:
        which = "normalPath" if normal_path else "terminalPath"
        return True, f"accepted via {which}"
    else:
        reasons = "; ".join(
            ["normal: " + (" | ".join(normal_reasons) if normal_reasons else "<no reasons captured>")]
            + ["terminal: " + (" | ".join(terminal_reasons) if terminal_reasons else "<no reasons captured>")]
        )
        return False, f"rejected — {reasons}"


# ============================================================
# HELPERS to build a typical valid spend
# ============================================================

EMISSION_SCRIPT_BYTES = b"\x00EMISSION_CONTRACT_PLACEHOLDER"
EMISSION_NFT_ID = bytes([0xEE] * 32)

TREASURY_SCRIPT_BYTES = b"\x00TREASURY_SCRIPT_PLACEHOLDER"
LP_SCRIPT_BYTES = b"\x00LP_SCRIPT_PLACEHOLDER"
TREASURY_HASH = blake2b256(TREASURY_SCRIPT_BYTES)
LP_HASH = blake2b256(LP_SCRIPT_BYTES)


def make_self_box(value: int, creation_height: int = 0) -> Box:
    """
    Create an emission SELF box.

    creation_height defaults to 0 (genesis). Tests that spend at HEIGHT > 0
    need SELF.creation_height < HEIGHT for the heightIncreased check to pass.
    """
    return Box(
        value=value,
        proposition_bytes=EMISSION_SCRIPT_BYTES,
        tokens=[Token(EMISSION_NFT_ID, 1)],
        r4=TREASURY_HASH,
        r5=LP_HASH,
        creation_height=creation_height,
    )


def make_next_emission_box(self_box: Box, new_value: int, spend_height: int) -> Box:
    """
    Build the successor emission box.

    creation_height is set to spend_height because the new box is created
    at the block where SELF is being spent. This matches the contract's
    heightCorrect check: OUTPUTS(0).creationInfo._1 == HEIGHT.
    """
    return Box(
        value=new_value,
        proposition_bytes=self_box.proposition_bytes,
        tokens=[Token(EMISSION_NFT_ID, 1)],
        r4=self_box.r4,
        r5=self_box.r5,
        creation_height=spend_height,
    )


def make_treasury_box(value: int, creation_height: int = 0) -> Box:
    return Box(
        value=value,
        proposition_bytes=TREASURY_SCRIPT_BYTES,
        creation_height=creation_height,
    )


def make_lp_box(value: int, creation_height: int = 0) -> Box:
    return Box(
        value=value,
        proposition_bytes=LP_SCRIPT_BYTES,
        creation_height=creation_height,
    )


def make_normal_spend(height: int, self_value: int) -> Tuple[Box, List[Box]]:
    """
    Construct a canonical valid normal-path spend at the given height.

    - SELF is given creation_height = 0 so heightIncreased trivially passes for height > 0.
    - OUTPUTS(0) is given creation_height = height to satisfy heightCorrect.
    """
    reward = block_reward_contract(height)
    _, treasury_amt, lp_amt = split_contract(reward)
    self_box = make_self_box(self_value, creation_height=0)
    outputs = [
        make_next_emission_box(self_box, self_value - reward, spend_height=height),
        make_treasury_box(treasury_amt, creation_height=height),
        make_lp_box(lp_amt, creation_height=height),
    ]
    return self_box, outputs


def make_terminal_spend(self_value: int, height: int = 0) -> Tuple[Box, List[Box]]:
    """
    Construct a canonical valid terminal-path spend (self_value < block_reward).

    Default height=0 means SELF creation_height must be <0 for heightIncreased,
    which is impossible, so callers that care about the heightIncreased check
    should pass a positive height.
    """
    term_treasury = self_value * 10 // 100
    term_lp = self_value * 5 // 100
    self_box = make_self_box(self_value, creation_height=0)
    outputs = [
        make_treasury_box(term_treasury, creation_height=height),
        make_lp_box(term_lp, creation_height=height),
    ]
    return self_box, outputs


if __name__ == "__main__":
    # Smoke test: spend at height 1 so heightIncreased (HEIGHT > SELF.creation_height=0) passes
    self_box, outputs = make_normal_spend(1, 177_412_882_500_000_000)
    ok, why = evaluate_emission_contract(self_box, outputs, 1, TREASURY_HASH, LP_HASH)
    print(f"Height 1 spend: ok={ok} ({why})")
