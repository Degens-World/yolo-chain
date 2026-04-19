# Handoff: LP Fund Contract

**Checklist Reference**: Phase 4.4
**Owner**: ErgoScript developer (you)
**Blockers**: None
**Dependencies**: Emission contract (LP output address must match)
**Deliverable**: LP fund contract with narrow spending mandate, tested on Ergo mainnet

---

## Objective

Write the contract that governs the 5% LP/market-making allocation. Unlike the treasury (general development funding), this contract has a narrow mandate: funds can only flow to DEX liquidity pools, market-making contracts, and bridge liquidity. This constraint should be enforced at the contract level, not just by governance norms.

---

## Design Requirements

1. **Narrow mandate**: funds can only be sent to whitelisted destination contract hashes (DEX LP contracts, bridge liquidity contracts, market-making contracts)
2. **Governance**: same multisig structure as treasury (can share signers or have separate set)
3. **Vesting**: consider time-locked release — e.g., 1/24th of accumulated funds available per month for first 2 years, preventing a day-one dump of the entire LP fund into one pool
4. **Whitelist management**: signers can add new destination contract hashes (for when new DEX or bridge contracts are deployed) via proposal + timelock
5. **Accumulation**: same pattern as treasury — accumulation boxes from coinbase, periodic consolidation

---

## Contract Architecture

### Spending Constraints

The LP fund contract enforces that every spend goes to a whitelisted contract:

```
For each output that isn't the change box:
  blake2b256(output.propositionBytes) must be in the whitelist
```

The whitelist is stored in a register as a collection of script hashes. It starts with known contract hashes (Spectrum AMM pool contract, AEther bridge lock contract) and can be updated via governance.

---

## ErgoScript Sketch

```ergoscript
{
  // LP Fund Contract
  // R4: Coll[Coll[Byte]] — whitelist of allowed destination script hashes
  // R5: Long — total coins received to date (for vesting calculation)
  // R6: Long — total coins spent to date (for vesting cap)
  
  val signers = Coll(
    PK("signer1"),  // PLACEHOLDER — may overlap with treasury signers
    PK("signer2"),
    PK("signer3")
  )
  val threshold = 2  // 2-of-3
  
  val whitelist = SELF.R4[Coll[Coll[Byte]]].get
  
  // All non-change outputs must go to whitelisted destinations
  val allOutputsWhitelisted = {
    val changeBox = OUTPUTS(0)  // first output is always change back to LP fund
    val spendOutputs = OUTPUTS.slice(1, OUTPUTS.size)
    spendOutputs.forall { (out: Box) =>
      val outHash = blake2b256(out.propositionBytes)
      whitelist.exists { (allowed: Coll[Byte]) => allowed == outHash }
    }
  }
  
  // Change box preserves contract and whitelist
  val correctChange = {
    OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
    OUTPUTS(0).R4[Coll[Coll[Byte]]].get == whitelist
  }
  
  // Vesting cap (optional — remove if not desired)
  // val maxSpendable = SELF.value * HEIGHT / vestingEndHeight
  // val totalSpent = SELF.R6[Long].get
  // val thisSpend = SELF.value - OUTPUTS(0).value
  // val underCap = totalSpent + thisSpend <= maxSpendable
  
  val isSpend = allOutputsWhitelisted && correctChange
  
  // Whitelist update: can add new script hash, same governance rules
  val isWhitelistUpdate = {
    OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
    OUTPUTS(0).value == SELF.value &&  // no spending during whitelist update
    OUTPUTS(0).R4[Coll[Coll[Byte]]].get.size >= whitelist.size  // can only add, not remove
  }
  
  val isConsolidation = {
    OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
    OUTPUTS(0).value >= SELF.value
  }
  
  atLeast(threshold, signers) && sigmaProp(isSpend || isWhitelistUpdate || isConsolidation)
}
```

---

## Vesting Design (Decision Needed)

**Option A — No vesting**: LP fund is fully available from day one. Simpler. Risk: governance could dump entire fund into poorly designed pools early.

**Option B — Linear vesting**: Only a fraction available based on time elapsed. E.g., after month 1, only 1/24th of total accumulated is spendable. After month 12, half. After month 24, all. Prevents premature depletion.

**Option C — Milestone vesting**: Funds unlock when specific conditions are met (e.g., first DEX pool has > X liquidity, bridge is operational). More complex to implement on-chain.

**Recommendation**: Option A for simplicity. The multisig governance and narrow whitelist already constrain spending. Adding vesting creates complexity for marginal benefit if the signers are trusted.

---

## Initial Whitelist

At launch, the whitelist should contain script hashes for:

1. Spectrum-style AMM pool contract (deployed on new chain)
2. AEther bridge lock contract (for seeding cross-chain liquidity)
3. A simple "hold" contract (for cases where LP funds need to be staged before deployment)

Additional script hashes added via governance proposal as new DeFi contracts deploy.

---

## Testing Plan

1. **Whitelisted spend**: send funds to a whitelisted contract — should succeed
2. **Non-whitelisted spend**: send funds to a random address — must reject
3. **Whitelist addition**: add a new script hash via governance — verify it works for subsequent spends
4. **Consolidation**: merge accumulation boxes
5. **Change box preservation**: verify whitelist and contract are preserved in change output
6. **Vesting enforcement** (if implemented): attempt to spend more than allowed — must reject

---

## What to Deliver

1. **lp_fund.es** — The LP fund contract
2. **lp_accumulation.es** — Accumulation contract (may be same as treasury accumulation)
3. **lp_fund_test suite** — All test cases
4. **WHITELIST.md** — Initial whitelist entries with script hashes and rationale
5. **VESTING_DECISION.md** — Document the vesting decision and reasoning
