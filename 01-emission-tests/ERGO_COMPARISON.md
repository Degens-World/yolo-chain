# ERGO_COMPARISON.md — Cross-reference findings vs. Ergo's `emissionBoxProp`

Source: `ErgoScriptPredef.scala` at commit `bd1906e` (ScorexFoundation/sigmastate-interpreter).
Retrieved 2026-04-17 via github.com web fetch.

## Ergo's emission contract (paraphrased from Scala AST builder)

```
AND(
  heightIncreased,              // Height > boxCreationHeight(Self)
  correctMinerOutput,           // minerOut script == expectedMinerScript(delay, pk)
                                // AND Height == boxCreationHeight(minerOut)
  OR(
    AND(
      outputsNum,               // SizeOf(Outputs) == 2
      sameScriptRule,           // SELF.script == rewardOut.script
      correctCoinsConsumed,     // EQ(coinsToIssue, SELF.value - rewardOut.value)  ← STRICT EQ
      heightCorrect             // boxCreationHeight(rewardOut) == Height
    ),
    lastCoins                   // SELF.value <= oneEpochReduction  ← terminal bypass
  )
)
```

Layout: `OUTPUTS(0)` = new emission box (rewardOut), `OUTPUTS(1)` = miner box. No treasury/LP in the per-block TX — Ergo Foundation has its own separate contract (`foundationScript`) at genesis.

---

## Finding-by-finding

### Finding 1: `>=` vs `==` on recipient outputs

| | Ergo | Ours (current) | Verdict |
|---|---|---|---|
| Amount consumed from emission box | `EQ(coinsToIssue, SELF.value - rewardOut.value)` | `nextBox.value == SELF.value - blockReward` | ✓ MATCHES (both strict `==`) |
| Recipient output value | N/A — Ergo checks miner output *script* but NOT value; miner can split the withdrawn amount any way inside the rewardOut box | `OUTPUTS(1).value >= treasuryReward`, `OUTPUTS(2).value >= lpReward` | ✗ **DEVIATES** |

Ergo's philosophy is strict `EQ` on the amount *taken out of the emission box*. Miner-output-value is not checked in the contract because Ergo doesn't split payment on-chain — miner takes all of `coinsToIssue`, foundation is a separate genesis box.

Our contract splits on-chain, which makes the recipient-value check load-bearing. Using `>=` is looser than Ergo's convention. **Tightening to `==` matches Ergo's strict-equality philosophy on emission accounting.**

**Action: ADJUST — change `>=` to `==` on treasury and LP values in both paths.**

---

### Finding 2: MIN_BOX_VALUE boundary check

| | Ergo | Ours (current) | Verdict |
|---|---|---|---|
| Minimum-value check on new emission box | ✗ Not present | ✗ Not present | ✓ MATCHES |
| Handling of exhaustion | `lastCoins = LE(SELF.value, oneEpochReduction)` — terminal bypass where miner takes everything, no new emission box created | `insufficient = SELF.value < blockReward` — terminal path splits remaining between treasury/LP, NFT burned | ✓ STRUCTURALLY EQUIVALENT |

Ergo does NOT add a `MIN_BOX_VALUE` precondition on the rewardOut box. They handle exhaustion with the `lastCoins` bypass: once the emission box balance drops to ≤ one epoch of rewards, the entire normal-path block is replaced with "miner takes everything, no continuation."

Our design has an equivalent bypass via the terminal path.

**Action: INVESTIGATE → NO CHANGE. Ergo doesn't do this check; our terminal path plays the same role as their `lastCoins` branch.**

---

### Finding 3: Dead-code floor clamp (`if (computed > minReward) computed else minReward`)

| | Ergo | Ours (current) | Verdict |
|---|---|---|---|
| Reward formula | `fixedRate - oneEpochReduction × epoch` (linear subtraction that can go to 0) | `INITIAL / 2^halvings` via lookup, floor-clamped to MIN_REWARD | ≠ DIFFERENT DESIGN |
| Floor clamp | Not needed (subtraction bottoms at 0 naturally, then `lastCoins` catches) | Redundant for enumerated branches but harmless | STYLE CHOICE |

Ergo's reward formula is a monotonic linear decrease; it bottoms at 0 and the `lastCoins` bypass catches exhaustion. Our lookup-table formula has a floor clamp that's technically dead code for branches 0–5 but catches the `else` fallthrough.

Removing our clamp would make halvings 6+ return 0 if we also changed the `else` branch — but the `else` already returns `minReward` explicitly, so the clamp is a no-op. Keeping it is defensive against future edits that might change the `else` branch.

**Action: INVESTIGATE → NO CHANGE. Different reward formula means Ergo's practice doesn't translate. The clamp is cheap defense-in-depth.**

---

### Finding 4: Zero-value terminal path accepts

| | Ergo | Ours (current) | Verdict |
|---|---|---|---|
| Behavior when `SELF.value` is very small | `lastCoins` bypasses amount checks entirely — miner can take any/all of remaining value | Terminal path accepts with `termTreasury = value×10%`, `termLP = value×5%`, miner gets rest at consensus | ✓ EQUIVALENT |

Both designs handle the "not enough for a full reward" case by relaxing the amount checks. Neither adds a floor to prevent a zero-value spend because consensus-level MIN_BOX_VALUE makes the scenario unreachable in practice.

**Action: NO CHANGE. Behavior matches Ergo's pattern.**

---

## NEW gaps found in cross-reference

These are checks Ergo has that our contract doesn't. Worth considering for parity with the canonical reference.

### Gap A: `heightIncreased = HEIGHT > SELF.creationInfo._1`

Ergo requires this on every spend. Prevents spending the emission box in the same block it was created.

**Relevance to our design:** likely redundant because we identify the emission box via a singleton NFT (which Ergo didn't have originally — added in EIP-27). A singleton prevents forgery of lookalike emission boxes, so `heightIncreased` is belt-and-suspenders here.

**Cost:** ~1 boolean op, negligible.

**Recommendation:** ADD for defense-in-depth. Matches Ergo.

---

### Gap B: `heightCorrect = OUTPUTS(0).creationInfo._1 == HEIGHT`

Ergo locks the new emission box's `creationHeight` to the current block height.

**Relevance to our design:** the emission box is spent every block, so storage rent never accrues on it — the `creationHeight` has no practical effect. Minimal benefit.

**Recommendation:** ADD for parity and future-proofing (if emission ever pauses for any reason, storage-rent manipulation would become relevant). Low cost.

---

### Gap C: `correctMinerOutput` — in-contract miner output check

Ergo enforces that `OUTPUTS(1).script == expectedMinerScript(delay, minerPk)` with a hardcoded 720-block time-lock on the miner's reward.

**Relevance to our design:** our handoff explicitly delegates miner output enforcement to the consensus layer, stating:
> `(miner reward enforced at consensus layer, not in this contract)`

This is a deliberate design choice, not an oversight. Ergo's approach is belt-and-suspenders (consensus + contract); ours is single-layer (consensus only).

**Security implication:** A modified node that ignores consensus miner-reward rules could in principle accept blocks that underpay/overpay the miner. Ergo's contract would reject these independently; ours wouldn't. In practice, the network-wide consensus catches this, so the attack is non-exploitable.

**Recommendation:** DOCUMENT as a known design difference. Adding a miner-output check in the contract would require hardcoding a miner-address format and delay period at genesis — more complexity for marginal benefit if consensus is trusted. Status quo acceptable; flag for the team.

---

## Summary of actions

| # | Finding / Gap | vs Ergo | Action |
|---|---|---|---|
| 1 | `>=` on treasury & LP recipient values | **DEVIATES** | **ADJUST → `==`** |
| 2 | No MIN_BOX_VALUE check on new emission box | MATCHES (both omit) | No change |
| 3 | Dead-code floor clamp in lookup | Different reward formula | No change (keep as defense) |
| 4 | Zero-value terminal accepts | Equivalent behavior | No change |
| A | No `heightIncreased` check | DEVIATES | **ADD for parity** (low cost) |
| B | No `heightCorrect` check | DEVIATES | **ADD for parity** (low cost) |
| C | No in-contract miner output check | DEVIATES (deliberate) | Document design difference |
