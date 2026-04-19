# SigmaChain Framework: Building a New PoW Sidechain

A practical reference for launching a Sigma-powered Proof of Work sidechain, synthesized from the Sigma Deck, Kushti's forum posts (Chain Commitments, Refactoring Plan), the ergohack-sidechain codebase, ErgoHack VII/VIII results, and community dev discussions through early 2026.

---

## 1. Architecture Decision: Three Deployment Models

Kushti's Chain Commitments post (Jul 2024) defines three distinct SigmaChain configurations. The choice determines every downstream engineering decision.

### A. Merge-Mined Sidechain (Lowest Barrier)

Ergo miners simultaneously mine the sidechain. Mainchain commitments arrive trivially because the Ergo block header is embedded in the sidechain header. Sidechain state is posted on Ergo via an NFT-tracked box that only the block miner can update.

- **Chain commitment direction**: mainchain → sidechain is implicit (header embedding); sidechain → mainchain uses a miner-updatable state box (`SideChainState.es` from ErgoHack VII)
- **Trust model**: Inherits Ergo's hashrate security directly
- **Existing code**: `contracts/MainChain/SideChainState.es`, `contracts/MainChain/Unlock.es`, `contracts/SideChain/Unlock.es`, `contracts/SideChain/UnlockComplete.es`, `DoubleUnlockPrevention.es`
- **Kushti's estimate**: Weeks for a developer with sigma-rust knowledge (Jul 2025 Ergoversary)
- **Con**: Tied to Autolykos2 / Ergo mining pools — no new ASIC audience

### B. Dedicated PoW Chain with Trustless Relay

A standalone blockchain running its own PoW algorithm (SHA-256, Scrypt, etc.) with trustless relays on both sides. This is the model the Sigma Deck pitches to Bitmain — support existing ASIC product lines.

- **Chain commitment direction**: Requires building a trustless relay on both chains. Easy for Autolykos2-based or simple PoW (sha256) sidechains. Kushti noted relay contracts for Bitcoin headers were demonstrated at ErgoHack VIII (`BtcRelay.es`, `BtcTxCheck.es`)
- **Funding mechanism**: Part of the sidechain's emission rewards mainchain miners for posting correct sidechain data on Ergo (better economics than Bitcoin's BIP-301 since rewards are constant)
- **Con**: Full node implementation required (Rust or Scala), relay contracts need per-algorithm work

### C. Dedicated Non-PoW Chain (Hybrid/PoS)

Also possible — sidechain could run PoS or hybrid consensus. The emission contract on the sidechain still rewards Ergo mainchain miners for posting correct data via relay. Included for completeness but not the Sigma Deck's focus.

---

## 1b. The Braid Precedent: Double Merge Mining Bitcoin + Ergo

The most concrete SigmaChain effort to date is **Braid**, a double merge-mined sidechain secured by both Bitcoin and Ergo hashrate simultaneously. Key details from community discussions:

- **Concept**: A single hash search advances Bitcoin, Ergo, and the Braid sidechain. Miners commit work to all three chains in one step. An attack on any one faces the combined defense of the shared energy pool.
- **Blog post**: "Braiding Lunarpunk and Solarpunk through Merged Mining" (Sep 2, 2025) describes dual modes — "solarpunk" (transparent governance) and "lunarpunk" (censorship-resistant privacy).
- **Dark tokens and private circuits**: Braid supports dark tokens and private transaction circuits. Assets can flow between transparent and private states. A stablecoin could be transparent in one jurisdiction and privacy-preserving in another.
- **Dark DeFi**: Kushti described (Jul 2025 Ergoversary) moving transparent ERGs to a sidechain and using them in dark contracts, potentially incorporating FCMP (Full Chain Membership Proofs) from Monero's upcoming updates.
- **Partner discussions**: As of Jul 2025, Braid was being discussed with "big Bitcoin mining pools" for support. The explicit goal was to avoid building something with no outside interest.
- **Better Money Labs**: The entity behind the Braid whitepaper development.

The Braid model is directly relevant to the Sigma Deck's pitch — it demonstrates how existing ASIC hardware (SHA-256 Bitcoin miners) can secure a Sigma-powered chain without switching algorithms.

**Litecoin/Dogecoin analogy** (repeatedly cited by Kushti): Dogecoin, as a merge-mined sidechain of Litecoin, is now the primary advertised use case for Scrypt ASICs. Litecoin's security budget benefits enormously from Dogecoin's merge-mined rewards. The same dynamic is the goal for Ergo + SigmaChains.

---

## 2. The Refactoring Plan (What Needs to Change in Code)

Kushti's June 2025 refactoring plan identifies two layers of modification:

### SigmaState (Contractual Layer)

The sigma-state interpreter contains Ergo-specific types that must be parameterized for new chain environments:

| Entity | Module | What to Change |
|---|---|---|
| `Header` / `PreHeader` | `SHeader`, `SPreHeader` | Chain-specific block header structure |
| `Box` | `SBox` | Storage unit — may need different register layout or size constraints |
| `MinerPubkey` | AST node | Different key format if different PoW |
| `Height` | AST node | Block height semantics |
| `sigma.Context` | Context class | Chain-specific context variables |

Kushti recommends tagging all context-dependent entities with a special trait, then moving them to dedicated packages so they're easy to find and swap.

Changes must propagate to: SigmaJS (JavaScript), compiler, and sigma-rust.

### Ergo Node (or New Node)

Two implementation paths:

**Path 1 — Rust (recommended for ASIC chains)**
- Use sigma-rust for the contractual layer (already translated)
- Adopt networking/consensus from an existing Rust PoW client (e.g., if targeting SHA-256, borrow from a Bitcoin Rust implementation)
- Fastest path for chains similar to existing PoW networks

**Path 2 — Scala (fork from Ergo node)**
- Derive from Ergo node code directly
- Mark and isolate PoW-related functions (mining API, PoW verification) into swappable packages
- In most cases, a dedicated SigmaChain in Scala only needs to change the PoW function
- More familiar to existing Ergo developers but heavier

For merge-mined sidechains, no Ergo node modifications needed — the existing `/mining/candidateWithTxs` API is sufficient for a merge-mining client to include sidechain block data in mainchain transactions.

---

## 3. Chain Commitments: The Cross-Chain Plumbing

### Merged Mining Flow

1. Miner calls Ergo node's `/mining/candidateWithTxs` to include sidechain block data in the Ergo block
2. Sidechain block header = Ergo block header + box proof + box data (sidechain state committed via registers)
3. `SideChainState.es` contract on Ergo: NFT-tracked box, updatable only by Ergo block miner, combined with emission logic

### Dedicated Chain Relay Flow

1. **Sidechain → Ergo**: Build a relay contract on Ergo that validates the sidechain's PoW headers. ErgoHack VIII's `BtcRelay.es` demonstrates this for Bitcoin (processes submitted headers, builds commitment to best chain, operates as SPV client). Same pattern works for any simple PoW algorithm.
2. **Ergo → Sidechain**: If sidechain supports Ergo header verification natively in Sigma, this is trivial. Otherwise, need equivalent relay logic on the sidechain.
3. **Incentivization**: Sidechain emission contract rewards mainchain miners for posting correct sidechain headers on Ergo. Constant emission rewards provide better economic security than Bitcoin's BIP-301 approach.

### Two-Way Peg Transfer Flow

From the ErgoHack VII contracts:

1. **Lock on main**: User sends ERG to `Unlock.es` contract on Ergo — funds locked, unlockable only with proof of corresponding sidechain deposit
2. **Mint on side**: Sidechain contract reads mainchain state (via chain commitment), issues equivalent sERG to user's sidechain address
3. **Burn on side**: User sends sERG to sidechain burn contract (`SideChain/Unlock.es` → `SideChain/UnlockComplete.es`), which records the burn
4. **Unlock on main**: Once mainchain contract sees proof of burn + sufficient confirmations, releases ERG back to user
5. **Double-unlock prevention**: `DoubleUnlockPrevention.es` prevents replay

---

## 4. Design Decisions for a New Chain

### 4.1 PoW Algorithm Selection

The Sigma Deck explicitly proposes letting Bitmain choose the algorithm based on demand for their current ASIC product lines. Practically:

- **SHA-256**: Largest installed ASIC base (Bitcoin). Relay contract already prototyped (`BtcRelay.es`). Risk: SHA-256 chains compete directly with Bitcoin for hashrate.
- **Scrypt**: Litecoin/Dogecoin ASIC base. Less competition for hashrate.
- **Ethash/Etchash**: Orphaned GPU/ASIC hardware post-Ethereum merge. Kushti specifically mentioned "taming orphaned hardware" as a use case.
- **Autolykos2**: Ergo's own algorithm. Merge mining is trivial. Smallest ASIC market.
- **Custom**: New algorithm designed for specific ASIC constraints. Highest barrier but cleanest competitive positioning.

### 4.2 Emission Schedule

Sigma enables smart contract-controlled emissions without a premine. Key design parameters:

- **Miner share vs. treasury split**: Ergo's model allocated ~10% to ecosystem development (now exhausted). A new chain can tune this.
- **On-chain governance of treasury**: Paideia-style DAO tooling is already portable.
- **Formal verification**: Ergo's emission logic was verified with the Stainless tool. Same approach available for new chains.
- **Genesis provenance**: Ergo embedded Bitcoin/Ethereum block hashes and newspaper headlines in the pre-genesis state as proof of no premine. Worth replicating.

### 4.3 Network Parameters

Following Ergo's design pattern, make key parameters miner-adjustable via on-chain voting:

- Block size limits
- Computational cost limits  
- Storage rent fees
- Foundational changes (block version): 32-epoch voting, 90% supermajority
- Everyday changes (block size): simple majority

### 4.4 Storage Rent

One of Ergo's strongest differentiators for long-term sustainability. Storage rent (demurrage) charges a fee on unspent outputs after 4 years. Configurable per-chain: the rent period, fee rate, and minimum box value can all be tuned for different economic models.

---

## 5. Portable DeFi Stack

The Sigma Deck's core thesis is that the entire Ergo DeFi stack is portable logic — deploy once on Ergo, redeploy on any SigmaChain with minimal changes. What's available today:

| Protocol | What It Does | Portability Notes |
|---|---|---|
| SigmaUSD (AgeUSD) | Overcollateralized stablecoin | Needs oracle pool on new chain |
| Dexy (USE) | Seigniorage stablecoin w/ LP reference market | Needs AMM pool + oracle |
| Spectrum DEX | AMM / constant product LP pools | Core contracts portable, needs liquidity bootstrap |
| SigmaFi | P2P collateralized bonds/lending | Fully portable |
| ErgoRaffle | Crowdfunding with lottery | Fully portable |
| ErgoPad | Project incubator / IDO platform | Portable, needs governance token |
| Paideia | On-chain DAO governance | Portable |
| Options (SigmaO / Etcha) | Decentralized options trading | Needs oracle for pricing |
| ChainCash | L2 peer-to-peer monetary system | Gold-denominated, requires trust graph |
| Oracle Pools | Decentralized price feeds | Must bootstrap operator set |
| Rosen Bridge | Cross-chain bridge (Ergocentric) | Bridge to sidechain from day one if Ergo-pegged |
| SmartPools / Lithos | Decentralized mining pools | Key for ASIC chains — prevents pool centralization |

---

## 5b. Dark DeFi: Privacy-First Sidechain Applications

A unique capability of SigmaChains that EVM chains cannot replicate natively:

**Core concept** (Kushti, Ergoversary 2025): Move transparent ERGs to a sidechain and use them in dark contracts — private DeFi applications where transaction amounts, participants, and contract logic are shielded.

**Technical foundation** (from EKB sigma-protocols concept doc): Sigma protocols enable composable zero-knowledge proofs using AND/OR/threshold logic. Ring signatures via OR-composition of Diffie-Hellman tuples mean the verifier cannot determine which party signed. `atLeast(k, n)` threshold proofs reveal only that the threshold was met, not which k participants signed. ErgoMixer already demonstrates non-interactive mixing via ZeroJoin protocol.

**Privacy sidechain possibilities**: Private transfers using ErgoMixer patterns or private amounts via advanced cryptography; FCMP (Full Chain Membership Proofs) from Monero's updates; dark tokens flowing between transparent and private states (per Braid blog); jurisdiction-aware privacy where a stablecoin is transparent in one market and privacy-preserving in another.

**Why this matters for the pitch**: This is a differentiator no EVM chain offers natively. Ethereum's account model fundamentally leaks identity. Sigma's UTXO model + native sigma protocols enable application-level privacy without third-party infrastructure.

---

## 6. Step-by-Step: What It Takes to Launch

Based on our prior research into blockers and Kushti's estimates:

### Phase 0: Specification (2–4 weeks)
- Choose deployment model (merge-mined vs. dedicated)
- Select PoW algorithm
- Design emission schedule and treasury split
- Define initial network parameters
- Write chain specification document

### Phase 1: SigmaState Modifications (4–8 weeks)
- Fork sigmastate-interpreter (or sigma-rust)
- Tag and isolate context-dependent entities
- Implement new `Header`/`PreHeader` types for chosen PoW
- Modify `Box`, `MinerPubkey`, `Height`, `Context` as needed
- Propagate changes to compiler and SDK

### Phase 2: Node Implementation (8–16 weeks)
- **If Rust**: Adopt networking layer from existing Rust PoW client, plug in modified sigma-rust
- **If Scala**: Fork Ergo node, swap PoW packages, modify genesis/emission contracts
- **If merge-mined**: Build merge-mining client only (much shorter — weeks not months)
- Implement chain commitment contracts (relay or merge-mine state box)

### Phase 3: Two-Way Peg Contracts (2–4 weeks)
- Deploy lock/unlock contracts on both chains (ErgoHack VII templates)
- Deploy double-unlock prevention
- Test transfer flows end-to-end on testnet

### Phase 4: Mining Infrastructure (4–8 weeks)
- Pool software integration (Stratum protocol adaptation)
- Mining pool UI for chain selection (Kushti's vision: pools just toggle chains on/off)
- SmartPool / Lithos deployment for decentralized pooling
- Test with target ASIC hardware

### Phase 5: DeFi Bootstrap (4–8 weeks)
- Deploy oracle pool with initial operators
- Deploy AMM (Spectrum-style) with seed liquidity
- Deploy stablecoin (SigmaUSD or Dexy variant)
- Connect Rosen Bridge for cross-chain transfers

### Phase 6: Governance & Launch (2–4 weeks)
- Deploy Paideia or equivalent DAO for treasury management
- Finalize mainnet parameters
- Genesis block with provenance proof
- Mainnet launch

**Total realistic timeline**: 6–12 months for a dedicated chain with a funded team of 3–5 developers. Merge-mined sidechain could be significantly faster (2–4 months) given the existing code.

---

## 7. Key References

| Resource | URL / Source |
|---|---|
| SigmaChains Pt.1: Chain Commitments (Kushti, Jul 2024) | https://ergoforum.org/t/sigmachains-pt-1-chain-commitments/4817 |
| SigmaChains Refactoring Plan (Kushti, Jun 2025) | https://ergoforum.org/t/sigmachains-refactoring-plan/5167 |
| ErgoHack VII Sidechain Repo | https://github.com/ross-weir/ergohack-sidechain |
| ErgoHack VII Sidechain Whitepaper | `docs/whitepaper/sidechain.pdf` in above repo |
| ErgoHack VII Sidechain Video | https://www.youtube.com/watch?v=G6xggrwA8ys |
| ErgoHack VIII Bitcoin Relay Contracts | `contracts/relay/BtcRelay.es`, `BtcTxCheck.es` in above repo |
| ErgoHack VIII Relays Slides | `docs/relays.pdf` in above repo |
| Sigma Deck (Bitmain Grant Proposal) | Uploaded PDF (this analysis) |
| Ergoversary 2025 — Kushti Vision Talk | https://youtube.com/watch?v=m6CEAdaYRME |
| Sigma Trees — Kushti, Ergoversary 2024 | https://youtube.com/watch?v=deXi73K4Z0k |
| Braiding Lunarpunk and Solarpunk (Braid Blog) | https://ergoplatform.org/en/blog/Braiding-Lunarpunk-and-Solarpunk-through-Merged-Mining |
| How Sigma Chains Will Bring Bitcoin to Ergo (Blog) | https://ergoplatform.org/en/blog/How-Sigma-Chains-Will-Bring-Bitcoin-To-Ergo |
| Weekly AMA Jun 12 2025 (merge mining estimate) | https://youtube.com/watch?v=92S1h09xOUs |
| Weekly AMA Jul 17 2025 (pool UI vision) | https://youtube.com/watch?v=KldESqqDQgs |
| Ergoversary Special Jul 1 2025 (coding started) | https://youtube.com/watch?v=Ku3fHTUagEs |
| Braid discussion Jul 10 2025 AMA | https://youtube.com/watch?v=ztE1JMS9sEc |
| Lithos + Merge Mining (Ergo Meetup Oct 2025) | https://youtube.com/watch?v=xmAaEmiP3-U |
| 2025 Year in Review / 2026 Outlook | https://youtube.com/watch?v=zVKtvaLDwjE |
| Sigma 6.0 Release (Jun 2025) | sigmastate-interpreter repository |
| Dev Chat Jan 2026 W03 (Scala vs Rust for Braid) | Ergo Developer Telegram |
| Dev Chat Jan 2026 W02 (Sidechain implementation language) | Ergo Developer Telegram |

---

## 8. Open Questions & Blockers (Updated via MCP Sources)

These remain unresolved as of early 2026:

1. **Scala vs. Rust for the reference client** — Still open as of Jan 2026 W03. Kushti posed the question directly in dev chat; community feedback (Josemi) suggested Rust, but no final decision was recorded. Luivatra vibe-coded an entire Ergo node in Rust, but Kushti flagged "a lot of hard forkish divergences with the reference client" and questioned whether it's safe for miners to run. Sigma-rust 6.0.x has deserialization versioning issues and a parsing divergence that broke Spectrum DEX backend (fixed by switching to develop branch).

2. **Mining pool adoption** — Kushti envisions a UI where mining pools just toggle chains on/off (Jul 17, 2025 AMA). Not built yet. Lithos (decentralized mining protocol) confirmed compatibility with merge mining — it uses separate stratum addresses per chain, so miners can mine both simultaneously (Ergo Meetup Oct 2025).

3. **sigma-rust completeness** — Multiple "hard-fork-ish divergences" noted between Rust and Scala implementations (Jan 2026). Luivatra stated "will take a bit to ensure complete parity" but gave no timeline. The u64 storage rent encoding and sigma-rust interpreter differences are specific known issues.

4. **Economic modeling** — The Litecoin/Dogecoin analogy is compelling but not a substitute for proper modeling. No published analysis exists of optimal emission curves for ASIC-backed SigmaChains with different hardware cost structures. The treasury split needs modeling per-algorithm.

5. **Relay contract generalization** — `BtcRelay.es` proves the concept for SHA-256. Each new PoW algorithm needs its own relay contract. No generic relay framework exists, though the pattern is well understood.

6. **Sub-blocks interaction** — The Matrix whitepaper was published Jan 2026 W02. Devnet testing ongoing but unstable — 30s block interval caused fork proliferation, 60s stabilized but sync issues persisted. Sub-blocks activation "probably another year or so" per community discussion. New SigmaChains should plan for eventual sub-block support but not gate on it.

7. **Merge mining coding status** — As of Jul 1, 2025 Ergoversary, Kushti confirmed coding had already started. By Jun 12, 2025 AMA, he estimated "weeks, maybe with a couple of developers or even one with knowledge of Sigma Rust" for a merge-mined sidechain. However, no public merge-mined sidechain has launched as of Apr 2026.

8. **Braid's status** — Whitepaper was in progress as of Jul 2025, with Bitcoin mining pool discussions ongoing. Sep 2025 blog post published. No public mainnet or testnet launch reported through early 2026. The generalization of Braid into a reusable SigmaChain framework is the next logical step (per Jan 2026 W03 dev chat where Kushti asked about Scala vs Rust for "Braid generalization").

---

## 9. What the Sigma Deck Gets Right (and What It Doesn't Say)

**Strengths of the pitch:**
- The EVM-vs-Sigma framing is accurate: functional/declarative UTXO contracts avoid entire classes of reentrancy and gas-manipulation exploits
- Portable DeFi logic across PoW chains is a real differentiator vs. fragmented EVM-compatible PoS chains
- Storage rent solves Bitcoin's long-term miner incentive problem
- SmartPools address mining centralization risk inherent in ASIC chains
- No-premine with smart emission treasury is a credible middle ground

**Gaps the framework must address:**
- The deck doesn't specify *which* algorithm or *why* — it punts to Bitmain. A real framework needs economic analysis per algorithm.
- Cross-chain DeFi composability is hand-waved. In practice, oracle bootstrapping and initial liquidity are the hardest problems.
- The "Simga" typo on slide 4 aside, the deck conflates Sigma (the cryptographic protocol family) with ErgoScript (the language) with SigmaState (the interpreter). The framework needs to be precise about which layer is being modified.
- No discussion of MEV, front-running, or transaction ordering — issues that ASIC miners with high hashrate will inevitably encounter on a DeFi-capable chain.
- The deck is dated (references crypto market cap under $1T, ~2022–2023 era). Current market context is very different.
