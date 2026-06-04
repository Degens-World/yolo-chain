# Rust Node Fork — Handoff

**Goal:** Fork [arkadianet/ergo](https://github.com/arkadianet/ergo) (v0.3.0 Rust Ergo node) into a SigmaChain node that produces blocks, runs the YOLO emission contract from genesis, and accepts the YoloDAO governance contracts already written.

**End state:** A local testnet node producing blocks, the existing integration test passing against the real node (not just sigma-rust evaluator), and a documented fork process someone else could repeat.

---

## What you're inheriting

This isn't a green-field project. The on-chain side is **done**:

- **7 governance contracts** in `06-governance/contracts/` — all compiled, audited 8.5-9/10, 60 tests passing
- **Emission contract** in `01-emission-tests/emission.es` — 50 coins/block, 1-year halvings, 1-coin tail emission
- **Treasury & LP multisig contracts** in `02-treasury-tests/`, `03-lp-fund-tests/` — production-ready
- **Storage rent economic model** in `04-storage-rent/` — parameters locked
- **Python lifecycle models + Rust integration tests** — full end-to-end coverage

You should not touch any of this code. The fork work is **node-level**, not contract-level. Your job is to make the node honor the parameters and contracts that already exist.

**Required reading before starting (15-20 min):**
1. `README.md` (root) — the project pitch
2. `STATUS.html` — current state of everything
3. `overall-documents/sigmachain-framework.md` — the fork plan from when no Rust node existed
4. `overall-documents/sigmachain-fork-checklist.md` — pre-existing checklist of what needs to change
5. `01-emission-tests/emission.es` and `01-emission-tests/emission_model.py` — the monetary policy you must implement
6. `04-storage-rent/RECOMMENDATIONS.md` — storage rent parameters

---

## The upstream repo

[github.com/arkadianet/ergo](https://github.com/arkadianet/ergo) — **read the README in full before forking.**

**Key facts:**
- Pre-1.0 alpha. Don't use for production funds. For our fork's testnet, that's fine.
- Rust 1.95.0 pinned via `rust-toolchain.toml`
- Build: `cargo build --release -p ergo-node -p ergo-wallet`
- Dual MIT/Apache 2.0 licensed — we can fork freely
- Mainnet sync verified to height 1,771,976 on 2026-04-26
- External miner REST protocol works (requires utxo state mode)
- sigma-rust used only as dev/test oracle, not in consensus

**Architecture (inferred — verify when you have the code):**
- `ergo-node` crate = the node binary
- `ergo-wallet` crate = HD wallet + multi-sig
- Other crates likely: consensus, networking, storage, PoW verification, transaction pool

---

## The fork plan

### Phase 0: Validate upstream (Day 1)

Before changing anything, prove arkadianet/ergo works as advertised:

```bash
git clone https://github.com/arkadianet/ergo arkadianet-ergo-reference
cd arkadianet-ergo-reference
git log --oneline -10            # confirm v0.3.0 tag
cat rust-toolchain.toml          # confirm 1.95.0
cargo build --release -p ergo-node -p ergo-wallet
# Builds clean? Note any warnings.
```

Try syncing against Ergo mainnet:
```bash
# Read docs/configuration.md first — find the right config layout
./target/release/ergo-node --config <some-mainnet-config>
# Watch it sync. If it reaches a reasonable height in a few hours, we're good.
```

**Deliverable:** A confirmation that v0.3.0 actually works on a fresh machine. If it doesn't, file an issue upstream and pause — we need to understand why before forking.

### Phase 1: Inventory consensus parameters (Day 2)

Find every hardcoded constant that defines the chain identity. Don't change anything yet — just list them.

**Things to find (likely locations in parentheses):**
- Network magic bytes (likely in a `network` or `protocol` crate)
- Genesis block hash / genesis state digest
- Block reward function (does the node read this from a contract or have it hardcoded?)
- Block time target (Ergo: 120s, YOLO: 20s)
- Halving interval (Ergo: 524,288 blocks ≈ 2yr, YOLO: 1,577,880 blocks ≈ 1yr)
- PoW algorithm (Autolykos2)
- Address prefix bytes (mainnet 0x00, testnet 0x10)
- Default port numbers
- DNS seed list
- Storage rent rate (Ergo: 1,250,000 nano/byte per 4yr = 312,500/yr, YOLO matches)
- Storage rent cycle (Ergo: 4yr, YOLO: 1yr)
- Storage rent collection rules (Ergo: anyone; YOLO: miner-only)
- Minimum box value (both: 360,000 nanocoins)
- ErgoTree version flags / activation heights
- Network protocol version

**Deliverable:** A markdown file at `overall-documents/sigmachain-parameter-inventory.md` listing every hardcoded value, its current Ergo value, the target YOLO value, and the file/line where it lives.

### Phase 2: Set up the SigmaChain fork (Day 3)

```bash
# Create the fork repo
git clone https://github.com/arkadianet/ergo sigmachain-node
cd sigmachain-node
# Add upstream as a remote so we can pull in future fixes
git remote add upstream https://github.com/arkadianet/ergo
git checkout -b sigmachain-main
```

**Repository placement decision:** Should this be a separate repo (`Degens-World/sigmachain-node`) or a subdirectory in `yolo-chain`? Probably a separate repo — node binaries deserve their own release cycle. Discuss with project lead before deciding.

Update top-level metadata:
- `Cargo.toml` — rename to `sigmachain-node`, bump version to 0.1.0
- `README.md` — replace with SigmaChain pitch (reference `yolo-chain/README.md`)
- License files — keep dual MIT/Apache, add copyright notice for SigmaChain modifications

### Phase 3: Apply parameter changes (Days 4-6)

Work through the inventory from Phase 1, one parameter at a time. After each change:
- Run `cargo build --release` — must build clean
- Run `cargo test` — must pass (or note which tests need parameter updates)

**Order of changes (least risky first):**

1. **Network magic + ports + address prefixes** — chain identity. Changing these means the SigmaChain node won't talk to Ergo nodes, which is what we want.
2. **Block time 120s → 20s** — affects difficulty adjustment, timestamp validation, mempool TX expiry.
3. **Halving interval 2yr → 1yr** — affects emission curve.
4. **Storage rent cycle 4yr → 1yr** — affects rent collection.
5. **Storage rent collection: anyone → miner-only** — this is a consensus rule change. Find where rent collection authorization lives and add the miner-only constraint. Document this carefully — it's the unique-to-YOLO rule.
6. **Genesis box value + genesis state** — the genesis emission box must hold the full YOLO supply (177,412,882.5 coins = 177,412,882,500,000,000 nanocoins).

**Don't change yet:** PoW algorithm. Defer Autolykos2 → Ethash to a separate decision (see analysis after this handoff).

### Phase 4: Genesis configuration (Day 7)

Build the genesis block setup:

1. **Genesis emission box** — must use the emission.es ErgoTree (already compiled — see `01-emission-tests/`)
2. **Treasury accumulation box** — initial empty box at `treasury_accumulation.es` script hash
3. **LP fund accumulation box** — initial empty box at `lp_accumulation.es` script hash
4. **YoloDAO contracts** — the 7 governance contracts deploy in a separate genesis tx after the chain starts (see `06-governance/HANDOFF.md` Phase 5)

The arkadianet node likely reads genesis from a config file or has it hardcoded. Find the genesis loader, add a SigmaChain genesis profile.

### Phase 5: Local testnet (Days 8-10)

Spin up a local single-node testnet:

```bash
./target/release/sigmachain-node \
  --config testnet.conf \
  --network testnet \
  --mining-enabled \
  --internal-miners 1
```

- Mine blocks locally
- Verify emission distribution (85/10/5 split actually happens)
- Verify halving transition by jumping to height 1,577,880 in config
- Test storage rent collection at height 1,577,880 (12 months)

**Critical validation:** Take the YoloDAO integration test from `06-governance/tests/integration_test.rs`, swap out the sigma-rust evaluator calls for real node-submitted transactions, run the full deposit → vote → execute → redeem lifecycle against the testnet.

### Phase 6: Document & hand back (Day 11-12)

- Write `sigmachain-node/README.md` — how to build, configure, run
- Write `sigmachain-node/docs/genesis.md` — how the genesis state was constructed
- Write `sigmachain-node/docs/fork-from-arkadianet.md` — what changed from upstream and why
- Push to remote
- File a status update in `yolo-chain/STATUS.html`

---

## Key constraints

**You cannot break Ergo consensus parity by accident.** The arkadianet node was carefully built to match Scala Ergo. Every parameter you change is an intentional divergence — document it. Every parameter you don't change should continue to behave exactly like Ergo (which is what we want for ErgoScript contract execution).

**The contracts are immutable.** If a contract test fails after a node change, the node is wrong, not the contract. The contracts are what they are — your job is to make the node honor them.

**Don't optimize prematurely.** Get a working local testnet first. PoW algorithm choice, mining pool integration, bridge integration, and exchange listings all come later.

**Ask before destructive moves.** If a parameter change requires touching consensus-critical code in a way that could create a fork from Ergo for the wrong reason, flag it and discuss. We want intentional divergence, not accidental.

---

## What to file as an issue (or ask the user about)

When you hit these decisions, stop and ask:
1. **Repo layout** — separate `sigmachain-node` repo or subdirectory in `yolo-chain`?
2. **PoW algorithm** — keep Autolykos2 (faster ship, GPU-mineable, but shares Ergo hashrate) or swap to Ethash (more work, but isolates from Ergo)? Analysis pending.
3. **Pre-mine for testnet validators** — do any boxes exist at genesis besides the emission, treasury, LP, and YoloDAO singletons? Probably no, but confirm.
4. **Mining bounty / activation incentive** — is there a plan for getting initial miners onboard? Affects how we configure difficulty adjustment.
5. **Upstream contributions** — if you find bugs in arkadianet/ergo while testing, file them upstream (good citizenship) before working around them.

---

## Validation checklist

Before declaring the fork "done":

- [ ] Builds clean on Rust 1.95.0 with `cargo build --release`
- [ ] All upstream tests still pass (or document why they don't apply)
- [ ] Local single-node testnet produces blocks
- [ ] Block reward is 50 coins, splits 85/10/5
- [ ] Halving boundary test (jump to height 1,577,880 and mint block — should pay 25 coins)
- [ ] Storage rent collection works (skip ahead 1,577,880 blocks on a dust box, verify miner can claim)
- [ ] Storage rent collection rejects non-miner claims (the YOLO-unique rule)
- [ ] Genesis emission box deployed with correct YoloDAO emission.es script
- [ ] Treasury accumulation box receives 10% of block reward
- [ ] LP accumulation box receives 5% of block reward
- [ ] Can deploy the 7 YoloDAO contracts in a post-genesis tx
- [ ] Full deposit → vote → execute → redeem lifecycle works on the real node
- [ ] Two nodes can peer with each other on the SigmaChain network (not Ergo)
- [ ] A SigmaChain node REFUSES to peer with an Ergo node (different network magic)

---

## Resources

| Resource | Where |
|----------|-------|
| arkadianet/ergo repo | https://github.com/arkadianet/ergo |
| arkadianet/ergo docs | `docs/configuration.md`, `docs/architecture.md` in the repo |
| Ergo Scala node (reference) | https://github.com/ergoplatform/ergo |
| Ergo node config reference | https://docs.ergoplatform.com/node/install/ |
| Ergo "fork your own chain" guide | https://docs.ergoplatform.com/node/testnet/mine-your-own-chain/ |
| Existing YOLO contracts | `01-emission-tests/`, `02-treasury-tests/`, `03-lp-fund-tests/`, `06-governance/` |
| Storage rent decisions | `04-storage-rent/RECOMMENDATIONS.md` |
| Original fork checklist | `overall-documents/sigmachain-fork-checklist.md` |
| Local node API endpoint (Ergo mainnet, for testing) | http://localhost:9053 |

---

## Mindset

This is not a research project — the design decisions are made. Your job is execution. Read the upstream code carefully, change the minimum that needs changing, document everything, and produce a testnet that runs the existing contracts.

Estimated effort: **8-12 working days** for a competent Rust developer who can also read the Scala reference when needed. Faster if arkadianet/ergo's documentation is comprehensive; slower if we hit undocumented Ergo consensus details.

When in doubt, the existing Ergo Scala node (`ergoplatform/ergo`) is the ground truth. The arkadianet Rust node is the implementation we're forking. The YOLO contracts are the constraints. Everything else is engineering.
