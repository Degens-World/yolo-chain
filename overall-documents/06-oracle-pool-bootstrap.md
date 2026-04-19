# Handoff: Oracle Pool Bootstrap

**Checklist Reference**: Phase 10.1
**Owner**: You (contract + operations), plus 2-4 recruited operators
**Blockers**: Soft — need operator commitments (people problem)
**Dependencies**: Chain must exist for deployment, but contract + operator recruitment starts now
**Deliverable**: Oracle pool contract ready to deploy, operator set committed, operational runbook

---

## Objective

Prepare the oracle pool infrastructure so price feeds are available immediately at launch. DeFi without oracles is dead — no stablecoin, no options pricing, no liquidations. You already operate an ERG/USD Oracle Pool 2.0 node on Ergo. This is the same pattern on the new chain.

---

## What's Needed Before Chain Exists

### 1. Oracle Pool Contract (ErgoScript)

Port the Oracle Pool 2.0 contract design to the new chain. Key parameters to adjust:

| Parameter | Ergo Oracle Pool 2.0 | New Chain | Notes |
|---|---|---|---|
| Epoch duration | ~30 blocks (60 min) | ~180 blocks (60 min at 20s) | Same real-time duration, more blocks |
| Posting window | ~5 blocks | ~30 blocks | Proportional increase |
| Minimum operators | 4 | 3 (initially) | Lower bar for bootstrap, increase later |
| Data point | ERG/USD | YourToken/USD | Primary feed |
| Reward mechanism | Refresh TX competition | Same or simplified | Your sniper experience informs this |

The oracle pool contract from Ergo is open source ErgoScript. Fork it, adjust constants, test.

### 2. Data Source Strategy

Each oracle operator needs a reliable price feed for YourToken/USD. At launch there's no CEX price. Options:

**Option A — Bootstrap from DEX price**
- Deploy Spectrum AMM pool on the new chain with seed liquidity (from LP fund)
- Oracle operators read the pool price
- Risk: thin liquidity = manipulable price
- Mitigation: use TWAP (time-weighted average price) over N blocks, not spot price

**Option B — Cross-chain reference**
- If the token is bridged to Ethereum/Cosmos via AEther, there may be a DEX price on EVM
- Operators can aggregate: on-chain DEX price + bridged DEX price
- More manipulation-resistant but adds bridge dependency

**Option C — Initial fixed price**
- For the first days/weeks, set a governance-determined initial price
- Transition to market-based feeds once DEX liquidity is sufficient
- Simple but centralized

**Recommendation**: Option A with TWAP. Accept that early oracle prices will be based on thin liquidity, and design the stablecoin/DeFi contracts to handle higher volatility (wider collateral ratios, larger liquidation buffers).

### 3. Operator Recruitment

Target: 3-5 operators committed before launch. You are operator #1.

Where to recruit:
- Existing Ergo oracle operators (they already understand the system)
- Community members running Ergo nodes (infrastructure-minded people)
- AEther team members (aligned incentives)

What operators need:
- A machine running 24/7 (VPS or home server)
- The oracle operator software (Node.js or Rust, same pattern as your ERG/USD setup)
- Willingness to stake collateral (oracle token or native token)
- Reliable data source access

Pitch: "You run an oracle node, you earn a share of refresh rewards from every oracle update. It's the same as Ergo's oracle pool but on a new chain where you're an early participant."

### 4. Oracle Operator Software

Your existing ERG/USD oracle bot (Node.js, Windows Task Scheduler, local node + public fallback) is the template. For the new chain:

- Same architecture: fetch price, submit data point, compete for refresh TX
- Change: point at new chain's node API instead of Ergo's
- Change: new oracle pool contract address
- Change: new token ID for the oracle pool NFT/token
- The refresh sniper logic (your v1-v5 Rust sniper) works identically since the contract pattern is the same

---

## Additional Feeds (Post-Launch)

After YourToken/USD is stable, expand to:
- ERG/USD (for cross-chain DeFi pricing)
- BTC/USD, ETH/USD (for options pricing on bridged assets)
- Consider: AVL oracle pool (your multi-datapoint design with 21 feeds) could be deployed here too

---

## What to Deliver

1. **oracle_pool.es** — Ported oracle pool contract with new chain parameters
2. **oracle_operator_guide.md** — Step-by-step for running an operator node
3. **OPERATORS.md** — Committed operator list (pubkeys, contact info, reliability track record)
4. **DATA_SOURCES.md** — Price feed strategy document
5. **oracle_bot/** — Forked operator bot code adapted for new chain
