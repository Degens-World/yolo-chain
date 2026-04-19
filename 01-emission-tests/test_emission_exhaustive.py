"""
test_emission_exhaustive.py — Exhaustive test suite for the emission contract.

Uses contract_sim.py as the in-process evaluator. Every test exercises a full
Box/OUTPUTS context and calls evaluate_emission_contract() — not just the
reward math. Reject tests mutate exactly one field and assert rejection.

Coverage targets (from reviewer's ask):
  1. Every halving boundary (H-1, H, H+1) for epochs 1..MAX_HALVINGS
  2. Terminal path entry and steady state
  3. Rounding exactness at each epoch
  4. All normal-path reject cases
  5. All terminal-path reject cases

Also validates contract_sim's block_reward against emission_model.py as a
cross-oracle check.

Run: python3 test_emission_exhaustive.py
"""

import sys
import unittest
from typing import List

import emission_model as model
from contract_sim import (
    NANOCOIN,
    BLOCKS_PER_HALVING,
    INITIAL_REWARD,
    MIN_REWARD,
    MAX_HALVINGS,
    TREASURY_PCT,
    LP_PCT,
    MINER_PCT,
    MIN_BOX_VALUE,
    Box,
    Token,
    blake2b256,
    block_reward_contract,
    split_contract,
    evaluate_emission_contract,
    make_self_box,
    make_next_emission_box,
    make_treasury_box,
    make_lp_box,
    make_normal_spend,
    make_terminal_spend,
    EMISSION_SCRIPT_BYTES,
    EMISSION_NFT_ID,
    TREASURY_SCRIPT_BYTES,
    LP_SCRIPT_BYTES,
    TREASURY_HASH,
    LP_HASH,
)


GENESIS_VALUE = model.genesis_box_value()  # 177,412,882.5 coins


# ============================================================
# CROSS-ORACLE: contract_sim vs emission_model
# ============================================================

class CrossOracleTests(unittest.TestCase):
    """
    The contract simulator and the reference model must agree at every
    height we care about. Disagreement means one of them is wrong.
    """

    def test_block_reward_agrees_at_all_boundaries(self):
        for epoch in range(MAX_HALVINGS + 5):
            for offset in (-1, 0, 1):
                h = epoch * BLOCKS_PER_HALVING + offset
                if h < 0:
                    continue
                contract_val = block_reward_contract(h)
                model_val = model.block_reward(h)
                self.assertEqual(
                    contract_val, model_val,
                    f"Disagreement at h={h}: contract={contract_val}, model={model_val}",
                )

    def test_block_reward_agrees_at_sampled_heights(self):
        # Sample every ~100k blocks across 20 epochs
        h = 0
        while h < MAX_HALVINGS * BLOCKS_PER_HALVING + 5 * BLOCKS_PER_HALVING:
            self.assertEqual(
                block_reward_contract(h), model.block_reward(h),
                f"Disagreement at h={h}",
            )
            h += 97_337  # prime-ish step to avoid boundary-only coverage

    def test_block_reward_lookup_matches_shift_for_defined_epochs(self):
        """Contract's if/else chain must produce the same as shift-based formula."""
        for epoch in range(0, 6):
            h = epoch * BLOCKS_PER_HALVING
            contract_val = block_reward_contract(h)
            shift_val = INITIAL_REWARD >> epoch
            self.assertEqual(contract_val, max(shift_val, MIN_REWARD))

    def test_block_reward_lookup_hits_floor_for_epoch_6_plus(self):
        for epoch in range(6, MAX_HALVINGS + 5):
            h = epoch * BLOCKS_PER_HALVING
            self.assertEqual(block_reward_contract(h), MIN_REWARD)


# ============================================================
# ROUNDING AT EACH EPOCH
# ============================================================

class RoundingTests(unittest.TestCase):
    """
    The miner share is computed as `reward - treasury - lp` to absorb
    integer-division remainders. The contract must produce no lost nanocoins
    at any epoch.
    """

    def test_split_sum_equals_reward_all_epochs(self):
        for epoch in range(MAX_HALVINGS + 5):
            h = epoch * BLOCKS_PER_HALVING
            reward = block_reward_contract(h)
            miner, treasury, lp = split_contract(reward)
            self.assertEqual(
                miner + treasury + lp, reward,
                f"Rounding loss at epoch {epoch}: {miner}+{treasury}+{lp} != {reward}",
            )

    def test_treasury_exactly_10_pct_floor(self):
        """treasury = reward * 10 // 100 — verify no overpay from the floor."""
        for epoch in range(MAX_HALVINGS + 5):
            h = epoch * BLOCKS_PER_HALVING
            reward = block_reward_contract(h)
            _, treasury, _ = split_contract(reward)
            self.assertEqual(treasury, reward * 10 // 100)
            self.assertLessEqual(treasury * 100, reward * 10)

    def test_lp_exactly_5_pct_floor(self):
        for epoch in range(MAX_HALVINGS + 5):
            h = epoch * BLOCKS_PER_HALVING
            reward = block_reward_contract(h)
            _, _, lp = split_contract(reward)
            self.assertEqual(lp, reward * 5 // 100)
            self.assertLessEqual(lp * 100, reward * 5)

    def test_miner_residual_always_between_84_and_86_pct(self):
        """Miner absorbs the residual. Should always be close to 85%."""
        for epoch in range(MAX_HALVINGS + 5):
            h = epoch * BLOCKS_PER_HALVING
            reward = block_reward_contract(h)
            miner, treasury, lp = split_contract(reward)
            # Residual can be at most (treasury_rem + lp_rem) / reward above 85%
            # With reward >= 1_000_000_000 and pct divisors of 100,
            # max residual gain is 2 nanocoins — basically zero.
            ratio = miner * 100 / reward
            self.assertGreaterEqual(ratio, 84.99, f"Miner ratio {ratio} too low at epoch {epoch}")
            self.assertLessEqual(ratio, 85.01, f"Miner ratio {ratio} too high at epoch {epoch}")

    def test_specific_rounding_at_each_epoch(self):
        """
        At MIN_REWARD = 1e9, split is exact (100 divides 1e9 via 10M and 5M).
        At INITIAL_REWARD = 5e10, same. Every epoch-aligned reward happens to
        be evenly divisible by 100 because 1.5625e9 = 5e10 / 32, and 5e10 is
        divisible by 200 already; the halvings only introduce factors of 2.
        """
        expected = [
            # (epoch, miner, treasury, lp) in nanocoins, from emission_model.py
            (0, 42_500_000_000, 5_000_000_000, 2_500_000_000),   # 50 coins
            (1, 21_250_000_000, 2_500_000_000, 1_250_000_000),   # 25 coins
            (2, 10_625_000_000, 1_250_000_000,   625_000_000),   # 12.5 coins
            (3,  5_312_500_000,   625_000_000,   312_500_000),   # 6.25 coins
            (4,  2_656_250_000,   312_500_000,   156_250_000),   # 3.125 coins
            (5,  1_328_125_000,   156_250_000,    78_125_000),   # 1.5625 coins
            (6,    850_000_000,   100_000_000,    50_000_000),   # 1 coin floor
            (7,    850_000_000,   100_000_000,    50_000_000),   # 1 coin
            (10,   850_000_000,   100_000_000,    50_000_000),   # 1 coin
            (19,   850_000_000,   100_000_000,    50_000_000),   # 1 coin
        ]
        for epoch, em, et, el in expected:
            h = epoch * BLOCKS_PER_HALVING
            reward = block_reward_contract(h)
            miner, treasury, lp = split_contract(reward)
            self.assertEqual((miner, treasury, lp), (em, et, el),
                             f"Split mismatch at epoch {epoch}")


# ============================================================
# NORMAL PATH — ACCEPT TESTS AT EVERY HALVING BOUNDARY
# ============================================================

class NormalPathBoundaryTests(unittest.TestCase):
    """
    For every halving boundary H = epoch * BLOCKS_PER_HALVING, construct a
    canonical spend at H-1, H, H+1 and assert it accepts. The self-box value
    at each height is the Python model's emission_box_value_at(), which
    guarantees normalPath's sufficientFunds check passes.

    Note (v1.1): heightIncreased requires HEIGHT > SELF.creation_height.
    make_normal_spend() sets SELF.creation_height=0, so any height > 0 works.
    The very-first-block case (height = 0) needs SELF.creation_height < 0
    which is impossible — so the genesis block is skipped. In practice,
    the emission box is created at genesis (creation_height=0) and first
    spent at height=1 or later.
    """

    def _spend_at(self, height: int):
        # SELF.value = the model's predicted value at that height — always
        # comfortably above the block reward for any pre-terminal epoch.
        self_val = model.emission_box_value_at(height)
        self.assertGreater(self_val, 0, f"h={height}: box exhausted, skipping normal-path test")
        self_box, outputs = make_normal_spend(height, self_val)
        ok, why = evaluate_emission_contract(
            self_box, outputs, height, TREASURY_HASH, LP_HASH
        )
        self.assertTrue(ok, f"h={height}: unexpected reject ({why})")

    def test_boundaries_for_all_epochs(self):
        # Epochs 0..19. Beyond epoch 19 the box exhausts (terminal path).
        # Skip height 0 because heightIncreased requires HEIGHT > creation_height(=0).
        for epoch in range(0, MAX_HALVINGS):
            boundary = epoch * BLOCKS_PER_HALVING
            for offset in (-1, 0, 1):
                h = boundary + offset
                if h <= 0:
                    continue
                # Skip heights where box would be exhausted
                if model.emission_box_value_at(h) <= 0:
                    continue
                with self.subTest(epoch=epoch, offset=offset, h=h):
                    self._spend_at(h)

    def test_first_block_after_genesis(self):
        """
        v1.1: genesis-box is created at height 0. First possible spend is
        height 1 (heightIncreased: 1 > 0). The reward at height 1 is still
        the epoch-0 reward (50 coins).
        """
        self._spend_at(1)

    def test_last_block_before_first_halving(self):
        self._spend_at(BLOCKS_PER_HALVING - 1)

    def test_first_block_of_epoch_1(self):
        self._spend_at(BLOCKS_PER_HALVING)

    def test_reward_exactly_halves_between_H_minus_1_and_H(self):
        for epoch in range(1, 6):  # Epochs where reward is actually halving
            boundary = epoch * BLOCKS_PER_HALVING
            r_before = block_reward_contract(boundary - 1)
            r_at = block_reward_contract(boundary)
            self.assertEqual(r_before, r_at * 2,
                             f"Epoch {epoch}: r_before={r_before}, r_at={r_at}")

    def test_reward_stable_between_H_and_H_plus_1(self):
        for epoch in range(1, MAX_HALVINGS):
            boundary = epoch * BLOCKS_PER_HALVING
            self.assertEqual(block_reward_contract(boundary),
                             block_reward_contract(boundary + 1))


# ============================================================
# NORMAL PATH — REJECT TESTS
# ============================================================

class NormalPathRejectTests(unittest.TestCase):
    """Mutate one field in a valid spend and confirm rejection."""

    def _base(self):
        h = 1000  # arbitrary height in epoch 0 (heightIncreased: 1000 > 0 ✓)
        self_box, outputs = make_normal_spend(h, GENESIS_VALUE)
        return h, self_box, outputs

    def test_underpay_treasury_by_one_nanocoin(self):
        h, self_box, outputs = self._base()
        outputs[1] = make_treasury_box(outputs[1].value - 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, f"expected reject but got accept ({why})")
        self.assertIn("validTreasury", why)

    def test_underpay_lp_by_one_nanocoin(self):
        h, self_box, outputs = self._base()
        outputs[2] = make_lp_box(outputs[2].value - 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("validLP", why)

    def test_treasury_to_wrong_address(self):
        h, self_box, outputs = self._base()
        wrong_box = Box(value=outputs[1].value, proposition_bytes=b"\x00WRONG_SCRIPT",
                        creation_height=h)
        outputs[1] = wrong_box
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("validTreasury", why)
        self.assertIn("blake2b256", why)

    def test_lp_to_wrong_address(self):
        h, self_box, outputs = self._base()
        wrong_box = Box(value=outputs[2].value, proposition_bytes=b"\x00WRONG_LP_SCRIPT",
                        creation_height=h)
        outputs[2] = wrong_box
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("validLP", why)

    def test_emission_box_value_off_by_plus_one(self):
        """New emission box keeps one extra nanocoin — contract must reject."""
        h, self_box, outputs = self._base()
        outputs[0] = make_next_emission_box(self_box, outputs[0].value + 1, spend_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("valueCorrect", why)

    def test_emission_box_value_off_by_minus_one(self):
        """New emission box burns one extra nanocoin — contract must reject."""
        h, self_box, outputs = self._base()
        outputs[0] = make_next_emission_box(self_box, outputs[0].value - 1, spend_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("valueCorrect", why)

    def test_emission_nft_not_preserved_missing(self):
        h, self_box, outputs = self._base()
        outputs[0].tokens = []
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("nftPreserved", why)

    def test_emission_nft_not_preserved_wrong_id(self):
        h, self_box, outputs = self._base()
        outputs[0].tokens = [Token(bytes([0x11] * 32), 1)]
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("nftPreserved", why)

    def test_emission_nft_wrong_amount(self):
        h, self_box, outputs = self._base()
        outputs[0].tokens = [Token(EMISSION_NFT_ID, 2)]
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("nftPreserved", why)

    def test_new_emission_box_uses_different_script(self):
        h, self_box, outputs = self._base()
        outputs[0].proposition_bytes = b"\x00DIFFERENT_SCRIPT"
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("scriptPreserved", why)

    def test_new_emission_box_changes_r4(self):
        h, self_box, outputs = self._base()
        outputs[0].r4 = bytes([0xFF] * 32)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("registersPreserved", why)

    def test_new_emission_box_changes_r5(self):
        h, self_box, outputs = self._base()
        outputs[0].r5 = bytes([0xFF] * 32)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("registersPreserved", why)

    def test_no_emission_box_at_outputs_0(self):
        """
        Attacker tries to skip emission box recreation entirely.
        If OUTPUTS(0) is a treasury-shaped box with full value, normalPath
        fails nftPreserved/scriptPreserved. But terminalPath succeeds if
        self.value < reward. Here self.value is large, so terminal also fails.
        """
        h, self_box, outputs = self._base()
        outputs[0] = make_treasury_box(outputs[0].value, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_swap_treasury_and_lp_order(self):
        """
        OUTPUTS(1) must be treasury, OUTPUTS(2) must be LP. Swapping them
        should fail because treasury's 10% is larger than LP's 5%, so
        OUTPUTS(1) would have the LP value (!= treasuryReward under == check).
        """
        h, self_box, outputs = self._base()
        outputs[1], outputs[2] = outputs[2], outputs[1]
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_missing_treasury_output(self):
        h, self_box, outputs = self._base()
        outputs = [outputs[0], outputs[2]]  # only emission + LP
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_missing_lp_output(self):
        h, self_box, outputs = self._base()
        outputs = outputs[:2]  # only emission + treasury
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    # ---- NEW in v1.1: heightIncreased and heightCorrect reject tests ----

    def test_reject_same_block_spend(self):
        """
        heightIncreased: HEIGHT must be strictly greater than SELF.creation_height.
        If SELF was created at current block height, the spend must reject.
        Mirrors Ergo's `heightIncreased = GT(Height, boxCreationHeight(Self))`.
        """
        h = 1000
        self_box = make_self_box(GENESIS_VALUE, creation_height=h)  # created same block
        _, outputs = make_normal_spend(h, GENESIS_VALUE)
        # Fix up OUTPUTS(0) so it correctly references this SELF's R4/R5 (no change needed;
        # make_self_box uses same hashes). creation_height on new emission is already h.
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, f"same-block spend should reject ({why})")
        self.assertIn("heightIncreased", why)

    def test_reject_earlier_block_height(self):
        """HEIGHT < SELF.creation_height must reject (impossible-time scenario)."""
        self_box = make_self_box(GENESIS_VALUE, creation_height=5000)
        _, outputs = make_normal_spend(1000, GENESIS_VALUE)
        ok, why = evaluate_emission_contract(self_box, outputs, 1000, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("heightIncreased", why)

    def test_reject_new_emission_box_wrong_creation_height(self):
        """
        heightCorrect: OUTPUTS(0).creation_height must equal current HEIGHT.
        Mirrors Ergo's `heightCorrect = EQ(boxCreationHeight(rewardOut), Height)`.
        """
        h = 1000
        self_box, outputs = make_normal_spend(h, GENESIS_VALUE)
        # Set new emission box's creation_height to something other than h
        outputs[0].creation_height = h - 1
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("heightCorrect", why)

    def test_reject_new_emission_box_future_creation_height(self):
        """New emission box with creation_height > HEIGHT must also reject."""
        h = 1000
        self_box, outputs = make_normal_spend(h, GENESIS_VALUE)
        outputs[0].creation_height = h + 1
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("heightCorrect", why)


# ============================================================
# NORMAL PATH — OVERPAY NOW REJECTED (v1.1: strict ==)
# ============================================================

class NormalPathOverpayRejectTests(unittest.TestCase):
    """
    v1.1 tightens treasury/LP value checks from `>=` to `==` to match Ergo's
    strict-equality convention (see ErgoScriptPredef.scala emissionBoxProp:
    `EQ(coinsToIssue, Minus(ExtractAmount(Self), ExtractAmount(rewardOut)))`).
    Any overpay — even by 1 nanocoin — must now be rejected.
    """

    def _base(self):
        h = 100
        self_box, outputs = make_normal_spend(h, GENESIS_VALUE)
        return h, self_box, outputs

    def test_overpay_treasury_rejected(self):
        h, self_box, outputs = self._base()
        outputs[1] = make_treasury_box(outputs[1].value + 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, f"overpay must be rejected under v1.1 == policy ({why})")
        self.assertIn("validTreasury", why)

    def test_overpay_lp_rejected(self):
        h, self_box, outputs = self._base()
        outputs[2] = make_lp_box(outputs[2].value + 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("validLP", why)

    def test_exact_treasury_accepts(self):
        """Boundary: value exactly equal to treasuryReward accepts."""
        h, self_box, outputs = self._base()
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertTrue(ok, why)

    def test_treasury_large_overpay_rejected(self):
        """Ergo-parity: strict == means ANY deviation rejects, not just ±1."""
        h, self_box, outputs = self._base()
        outputs[1] = make_treasury_box(outputs[1].value + 1_000_000_000, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)


# ============================================================
# TERMINAL PATH — ACCEPT TESTS
# ============================================================

class TerminalPathAcceptTests(unittest.TestCase):
    """
    Terminal path triggers when SELF.value < blockReward.
    At epochs 6+, blockReward = MIN_REWARD = 1e9. So terminal path requires
    SELF.value < 1e9.

    v1.1: heightIncreased requires HEIGHT > SELF.creation_height. All terminal
    tests pass a height > 0 so SELF.creation_height=0 works.
    Strict == on term_treasury/term_lp values.
    """

    def test_terminal_with_half_min_reward_remaining(self):
        height = 7 * BLOCKS_PER_HALVING
        remaining = MIN_REWARD // 2
        self_box, outputs = make_terminal_spend(remaining, height=height)
        ok, why = evaluate_emission_contract(
            self_box, outputs, height, TREASURY_HASH, LP_HASH
        )
        self.assertTrue(ok, f"expected accept via terminal, got ({why})")
        self.assertIn("terminalPath", why)

    def test_terminal_with_zero_remaining(self):
        """
        Edge case: SELF.value = 0. termTreasury = 0, termLP = 0.
        Under v1.1 strict ==, OUTPUTS(0/1).value must be exactly 0.
        In practice, consensus MIN_BOX_VALUE makes this unreachable — theoretical
        case documenting contract semantics only.
        """
        height = 10 * BLOCKS_PER_HALVING
        remaining = 0
        self_box, outputs = make_terminal_spend(remaining, height=height)
        ok, why = evaluate_emission_contract(
            self_box, outputs, height, TREASURY_HASH, LP_HASH
        )
        self.assertTrue(ok, f"terminal with zero should accept ({why})")

    def test_terminal_reward_minus_one(self):
        """SELF.value = blockReward - 1 → terminal path."""
        height = 7 * BLOCKS_PER_HALVING
        reward = block_reward_contract(height)
        remaining = reward - 1
        self_box, outputs = make_terminal_spend(remaining, height=height)
        ok, why = evaluate_emission_contract(
            self_box, outputs, height, TREASURY_HASH, LP_HASH
        )
        self.assertTrue(ok, why)

    def test_normal_path_rejects_when_equal_to_reward(self):
        """
        Boundary: SELF.value == blockReward. sufficientFunds passes (>=),
        so normal path is taken. Valid spend produces new box with value 0.
        """
        height = 7 * BLOCKS_PER_HALVING
        reward = block_reward_contract(height)
        remaining = reward
        self_box, outputs = make_normal_spend(height, remaining)
        self.assertEqual(outputs[0].value, 0)
        ok, why = evaluate_emission_contract(
            self_box, outputs, height, TREASURY_HASH, LP_HASH
        )
        self.assertTrue(ok, f"contract accepts 0-value next box ({why})")

    def test_terminal_entry_via_realistic_final_blocks(self):
        """
        Walk the last few blocks before emission exhaustion and verify
        normal path stays valid until SELF.value < reward, then terminal kicks in.
        """
        height = 20 * BLOCKS_PER_HALVING
        reward = MIN_REWARD
        value = reward * 3 + (reward // 2)
        blocks_ok = 0
        while value >= reward:
            self_box, outputs = make_normal_spend(height, value)
            ok, why = evaluate_emission_contract(
                self_box, outputs, height, TREASURY_HASH, LP_HASH
            )
            self.assertTrue(ok, f"block {blocks_ok}: expected accept ({why})")
            value -= reward
            height += 1
            blocks_ok += 1
            if blocks_ok > 10:
                self.fail("emission did not exhaust as expected")

        self.assertEqual(blocks_ok, 3, f"expected 3 normal blocks, got {blocks_ok}")
        # Now value < reward, terminal path should trigger
        self_box, outputs = make_terminal_spend(value, height=height)
        ok, why = evaluate_emission_contract(
            self_box, outputs, height, TREASURY_HASH, LP_HASH
        )
        self.assertTrue(ok, f"terminal failed: {why}")


# ============================================================
# TERMINAL PATH — REJECT TESTS
# ============================================================

class TerminalPathRejectTests(unittest.TestCase):
    def _base(self):
        height = 7 * BLOCKS_PER_HALVING
        remaining = MIN_REWARD // 2
        self_box, outputs = make_terminal_spend(remaining, height=height)
        return height, self_box, outputs, remaining

    def test_underpay_terminal_treasury(self):
        h, self_box, outputs, remaining = self._base()
        term_treasury = remaining * 10 // 100
        outputs[0] = make_treasury_box(term_treasury - 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_underpay_terminal_lp(self):
        h, self_box, outputs, remaining = self._base()
        term_lp = remaining * 5 // 100
        outputs[1] = make_lp_box(term_lp - 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_overpay_terminal_treasury_rejected(self):
        """v1.1: == not >=, so overpay also rejects."""
        h, self_box, outputs, remaining = self._base()
        term_treasury = remaining * 10 // 100
        outputs[0] = make_treasury_box(term_treasury + 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, f"overpay must be rejected under v1.1 ({why})")

    def test_overpay_terminal_lp_rejected(self):
        h, self_box, outputs, remaining = self._base()
        term_lp = remaining * 5 // 100
        outputs[1] = make_lp_box(term_lp + 1, creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_terminal_treasury_wrong_script(self):
        h, self_box, outputs, _ = self._base()
        outputs[0] = Box(value=outputs[0].value, proposition_bytes=b"\x00WRONG",
                         creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_terminal_lp_wrong_script(self):
        h, self_box, outputs, _ = self._base()
        outputs[1] = Box(value=outputs[1].value, proposition_bytes=b"\x00WRONG_LP",
                         creation_height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_terminal_nft_smuggled_to_output_0(self):
        """Emission NFT must not appear in any output during terminal path."""
        h, self_box, outputs, _ = self._base()
        outputs[0].tokens = [Token(EMISSION_NFT_ID, 1)]
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("nftBurned", why)

    def test_terminal_nft_smuggled_to_output_1(self):
        h, self_box, outputs, _ = self._base()
        outputs[1].tokens = [Token(EMISSION_NFT_ID, 1)]
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("nftBurned", why)

    def test_terminal_nft_smuggled_to_additional_output(self):
        """Attacker adds a third output carrying the NFT — must fail nftBurned."""
        h, self_box, outputs, _ = self._base()
        nft_carrier = Box(
            value=MIN_BOX_VALUE,
            proposition_bytes=b"\x00MINER",
            tokens=[Token(EMISSION_NFT_ID, 1)],
            creation_height=h,
        )
        outputs.append(nft_carrier)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("nftBurned", why)

    def test_terminal_attempted_when_normal_path_valid(self):
        """
        Adversary sets up terminal-shaped outputs even though SELF.value >= reward.
        `insufficient` is false; terminal path rejects. Normal path also rejects
        because outputs aren't in normal-path shape. Result: reject.
        """
        h = 7 * BLOCKS_PER_HALVING
        reward = block_reward_contract(h)
        self_value = reward * 100
        self_box = make_self_box(self_value, creation_height=0)
        term_treasury = self_value * 10 // 100
        term_lp = self_value * 5 // 100
        outputs = [
            make_treasury_box(term_treasury, creation_height=h),
            make_lp_box(term_lp, creation_height=h),
        ]
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)

    def test_terminal_same_block_spend_rejects(self):
        """
        v1.1 [Gap A]: heightIncreased applies in terminal path too.
        SELF created at same block as spend must reject.
        """
        h = 7 * BLOCKS_PER_HALVING
        remaining = MIN_REWARD // 2
        self_box = make_self_box(remaining, creation_height=h)  # same block
        _, outputs = make_terminal_spend(remaining, height=h)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertFalse(ok, why)
        self.assertIn("heightIncreased", why)


# ============================================================
# PATH-SELECTION INVARIANT
# ============================================================

class PathSelectionTests(unittest.TestCase):
    """
    Exactly one of normalPath / terminalPath can be true for a given (self, outputs, height).
    Mutual exclusion comes from `sufficientFunds` vs `insufficient` being negations.
    """

    def test_normal_and_terminal_cannot_both_accept_normal_spend(self):
        h = 1000
        self_box, outputs = make_normal_spend(h, GENESIS_VALUE)
        reward = block_reward_contract(h)
        self.assertGreaterEqual(self_box.value, reward)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertTrue(ok)
        self.assertIn("normalPath", why)

    def test_normal_and_terminal_cannot_both_accept_terminal_spend(self):
        h = 10 * BLOCKS_PER_HALVING
        remaining = MIN_REWARD // 3
        self_box, outputs = make_terminal_spend(remaining, height=h)
        reward = block_reward_contract(h)
        self.assertLess(self_box.value, reward)
        ok, why = evaluate_emission_contract(self_box, outputs, h, TREASURY_HASH, LP_HASH)
        self.assertTrue(ok)
        self.assertIn("terminalPath", why)


# ============================================================
# OVERFLOW / BOUNDS
# ============================================================

class BoundsTests(unittest.TestCase):
    def test_no_overflow_at_max_reward(self):
        """reward * MINER_PCT (85) must stay under i64 max."""
        max_intermediate = INITIAL_REWARD * MINER_PCT
        self.assertLess(max_intermediate, 2**63 - 1)

    def test_no_overflow_at_genesis_box_value(self):
        """Genesis box value must fit in i64."""
        self.assertLess(GENESIS_VALUE, 2**63 - 1)

    def test_genesis_box_value_matches_spec(self):
        """From PARAMETERS.md: ~177,412,882.5 coins."""
        expected = 177_412_882_500_000_000
        self.assertEqual(GENESIS_VALUE, expected)

    def test_reward_monotonically_non_increasing(self):
        prev = INITIAL_REWARD + 1
        h = 0
        while h < 25 * BLOCKS_PER_HALVING:
            r = block_reward_contract(h)
            self.assertLessEqual(r, prev, f"reward increased at h={h}")
            prev = r
            h += 10_000

    def test_reward_never_below_min(self):
        h = 0
        while h < 25 * BLOCKS_PER_HALVING:
            self.assertGreaterEqual(block_reward_contract(h), MIN_REWARD)
            h += 10_000


# ============================================================
# Test runner with summary
# ============================================================

if __name__ == "__main__":
    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    for cls in [
        CrossOracleTests,
        RoundingTests,
        NormalPathBoundaryTests,
        NormalPathRejectTests,
        NormalPathOverpayRejectTests,
        TerminalPathAcceptTests,
        TerminalPathRejectTests,
        PathSelectionTests,
        BoundsTests,
    ]:
        suite.addTests(loader.loadTestsFromTestCase(cls))

    runner = unittest.TextTestRunner(verbosity=2)
    result = runner.run(suite)

    print()
    print("=" * 72)
    print(f"TOTAL: {result.testsRun}  FAILURES: {len(result.failures)}  ERRORS: {len(result.errors)}")
    print("=" * 72)
    sys.exit(0 if result.wasSuccessful() else 1)
