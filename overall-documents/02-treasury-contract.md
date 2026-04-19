# Handoff: Treasury Governance Contract

**Checklist Reference**: Phase 4.3
**Owner**: ErgoScript developer (you)
**Blockers**: None
**Dependencies**: Emission contract (treasury output address must match)
**Deliverable**: Multisig treasury contract with proposal/vote mechanism, tested on Ergo mainnet

---

## Objective

Write the contract that governs the 10% dev treasury allocation. This contract receives coins every block from the emission contract and controls how they can be spent. It must prevent unilateral spending while enabling legitimate development funding.

---

## Design Requirements

1. **Initial governance**: 2-of-3 or 3-of-5 multisig among known, trusted community members
2. **Proposal mechanism**: spending requires a formal proposal specifying recipient, amount, and purpose
3. **Time delay**: approved proposals cannot execute immediately — minimum 48-72 hour delay between approval and execution (gives community time to react)
4. **Spending caps**: optional per-proposal or per-epoch spending limit
5. **Migration path**: contract should support upgrading to DAO governance (Paideia-style) via a supermajority vote of signers
6. **Accumulation**: treasury box accumulates coins from multiple blocks. Periodically consolidate into a single UTXO for cleaner management

---

## Contract Architecture

Two contracts working together:

### Contract A: Treasury Accumulation Box
Receives the 10% from each coinbase TX. Simple — just accumulates value. Periodically consolidated by any signer into the main treasury box.

```
Guard: any signer can consolidate multiple accumulation boxes into one,
       OR spend to the governance contract address
```

### Contract B: Treasury Governance Box
The main treasury holding. Spending requires:

```
1. Proposal registered (recipient address, amount, justification hash in R4-R6)
2. k-of-n signers approve (threshold sigma proof)
3. Timelock elapsed since approval (HEIGHT > approvalHeight + delayBlocks)
4. Output matches proposal exactly (amount, recipient)
5. Change returned to same governance contract
```

---

## ErgoScript Sketch

```ergoscript
{
  // Treasury Governance Contract
  // Registers:
  //   R4: Coll[Byte] — proposal hash (blake2b256 of: recipient ++ amount ++ justification)
  //   R5: Long — approval height (0 if no active proposal)
  //   R6: Coll[Byte] — recipient proposition bytes
  //   R7: Long — approved amount
  
  val signers = Coll(
    PK("signer1_pubkey"),  // PLACEHOLDER
    PK("signer2_pubkey"),  // PLACEHOLDER  
    PK("signer3_pubkey"),  // PLACEHOLDER
    PK("signer4_pubkey"),  // PLACEHOLDER
    PK("signer5_pubkey")   // PLACEHOLDER
  )
  
  val threshold = 3  // 3-of-5
  val timelockBlocks = 12960L  // ~72 hours at 20s blocks (72 * 3600 / 20)
  
  val isApproval = {
    // Setting a new proposal: output box has proposal data in registers
    // Requires threshold signature
    val out = OUTPUTS(0)
    out.propositionBytes == SELF.propositionBytes &&
    out.value == SELF.value &&  // no spending during approval, just register update
    out.R5[Long].get == HEIGHT  // record approval height
  }
  
  val isExecution = {
    // Executing an approved proposal
    val approvalHeight = SELF.R5[Long].get
    val recipient = SELF.R6[Coll[Byte]].get
    val amount = SELF.R7[Long].get
    
    // Timelock check
    val timelockPassed = HEIGHT > approvalHeight + timelockBlocks
    
    // Proposal was actually set (approval height > 0)
    val hasActiveProposal = approvalHeight > 0L
    
    // Payment output matches proposal
    val correctPayment = {
      OUTPUTS(1).propositionBytes == recipient &&
      OUTPUTS(1).value >= amount
    }
    
    // Change goes back to treasury with cleared proposal
    val correctChange = {
      OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
      OUTPUTS(0).value >= SELF.value - amount - 1000000L &&  // allow for TX fee
      OUTPUTS(0).R5[Long].get == 0L  // clear proposal after execution
    }
    
    timelockPassed && hasActiveProposal && correctPayment && correctChange
  }
  
  val isConsolidation = {
    // Merging multiple treasury boxes into one — no spending, just combining
    OUTPUTS(0).propositionBytes == SELF.propositionBytes &&
    OUTPUTS(0).value >= SELF.value  // at least as much as this input (other inputs add more)
  }
  
  atLeast(threshold, signers) && sigmaProp(isApproval || isExecution || isConsolidation)
}
```

**NOTE**: Sketch only. Production version needs:
- Proper proposal cancellation mechanism
- Protection against proposal replacement attacks (someone submits new proposal to reset timelock)
- Maximum spending per epoch
- Emergency pause (e.g., if a signer key is compromised, remaining signers can freeze treasury)

---

## Testing Plan

1. **Happy path**: create proposal → approve with 3 sigs → wait timelock → execute → verify payment
2. **Timelock enforcement**: attempt execution before delay — must reject
3. **Threshold enforcement**: attempt with 2-of-5 sigs — must reject
4. **Wrong recipient**: execute with different address than proposal — must reject
5. **Wrong amount**: execute with different amount — must reject
6. **Double execution**: attempt to execute same proposal twice — must reject (proposal cleared)
7. **Consolidation**: merge 10 small accumulation boxes into one treasury box
8. **Proposal replacement**: what happens if a new proposal overwrites a pending one? Define behavior.

Deploy test version on Ergo mainnet with small amounts to prove the flow works end-to-end with real transactions.

---

## Migration to DAO

The contract should include a migration path:

```
If ALL signers approve (n-of-n, not k-of-n) AND timelock passes:
  Treasury can be moved to a new contract address (the DAO contract)
```

This is a one-time irreversible migration. Once treasury moves to DAO governance, the multisig contract is empty and abandoned.

---

## What to Deliver

1. **treasury_accumulation.es** — Simple accumulation contract
2. **treasury_governance.es** — Main governance contract
3. **treasury_test suite** — All test cases above
4. **SIGNERS.md** — Process for selecting initial signers, key management requirements
5. **MIGRATION.md** — DAO migration plan and timeline
