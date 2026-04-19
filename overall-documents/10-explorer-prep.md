# Handoff: Explorer Backend Preparation

**Checklist Reference**: Phase 7.2
**Owner**: You (research + planning), implementation after node exists
**Blockers**: Soft — can research now, can't test without running node
**Dependencies**: Running node with API
**Deliverable**: Documented modification plan for forking an Ergo explorer

---

## Objective

Prepare to deploy a block explorer for the new chain. Since the new chain uses the same box model, transaction format, and API structure as Ergo, an Ergo explorer fork should work with minimal changes. The goal now is to understand the codebase and document what needs changing so deployment is fast once the node exists.

---

## Explorer Options to Fork

### Option A: Ergo Explorer (ergo-platform/explorer-backend)
- Scala/Play Framework backend
- PostgreSQL database
- Indexes all blocks, transactions, boxes, tokens
- REST API for frontend
- Already understands Ergo's box model natively
- Con: Scala — harder to modify if you're not a Scala dev

### Option B: Sigmaspace Explorer
- Alternative explorer implementation
- Check: is it open source? What's the tech stack?

### Option C: Build minimal explorer from scratch
- Use sigma-rust to scan chain, index into PostgreSQL or SQLite
- Build simple REST API
- More work but you control everything
- Could leverage your existing data infrastructure experience (Supabase, GitHub Actions pipelines)

**Recommendation**: Option A (fork ergo-platform/explorer-backend) unless the Rust node devs have an explorer in mind. It's the most battle-tested and already handles the full box/token/transaction model.

---

## What Needs Changing in an Ergo Explorer Fork

### Definitely Change

| Component | Current (Ergo) | New Chain | Effort |
|---|---|---|---|
| Node API URL | Points to Ergo node | Point to new chain node | Config change |
| Network ID | Ergo mainnet (0x00) | New chain's network ID | Config/constant |
| Address prefix | `9` (Ergo P2PK) | New prefix | Serialization code |
| Genesis block hash | Ergo's genesis | New chain's genesis | Config |
| Block time display | ~120 seconds | 15-20 seconds | Display logic |
| PoW display | Autolykos2 difficulty/hashrate | Etchash difficulty/hashrate | Display + calculation |
| Branding | Ergo logos, naming | New chain branding | Frontend assets |

### Probably Change

| Component | Notes |
|---|---|
| Hashrate calculation | Different algorithm means different hash/difficulty relationship |
| Uncle block display | If GHOST protocol implemented, explorer needs to show uncle blocks |
| Storage rent indicators | Could add: "rent eligible in X blocks" countdown on boxes |
| Mining stats | Etchash-specific stats (DAG epoch, effective hashrate) |

### Should Not Change

| Component | Why It Stays |
|---|---|
| Box display (registers, tokens, value) | Identical box model |
| Transaction display (inputs, outputs, data inputs) | Identical transaction model |
| Token indexing (EIP-4) | Same token standard |
| Address generation/display | Same crypto, different prefix only |
| ErgoScript decompilation | Same contract language |
| Search functionality | Same identifiers (box ID, TX ID, address) |

---

## Research Tasks (Do Now)

### 1. Clone and Read the Explorer Backend Code
```bash
git clone https://github.com/ergoplatform/explorer-backend
```
- Map the directory structure
- Identify where network-specific constants live
- Identify where the node API client is configured
- Identify where address serialization happens (for prefix change)
- Identify where hashrate/difficulty calculations happen

### 2. Document the Database Schema
- What tables exist?
- How are blocks, transactions, boxes, tokens stored?
- Are there any Ergo-specific columns that would need modification?
- Can the schema be used as-is with just different data?

### 3. Identify Frontend Dependencies
- What frontend does the explorer use?
- Is it a separate repo?
- What components show PoW-specific data?

### 4. Test Locally Against Ergo
- Run the explorer locally against an Ergo node
- Understand the sync process
- Identify: how long does initial sync take? What are the resource requirements?
- This gives you familiarity before swapping in the new chain's node

---

## Storage Rent Enhancement (Unique Feature)

Since aggressive storage rent is a defining feature of the new chain, the explorer should highlight it:

- **Box detail page**: show "Rent eligible: Yes/No" and "Rent due in: X blocks (~Y hours)"
- **UTXO statistics page**: show total rent-eligible boxes, total value at risk, recent rent collections
- **Miner page**: show rent revenue per block alongside block rewards and TX fees
- **Dashboard**: rent collection rate as a network health metric

This is new UI that doesn't exist in any Ergo explorer. Design it after launch, but document the concept now.

---

## What to Deliver

1. **EXPLORER_AUDIT.md** — Code review of ergo explorer-backend: what changes, what doesn't
2. **CONSTANTS_MAP.md** — Every constant/config that needs changing, with old and new values
3. **SCHEMA_REVIEW.md** — Database schema analysis, any needed modifications
4. **RENT_DISPLAY_SPEC.md** — UI spec for storage rent features (mockups optional)
5. **DEPLOYMENT_PLAN.md** — Infrastructure: where to host, expected resource requirements, sync time estimate
