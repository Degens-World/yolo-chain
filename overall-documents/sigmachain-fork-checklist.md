# Forking the Ergo Rust Node: Complete Checklist

The first-ever fork of ErgoScript/Sigma into a standalone chain. No prior guide exists for this. This document is the guide.

**Source codebase**: Ergo Rust node (Arkadia or Muad'Dib implementation)
**Target chain**: Standalone Etchash PoW chain with ErgoScript contracts, 15-20 second blocks, aggressive miner-only storage rent, no premine

---

## HIGH-LEVEL OVERVIEW

```
Phase 0: Preparation & Planning
Phase 1: Fork the Rust Node Codebase
Phase 2: Swap the Mining Algorithm (Autolykos → Etchash)
Phase 3: Adjust Consensus Parameters
Phase 4: Build Genesis Block & Emission Contracts
Phase 5: Modify Storage Rent (Aggressive, Miner-Only)
Phase 6: Network Identity (Separate from Ergo)
Phase 7: Wallet & Tooling
Phase 8: Mining Infrastructure
Phase 9: Bridge & Liquidity (AEther / Ergo Connection)
Phase 10: DeFi Stack Deployment
Phase 11: Testnet
Phase 12: Mainnet Launch
```

---

## PHASE 0: PREPARATION & PLANNING

### 0.1 — Secure the Rust Node Fork Source
- [ ] Determine which Rust node to fork: Arkadia's or Muad'Dib's
- [ ] Arkadia: past 1.4M headers, JIT cost parity with Scala sigmastate-interpreter, full difficulty validation through v2 PoW switch and EIP-37
- [ ] Muad'Dib: 700K blocks from stable release tag
- [ ] Evaluate: which has cleaner separation between PoW validation and everything else?
- [ ] Establish relationship with chosen dev(s) — they understand the codebase better than anyone

### 0.2 — Define Chain Parameters (Decisions Needed Before Code)
- [ ] Chain name and ticker symbol
- [ ] Block time: 15 or 20 seconds (recommend starting at 20, can tighten later via miner vote)
- [ ] Etchash DAG starting size: 4GB recommended (gates out ancient 2GB GPUs, keeps 4GB+ population in)
- [ ] DAG growth rate (Etchash default or modified)
- [ ] Emission curve: total supply, block reward, halving interval
- [ ] Emission split: 85% miners / 10% dev treasury / 5% LP fund
- [ ] Storage rent cycle: 6 months or 12 months
- [ ] Storage rent fee rate per byte
- [ ] Initial miner-adjustable parameter ranges
- [ ] Minimum box value (prevents dust spam)

### 0.3 — Legal / Identity
- [ ] Open source license (same as Ergo — CC0 or MIT)
- [ ] Genesis provenance: which external block hashes and news headlines to embed
- [ ] Treasury governance structure: multisig initially? DAO contract timeline?

---

## PHASE 1: FORK THE RUST NODE CODEBASE

### 1.1 — Repository Setup
- [ ] Fork the chosen Rust node repo
- [ ] Create new branch for the new chain
- [ ] Strip any Ergo-specific branding from codebase
- [ ] Update Cargo.toml with new project name, version 0.1.0

### 1.2 — Understand the Code Layout
Map the codebase into functional areas. In any Ergo node (Scala or Rust), the key modules are:

| Module | What It Does | Change Needed? |
|---|---|---|
| **PoW validation** | Verifies block meets difficulty target using Autolykos2 | **YES — replace entirely** |
| **Mining API** | Provides block candidates to miners, accepts solutions | **YES — adapt for Etchash** |
| **Difficulty adjustment** | Calculates next block difficulty | **YES — retune for 15-20s target** |
| **Block structure** | Header format, body format, extension section | **Partial — header fields change** |
| **Transaction validation** | Evaluates ErgoScript, checks sigma proofs | **NO — this stays identical** |
| **Box model** | UTXO creation, spending, register handling | **NO** |
| **Mempool** | Transaction pool management | **NO (maybe tune size limits)** |
| **P2P networking** | Peer discovery, block/tx propagation | **YES — new network magic, seeds** |
| **Storage rent** | Rent eligibility scanning, collection logic | **YES — timing + miner-only rule** |
| **Emission contract** | Coinbase transaction construction | **YES — new schedule + splits** |
| **API / REST** | Node HTTP API for wallets and dApps | **Minimal — mostly unchanged** |
| **Sigma-rust** | ErgoScript interpreter, JIT, sigma proofs | **NO — this is the whole point** |

### 1.3 — Dependency Audit
- [ ] Identify sigma-rust version used (must be 6.0+ for latest ErgoScript features)
- [ ] Verify JIT costing works correctly (Arkadia confirmed parity)
- [ ] Check for any Ergo-specific hardcoded constants (network type, address prefix, etc.)
- [ ] List all external crate dependencies

---

## PHASE 2: SWAP THE MINING ALGORITHM

This is the single largest code change. Everything else is parameter tweaks.

### 2.1 — Remove Autolykos2
- [ ] Locate the Autolykos2 PoW verification function
- [ ] Locate the mining candidate generation code
- [ ] Locate difficulty adjustment algorithm
- [ ] Document the interface between PoW module and the rest of the node (what functions are called, what types are passed)

### 2.2 — Implement Etchash
Etchash is Ethereum Classic's modified Ethash. Well-documented algorithm with existing Rust implementations.

- [ ] Add Etchash crate or implement from spec (ethash algorithm with ETC's modifications)
- [ ] Key reference: `rust-ethash` crate or ETC's specification
- [ ] Implement DAG generation (epoch-based, grows over time)
- [ ] Implement PoW verification: given a block header and nonce, verify the hash meets difficulty
- [ ] Implement mining: generate candidate header, iterate nonces, check against target
- [ ] Configure starting DAG size (4GB) and growth schedule
- [ ] Wire into existing node interface where Autolykos2 was removed

### 2.3 — Block Header Modification
The block header must change to accommodate Etchash-specific fields:

- [ ] Replace Autolykos2 solution fields with Etchash nonce + mixHash
- [ ] Keep all Sigma-relevant header fields: version, parentId, transactionsRoot, stateRoot, timestamp, nBits, height, extensionRoot, votes
- [ ] Update header serialization/deserialization
- [ ] Update header hash function if needed (Etchash uses Keccak-256 for header hash)
- [ ] Update `PreHeader` type in sigma-rust context (this is what ErgoScript contracts see)

### 2.4 — Difficulty Adjustment
Ergo uses a linear least-squares epoch-based adjustment. For 15-20 second blocks:

- [ ] Retune difficulty adjustment for target block time (15 or 20 seconds vs Ergo's 120)
- [ ] Consider: shorter adjustment window since blocks come faster
- [ ] May want to use a different algorithm entirely (e.g., LWMA — Linearly Weighted Moving Average — popular in GPU chains for faster response to hashrate changes)
- [ ] LWMA is better for new chains where hashrate is volatile (miners hopping on/off)
- [ ] Test: simulate difficulty adjustment with wildly varying hashrate

---

## PHASE 3: ADJUST CONSENSUS PARAMETERS

### 3.1 — Block Time
- [ ] Change target block interval constant from 120 seconds to 15-20 seconds
- [ ] Adjust all time-dependent parameters proportionally:
  - [ ] Epoch length (number of blocks per voting epoch) — scale up proportionally so epochs span similar real time
  - [ ] Confirmation depth requirements
  - [ ] Any HEIGHT-based timelock assumptions in predefined contracts

### 3.2 — Block Size & Computational Limits
- [ ] Set initial max block size (can be generous — 15-20s blocks give more throughput headroom)
- [ ] Set initial max computational cost per block
- [ ] These should be miner-adjustable via on-chain voting (copy Ergo's mechanism)
- [ ] Voting epochs: keep similar real-time duration (e.g., if Ergo epochs are 1024 blocks × 2 min = ~1.4 days, your epochs might be 6144 blocks × 20s = ~1.4 days)

### 3.3 — Uncle/GHOST Protocol (Optional but Recommended)
At 15-20 second blocks, orphan rates will be low but non-zero:

- [ ] Decide: include uncle block rewards or not
- [ ] If yes: define uncle inclusion depth (how many blocks back can an uncle be referenced)
- [ ] Define uncle reward (e.g., 50-75% of base block reward)
- [ ] Modify block structure to include uncle header references
- [ ] This adds complexity — can defer to post-launch upgrade if orphan rates are acceptable at 20s

### 3.4 — Miner-Adjustable Parameters
Copy Ergo's voting mechanism but with your chain's values:

- [ ] Computational cost limit (adjustable by simple majority)
- [ ] Block size limit (adjustable by simple majority)
- [ ] Storage rent fee rate (adjustable by simple majority)
- [ ] Foundational changes (block version): require 90% supermajority over 32 epochs
- [ ] Each block header includes vote fields (up to 2 everyday + 1 foundational)

---

## PHASE 4: GENESIS BLOCK & EMISSION CONTRACTS

### 4.1 — Genesis Block Construction
- [ ] Embed provenance data: Bitcoin block hash, Ergo block hash, news headlines from major outlets (same pattern as Ergo's mainnet.conf)
- [ ] Create genesis UTXO set:
  - [ ] Emission box: contains all unmined coins, guarded by emission contract
  - [ ] No other initial boxes (no premine, no team allocation, no investor boxes)
- [ ] Set genesis difficulty (low enough for initial miners, high enough to prevent instant spam)
- [ ] Set genesis timestamp

### 4.2 — Emission Contract (ErgoScript)
This is the most important contract on the chain. You can write this — it's ErgoScript.

```
Block reward schedule example (simple halving):
- Blocks 0 - 1,051,200 (~1 year at 20s): 50 coins/block
- Blocks 1,051,200 - 2,102,400 (~2 years): 25 coins/block  
- Blocks 2,102,400 - 3,153,600 (~3 years): 12.5 coins/block
- ... halving continues

Per block distribution:
- 85% (42.5 coins) → miner
- 10% (5 coins) → treasury contract
- 5% (2.5 coins) → LP fund contract
```

- [ ] Write emission contract enforcing the schedule above
- [ ] Emission contract must be in the coinbase transaction (first TX of each block)
- [ ] Treasury output goes to a treasury box guarded by multisig or DAO contract
- [ ] LP fund output goes to a separate box guarded by LP governance contract
- [ ] Formally verify emission contract (Stainless or equivalent)
- [ ] Unit test emission at every boundary (halving blocks, edge cases)

### 4.3 — Treasury Governance Contract
- [ ] Initial: 2-of-3 or 3-of-5 multisig (known community members)
- [ ] Planned: migrate to Paideia-style DAO once governance token exists
- [ ] Contract must enforce: spending requires proposal + vote + time delay
- [ ] Treasury funds can only be spent according to governance rules

### 4.4 — LP Fund Contract
- [ ] Similar governance to treasury but with narrower mandate
- [ ] Funds can only go to: DEX liquidity pools, market-making contracts, bridge liquidity
- [ ] May want time-locked vesting (e.g., LP fund releases 1/24th per month over 2 years)

---

## PHASE 5: STORAGE RENT (AGGRESSIVE, MINER-ONLY)

This is the novel consensus change that doesn't exist on Ergo.

### 5.1 — Timing Change
- [ ] Change rent eligibility from 4 years (Ergo) to 6-12 months
- [ ] Constant: `StorageRentPeriod` — set in blocks (e.g., 12 months × 365.25 days × 24h × 3600s / 20s = ~1,577,880 blocks)
- [ ] A box becomes rent-eligible when `currentHeight - box.creationHeight > StorageRentPeriod`

### 5.2 — Miner-Only Collection (THE KEY CONSENSUS CHANGE)
On Ergo, anyone can construct a transaction that spends rent-eligible boxes. On your chain, this is restricted to miners via consensus rule:

- [ ] **Consensus validation rule**: A transaction that collects storage rent (spends a box via the rent mechanism) is ONLY valid if it appears in the coinbase transaction (or a designated miner-only transaction slot)
- [ ] Implementation: in the block validation function, check each transaction. If a transaction spends a box via storage rent path, verify it is the coinbase TX or created by the block miner
- [ ] Reject blocks where non-coinbase transactions claim storage rent
- [ ] This is enforced by every validating node — not a contract rule, a consensus rule

### 5.3 — Rent Collection in Mining
- [ ] Miner's block template builder scans UTXO set for rent-eligible boxes
- [ ] Eligible boxes are included in the coinbase transaction
- [ ] Miner collects the rent fee; remainder (if any) is returned to the original owner's address in a new box with reset creation height
- [ ] If the box value doesn't cover rent, miner takes the entire box (same as Ergo)

### 5.4 — Fee Rate
- [ ] Set initial rent fee rate (nanocoins per byte of box storage)
- [ ] Make this miner-adjustable via on-chain voting
- [ ] Start conservative — can increase later via miner vote if needed

---

## PHASE 6: NETWORK IDENTITY

### 6.1 — Network Magic Bytes
- [ ] Change network magic bytes (the prefix on all P2P messages)
- [ ] This prevents your chain's nodes from accidentally connecting to Ergo nodes
- [ ] Typically 4 bytes, must be unique

### 6.2 — Address Prefix
- [ ] Change the address prefix byte
- [ ] Ergo mainnet addresses start with `9` (prefix byte 0x01 for P2PK)
- [ ] Your chain needs a different prefix so wallets can distinguish
- [ ] Choose something recognizable (e.g., a different starting character)

### 6.3 — Network ID
- [ ] Ergo mainnet = 0x00, testnet = 0x10
- [ ] Your chain needs its own network ID byte
- [ ] This is embedded in addresses and used during handshake

### 6.4 — Seed Nodes
- [ ] Set up initial seed nodes (at least 3-5 geographically distributed)
- [ ] Hardcode seed node addresses/DNS in the node config
- [ ] Seed nodes should be reliable infrastructure you or trusted community members control

### 6.5 — Default Ports
- [ ] Change default P2P port (Ergo uses 9030)
- [ ] Change default API port (Ergo uses 9053)
- [ ] Avoids conflicts for people running both Ergo and your chain on the same machine

---

## PHASE 7: WALLET & TOOLING

### 7.1 — Basic Wallet
- [ ] The Rust node should include basic wallet API (send, receive, check balance)
- [ ] Update address generation to use new prefix
- [ ] Test: generate address, send coins, receive coins, check balances

### 7.2 — Explorer
- [ ] Fork an existing Ergo explorer backend
- [ ] Point at new chain's node API
- [ ] Update branding
- [ ] The explorer should work with minimal changes since the box/transaction model is identical

### 7.3 — Web Wallet (Later)
- [ ] Nautilus fork or new wallet using sigma-rust WASM
- [ ] Update for new chain's address format and network ID
- [ ] Not required for launch — miners can use node wallet initially

---

## PHASE 8: MINING INFRASTRUCTURE

### 8.1 — Stratum Protocol
- [ ] Implement or adapt Etchash stratum endpoint
- [ ] Etchash stratum is well-documented (same as Ethash stratum with minor modifications)
- [ ] Miners connect their existing Etchash mining software (ethminer, lolMiner, T-Rex, etc.)
- [ ] The node serves work (block header template + target difficulty)
- [ ] The miner returns nonce + mixHash when solution found

### 8.2 — Mining Pool Software
- [ ] Fork existing open-source Etchash pool software (e.g., open-ethereum-pool adapted for ETC)
- [ ] Modify to talk to your chain's node API instead of an Ethereum/ETC node
- [ ] Key difference: your node uses Ergo-style block structure, not Ethereum block structure
- [ ] The pool needs to construct valid blocks (coinbase TX with emission, storage rent claims, etc.)
- [ ] This is the most nuanced integration piece — pool software must understand Ergo-style coinbase

### 8.3 — Solo Mining
- [ ] Node should support solo mining out of the box
- [ ] Miner connects stratum client directly to node
- [ ] Essential for early chain bootstrapping before pools exist

### 8.4 — Mining Calculator
- [ ] Simple web page: enter your hashrate, see estimated daily earnings
- [ ] Pulls difficulty and block reward from chain API
- [ ] This is marketing, not engineering — but miners won't mine what they can't calculate

---

## PHASE 9: BRIDGE & LIQUIDITY

### 9.1 — Chain Commitment to Ergo
- [ ] Deploy `SideChainState.es` equivalent on Ergo mainchain
- [ ] Or: build trustless relay contract on Ergo that validates your chain's Etchash headers
- [ ] This gives Ergo smart contracts the ability to verify events on your chain

### 9.2 — AEther Integration
- [ ] Coordinate with AEther team (Degens World)
- [ ] Add your chain as a new custody/settlement domain in AEther
- [ ] Path: Your chain → Ergo (chain commitment) → AEther Cosmos layer → IBC Eureka → Ethereum/L2s
- [ ] The native token needs to be representable as a wrapped asset on EVM chains
- [ ] Define token standard on your chain (same as Ergo's EIP-4 token standard since you're using same box model)

### 9.3 — Rosen Bridge (Backup/Parallel)
- [ ] Rosen can also bridge your chain directly since it only requires threshold signature support
- [ ] Sigma protocols provide this natively
- [ ] Two bridge options is better than one — users choose based on speed/cost preference

### 9.4 — Initial Liquidity Deployment
- [ ] Use LP fund to seed initial liquidity pools
- [ ] Priority pairs: YourToken/ERG, YourToken/SigUSD, rsYourToken/ETH (on Ethereum DEX)
- [ ] Coordinate LP seeding with bridge activation — tokens need to be bridgeable before EVM LP makes sense

---

## PHASE 10: DEFI STACK DEPLOYMENT

All ErgoScript. All portable. You can do this.

### 10.1 — Oracle Pool
- [ ] Deploy oracle pool contract (you already operate an oracle node)
- [ ] Initial feed: YourToken/USD price
- [ ] Bootstrap with 3-5 oracle operators minimum
- [ ] Oracle data is needed for: stablecoin, options pricing, DeFi generally

### 10.2 — DEX (Spectrum-style AMM)
- [ ] Deploy constant-product AMM contracts (Spectrum's contracts are open source ErgoScript)
- [ ] Initial pools: YourToken/SigUSD equivalent
- [ ] LP incentives from the 5% LP fund

### 10.3 — Stablecoin
- [ ] Deploy SigmaUSD (AgeUSD) variant or Dexy variant
- [ ] Needs oracle feed from 10.1
- [ ] SigmaUSD is simpler and more conservative
- [ ] Dexy is more capital efficient but more complex

### 10.4 — Options (Etcha Port)
- [ ] Port Etcha contracts to the new chain
- [ ] Contracts are identical ErgoScript — just deploy on the new chain
- [ ] Needs oracle feed for Black-Scholes pricing

### 10.5 — Lending (SigmaFi Port)
- [ ] Port SigmaFi P2P lending contracts
- [ ] No oracle dependency — purely collateral-based

### 10.6 — Governance (Paideia Port)
- [ ] Deploy DAO governance for treasury management
- [ ] Migrate treasury from multisig to DAO once governance token is distributed

---

## PHASE 11: TESTNET

### 11.1 — Private Testnet
- [ ] 3-5 nodes run by you and trusted parties
- [ ] Mine blocks, test transactions, test contracts
- [ ] Verify: Etchash mining works, difficulty adjusts correctly, emission schedule is correct
- [ ] Verify: storage rent collection works and is miner-only
- [ ] Verify: sigma-rust contract validation produces identical results to expected behavior
- [ ] Stress test: high transaction volume, large blocks, many simultaneous miners

### 11.2 — Public Testnet
- [ ] Open testnet to community
- [ ] Faucet for test tokens
- [ ] Mining open to anyone
- [ ] Bug bounty for consensus issues
- [ ] Run for minimum 2-4 weeks with no consensus failures before mainnet

### 11.3 — Bridge Testing
- [ ] Test AEther integration end-to-end on testnet
- [ ] Move test tokens: your chain → Ergo → Cosmos testnet → Ethereum testnet
- [ ] Verify: assets arrive, amounts correct, refund path works

---

## PHASE 12: MAINNET LAUNCH

### 12.1 — Pre-Launch
- [ ] Finalize all parameters (no changes after genesis)
- [ ] Deploy seed nodes (5+ geographically distributed)
- [ ] Publish node binary and source code
- [ ] Publish mining guide (how to point Etchash miner at your chain)
- [ ] Publish mining calculator
- [ ] Announce launch date and genesis block time

### 12.2 — Genesis
- [ ] Genesis block mined with provenance proof (BTC hash, ERG hash, news headlines)
- [ ] Emission contract live
- [ ] Treasury and LP fund contracts live
- [ ] Storage rent consensus rule active from block 0

### 12.3 — Post-Launch (First Week)
- [ ] Monitor: block production, difficulty adjustment, orphan rates
- [ ] Monitor: storage rent not being claimed by non-miners (consensus rule working)
- [ ] Mining pool goes live (or solo mining guide available)
- [ ] Explorer live
- [ ] Bridge activation (AEther connection to Ergo)

### 12.4 — Post-Launch (First Month)
- [ ] Seed initial DEX liquidity pools
- [ ] Deploy oracle pool
- [ ] First DeFi contracts (AMM, basic lending)
- [ ] Community growth: mining community, holders, DeFi users

---

## WHAT YOU CAN DO vs. WHAT NEEDS A RUST DEV

### You (ErgoScript / Business / Community)
- All emission, treasury, LP fund contracts (Phase 4)
- Storage rent parameter design (Phase 5 — design, not consensus code)
- All DeFi contract deployment (Phase 10)
- Oracle pool operation (Phase 10.1)
- Mining calculator and community tools (Phase 8.4)
- Explorer setup (Phase 7.2 — fork + configure)
- Bridge coordination with AEther team (Phase 9)
- Chain parameter decisions (Phase 0.2)
- Community building, marketing, miner outreach
- Testnet coordination and testing

### Rust Developer(s) Needed
- Fork and modify the Rust node (Phases 1-3)
- Etchash implementation and integration (Phase 2)
- Miner-only storage rent consensus rule (Phase 5.2)
- Network identity changes (Phase 6)
- Stratum endpoint (Phase 8.1)
- Mining pool software adaptation (Phase 8.2)
- Block header serialization changes (Phase 2.3)
- Difficulty adjustment retuning (Phase 2.4)
- Genesis block construction tooling (Phase 4.1)

### Either / Collaborative
- Wallet updates (Phase 7)
- Testnet operation (Phase 11)
- Bridge integration (Phase 9 — contract side is ErgoScript, plumbing side is Rust/infra)

---

## TIMELINE ESTIMATE

| Phase | Duration | Dependencies |
|---|---|---|
| Phase 0: Planning | 2 weeks | Decisions only |
| Phase 1-3: Node fork + Etchash | 6-10 weeks | Rust dev(s), stable source node |
| Phase 4: Genesis & emission | 2-3 weeks (parallel) | ErgoScript (you) |
| Phase 5: Storage rent mod | 1-2 weeks | Rust dev, included in Phase 1-3 |
| Phase 6: Network identity | 1 week | Rust dev, included in Phase 1-3 |
| Phase 7: Basic wallet/explorer | 2 weeks | After node works |
| Phase 8: Mining infra | 3-4 weeks | After node mines blocks |
| Phase 9: Bridge setup | 2-4 weeks | AEther team coordination |
| Phase 10: DeFi deployment | 4-6 weeks | After mainnet, ongoing |
| Phase 11: Testnet | 4-6 weeks | After node + mining work |
| Phase 12: Mainnet | 1 week | After testnet stable |

**Critical path**: Phases 1-3 (node fork) → Phase 11 (testnet) → Phase 12 (launch)
**Parallel work**: Phases 4, 5, 8, 9, 10 can all progress alongside node development

**Realistic total: 4-6 months from "Rust node stable release" to mainnet launch** — assuming 1-2 Rust devs working on it consistently, with you handling contracts and community in parallel.

Given Arkadia and Muad'Dib are both weeks-to-months from stable releases on the Ergo Rust node, the starting gun is closer than it's ever been.
