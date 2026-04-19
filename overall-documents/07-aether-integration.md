# Handoff: AEther Integration Specification

**Checklist Reference**: Phase 9.1, 9.2
**Owner**: You (coordination), AEther/Degens World team (implementation)
**Blockers**: YES — requires AEther team availability and willingness to scope
**Dependencies**: Chain commitment design (Ergo ↔ new chain), AEther's current architecture
**Deliverable**: Written integration spec agreed by both sides, defining exactly what each party builds

---

## Objective

Define the technical interface between the new chain and AEther so that bridging works at or near launch. AEther has been tested with Ergo mainnet ↔ Cosmos/Ethereum/Ronin testnets. The new chain needs to be added as a new custody/settlement domain.

This is a coordination document, not an implementation document. The goal is to get both teams aligned on what's built, by whom, and in what order.

---

## What You Need From the AEther Team

### Questions to Ask

1. **What does AEther require from a new chain to recognize it as a custody domain?**
   - Transaction format requirements?
   - Event types AEther watchers need to observe?
   - Confirmation depth requirements?
   - API endpoints AEther needs from the new chain's node?

2. **How does AEther handle a chain that uses Ergo's box model (eUTXO) vs. account model?**
   - AEther already handles Ergo's eUTXO. Is the new chain treated as "another Ergo-like chain" or does it need separate integration work?

3. **What is the lock/escrow contract pattern AEther expects?**
   - Does the new chain need to deploy AEther-specific lock contracts?
   - Or does AEther observe standard box creation patterns?
   - Is there a standard AEther lock contract template to port?

4. **What's the path from new chain → Ergo → Cosmos → EVM?**
   - Is this: new chain native bridge to Ergo, then AEther handles Ergo → Cosmos → EVM?
   - Or does AEther directly integrate the new chain as a first-class domain?

5. **What's the AEther team's timeline and capacity?**
   - Can they scope new chain integration within the next 2-3 months?
   - What resources do they need from us?

6. **What tokens are supported?**
   - Native coin only initially?
   - EIP-4 tokens (same standard as Ergo since same box model)?
   - NFTs?

---

## What You Can Prepare For the AEther Team

### Chain Technical Summary

Prepare a one-page document:

```
Chain Name: [TBD]
Consensus: Etchash Proof of Work
Block Time: 15-20 seconds
Transaction Model: eUTXO (identical to Ergo — sigma-rust based)
Box Model: Same as Ergo (registers R0-R9, same serialization)
Token Standard: EIP-4 compatible (same as Ergo)
Address Format: [New prefix — TBD]
Node API: Same as Ergo node API (REST, same endpoints)
Signature Scheme: Same as Ergo (Schnorr, sigma protocols, threshold)
Threshold Signature Support: Yes (native sigma protocols — atLeast(k, n))
```

The key selling point: since the box model, transaction model, and API are identical to Ergo, AEther's existing Ergo integration should require minimal changes to support the new chain.

### Chain Commitment Design

Define how the new chain and Ergo communicate state:

**Option A — Chain commitment via relay**
- Deploy relay contract on Ergo that validates new chain's Etchash headers
- AEther uses this relay to verify new chain events
- More trustless, more engineering

**Option B — Direct AEther integration**
- AEther watchers observe the new chain directly (same as they observe Ergo)
- No relay needed — AEther's own verification layer provides the trust
- Simpler, depends on AEther's architecture supporting this

**Option C — Through Ergo**
- New chain assets bridge to Ergo first (via chain commitments / ErgoHack VII pattern)
- Then AEther bridges Ergo → everywhere
- Most conservative, but adds a hop

**Discuss with AEther team which option they prefer and can support.**

---

## Timeline Alignment

| Milestone | Your Chain | AEther |
|---|---|---|
| Now | Begin conversation, share chain spec | Assess integration scope |
| Month 1-2 | Contracts written, node being forked | Integration spec finalized |
| Month 3-4 | Testnet live | AEther testnet integration |
| Month 4-5 | Bridge testing | End-to-end transfer testing |
| Month 5-6 | Mainnet launch | AEther production integration |

The key constraint: AEther integration doesn't need to be day-one, but it should be within the first few weeks. The "liquid from launch" story depends on this.

---

## What to Deliver

1. **chain_technical_summary.md** — One-page chain spec for AEther team
2. **integration_questions.md** — The questions listed above, sent to AEther team
3. **integration_spec.md** — Joint document (written with AEther team) defining the interface
4. **timeline_agreement.md** — Agreed milestones with dates
5. **test_plan.md** — End-to-end bridge testing plan (what assets, what directions, what failure cases)
