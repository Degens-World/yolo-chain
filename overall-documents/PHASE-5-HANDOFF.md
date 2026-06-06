# Phase 5 — Local Testnet Boot — Handoff

**Goal:** Boot a single-node SigmaChain testnet from the genesis state already built in Phases 1–4, mine real blocks against it, and run the YoloDAO governance integration test against the live node end-to-end (deposit → vote → execute → redeem).

**End state:**
- `cargo run -p ergo-node -- --config <sigmachain-testnet.toml>` produces blocks on a local network with `Network::SigmaChainTestnet` parameters.
- The AVL+ state digest at height 0 is captured from the running node and pinned on `GenesisParams::sigmachain_testnet` (currently a placeholder).
- The miner-only-storage-rent rule is enforced (`enforce_miner_only_storage_rent: true` at the block-validation boundary for this network).
- `06-governance/tests/integration_test.rs` is forked into a node-talking variant that submits real transactions over the node's REST API and asserts the same lifecycle outcomes.

You are inheriting a project that is **3,832 tests green**, with every consensus rule wired up at the node level but no real chain ever booted yet. Your job is to prove the bytes match the math.

---

## Required reading before you touch anything (~30 min)

In this order:

1. **`/home/cq/.claude/projects/-home-cq-working-files-yolo-chain/memory/MEMORY.md`** — auto-loaded user-feedback memory. Read every linked file. These are non-negotiable rules from prior incidents. The most critical for Phase 5:
   - `feedback_listen_first.md` — diagnose from CODE not production infra. NEVER run state-changing node/wallet commands without explicit approval. Booting a node is state-changing; ask first.
   - `feedback_full_math_v2.md` — compute ALL resource requirements end-to-end; follow the approved plan exactly; ASK about wallet balances upfront.
   - `feedback_never_burn_tokens.md` — CRITICAL. Always preserve ALL tokens from fee/input boxes in change outputs.
   - `feedback_no_push_before_test.md` — never commit/push until the user has tested and confirmed.
   - `feedback_run_all_tests.md` — run the full ergo-ser + ergo-sigma + integration suite after every node-side edit. CSE tweaks silently break other contracts.
   - `feedback_no_coauthor.md` — do not add `Co-Authored-By` lines to commits.
   - `feedback_never_print_secrets.md` — CRITICAL. Never echo, `curl -v`, or log API keys / mnemonics / wallet passwords to terminal output.
   - `feedback_audit_second_order.md` — always ask: "what does this protocol look like if half the validators know the trick?"
   - `feedback_branch_before_main.md` — push to a feature branch first, never directly to main.
   - `project_yolo_node_inheritance.md` — YOLO node inherits Ergo's header struct + Autolykos2 params + Sigma 6.0 ops byte-identically. You do **not** need to re-verify any of this.

2. **`STATUS.html`** (repo root) — current state of the entire project. The "SigmaChain Fork — Phase Progress" section is the most important; everything labelled Phase 0–4.5 is shipped, the "Trustless Peg — Architecture Spike" section is **deliberately shelved** (see "What NOT to touch" below).

3. **`overall-documents/CLAUDE-CODE-FORK-PLAN.md`** — the master plan that produced Phases 1–4.5.

4. **`overall-documents/RUST-NODE-FORK-HANDOFF.md`** — the original handoff that bootstrapped the node fork. Useful for orientation; some items are now done.

5. **`overall-documents/sigmachain-parameter-inventory.md`** — every per-parameter divergence between Ergo and SigmaChain with exact file:line references. This is your map for finding code.

6. **`07-sigmachain-node/UPSTREAM.md`** — fork provenance + how to pull upstream fixes manually (no submodule, no subtree). Important if you find an upstream bug.

7. **`01-emission-tests/SKILL-rust-test-harness.md`** — the test-harness pattern used by all four `0N-*-tests` directories: compile ErgoScript via running Ergo node 6.1.2 at `localhost:9053`, evaluate compiled trees in Rust with `ergo-lib` / YOLO's own `ergo-sigma`. The Phase 5.4 node-talking variant will follow a similar pattern but submit real transactions instead of evaluating in-memory.

8. **`07-sigmachain-node/sigmachain-node/ergo-chain-spec/src/lib.rs`** — the single source of truth for network parameters. `Network::SigmaChainTestnet` is defined here. **Read it; do not modify it on day one.**

---

## Where things live (cheat sheet)

| What | Where |
|---|---|
| The forked node code | `07-sigmachain-node/sigmachain-node/` |
| Chain-spec / network identity | `07-sigmachain-node/sigmachain-node/ergo-chain-spec/src/lib.rs` |
| Genesis state JSON | `07-sigmachain-node/sigmachain-node/test-vectors/sigmachain-testnet/genesis_boxes.json` |
| Compiled YOLO ErgoTrees (emission, treasury, lp) | `07-sigmachain-node/sigmachain-node/ergo-chain-spec/src/yolo_genesis_scripts.rs` |
| Coinbase tx builder (4-output YOLO) | `07-sigmachain-node/sigmachain-node/ergo-mining/src/coinbase.rs` (`build_yolo_emission_tx`) |
| Miner-only storage rent enforcement | `BlockValidationContext::enforce_miner_only_storage_rent` (grep for it) |
| Upstream reference (read-only) | `07-sigmachain-node/arkadianet-ergo-reference/` |
| Genesis tool (already used in Phase 4) | `07-sigmachain-node/sigmachain-genesis-tool/` |
| YoloDAO integration test (in-memory) | `06-governance/tests/integration_test.rs` (actually under `06-governance/contracts/...`) — verify path by grep |
| The Rust test harness skill doc | `01-emission-tests/SKILL-rust-test-harness.md` |
| Memory directory | `/home/cq/.claude/projects/-home-cq-working-files-yolo-chain/memory/` |

---

## Sub-phases (in order)

### 5.1 — Wire `Network::SigmaChainTestnet` through the node config

**Goal.** Make the node selectable for `Network::SigmaChainTestnet` from the same `ergo-node.toml`-style config the upstream Ergo node uses. Right now `Network::SigmaChainTestnet` is defined as an enum variant but you cannot launch a node bound to it — the config plumbing isn't there.

**Concrete steps.**

1. Find the existing config-loading code: `grep -rn 'ergo-node.toml\|Network::Mainnet\|Network::Testnet' 07-sigmachain-node/sigmachain-node/ergo-node/src` and follow the call chain. The upstream supports `mainnet` and `testnet` as string keys — you'll add `sigmachain-testnet`.
2. The string-parse for `Network::SigmaChainTestnet` already exists (`"sigmachain-testnet"` per `chain-spec/src/lib.rs:94`). The question is whether it's wired all the way through to the node-startup boundary.
3. Make a sample `sigmachain-testnet.toml` in the node's example-config directory. **Do not commit a wallet mnemonic or API key into it.** Use placeholders and document the env-var fallback. (See memory rule: never print secrets.)
4. At the SigmaChain validation boundary, flip `enforce_miner_only_storage_rent` to `true`. Grep for that field name; the Phase 4.5 commit (`43372b0`) put the flag in place but defaulted it to `false` on Ergo paths to preserve byte-parity.

**What success looks like.**
- `cargo run -p ergo-node -- --config <path>` parses the toml and reports `network = sigmachain-testnet` in its startup log.
- The full `cargo test -p ergo-ser -p ergo-sigma -p ergo-chain-spec -p ergo-node -p ergo-mining -p ergo-validation` suite still passes — **no regressions on the Ergo paths.** This is the user's `feedback_run_all_tests` rule.

**Pitfalls.**
- The node may have multiple places where network is selected (config-load + p2p-handshake + difficulty-epoch + emission-curve). Make sure all paths agree.
- Do **not** modify `Network::Mainnet` or `Network::Testnet` semantics. Byte-parity with upstream Ergo is a hard constraint per `UPSTREAM.md`.

### 5.2 — Capture + pin the genesis state digest

**Goal.** The AVL+ state root after applying the single genesis emission box. Currently `GenesisParams::sigmachain_testnet` carries a zero placeholder for this. Without the real digest, no node can validate the genesis block.

**Concrete steps.**

1. Read `test-vectors/sigmachain-testnet/genesis_boxes.json` — there is exactly one box at genesis (the emission box; treasury + LP boxes are created fresh by the coinbase emission tx, not at genesis). Understand its registers (R4 = blake2b256 of `treasury_accumulation.es` bytes; R5 = same for `lp_accumulation.es`).
2. There should already be helper code that computes a state digest from a box set — grep for `AvlTree.*from\|StateRoot\|state_root` in `ergo-state` and `ergo-validation`. If a helper exists, use it. If not, the canonical Ergo path is `AVLProver -> performOneOperation(Insert(boxId, boxBytes)) -> rootHash`.
3. Add a one-shot test in `ergo-chain-spec` or `ergo-validation` that:
   - Loads the genesis JSON
   - Computes the state digest
   - Asserts it matches a constant you pin into the codebase
4. Update `GenesisParams::sigmachain_testnet` to use the real digest instead of zero.
5. The arkadianet upstream may have done this for Mainnet/Testnet — check `ergo-chain-spec/src/lib.rs:GenesisParams::ergo_mainnet` / `ergo_testnet` for the reference pattern.

**What success looks like.**
- The pinned digest is reproducible: re-running the computation from the JSON always yields the same hex.
- A node booted with this digest accepts a self-mined block 1 (the first non-genesis block) without "stateRoot mismatch" errors.

**Pitfalls.**
- The state digest is over `(boxId, boxBytes)`, not over the box JSON. Encoding mistakes silently produce wrong digests that look correct.
- Storage rent should NOT yet apply at height 0 (genesis is at height 0).
- Per memory rule `feedback_full_math_v2`: compute everything end-to-end before submitting any tx. Don't iterate against a running node to find the right digest — diagnose from code.

### 5.3 — Boot the node, mine the first blocks

**Goal.** A single-node `sigmachain-testnet` mining its own blocks, producing the 4-output YOLO coinbase, and validating them without errors.

**Concrete steps.**

1. **Ask the user before starting the node.** Per `feedback_listen_first`: state-changing node commands need approval. Specifically confirm:
   - Which directory to put `~/.sigmachain-testnet/` data in
   - Whether to use a fresh wallet or a known mnemonic the user provides (never print it)
   - Which port for P2P and REST (default Ergo is 9020/9052 mainnet, 9030/9053 testnet — YOLO should be its own pair)
2. Generate a mining wallet. Address goes into the coinbase config so `OUTPUTS[3]` of every emission tx pays it.
3. Boot the node with the config from 5.1 + the digest from 5.2.
4. Mine one block. Verify it validates against the node's own validator (`ergo-validation`).
5. Mine ~10 more blocks. Verify the 4-output emission structure on every coinbase:
   - OUTPUTS[0]: continuation emission box, value = input − reward
   - OUTPUTS[1]: treasury accumulation box, value = reward × 10%
   - OUTPUTS[2]: LP accumulation box, value = reward × 5%
   - OUTPUTS[3]: miner reward, value = reward × ~85% (computed as remainder)
6. Trigger the miner-only-storage-rent path: deliberately submit a non-coinbase tx with empty proof + context-extension variable 127 on an input. The block must be rejected with `NonCoinbaseStorageRentClaim { tx_index, input_index }`. If it's *not* rejected, the Phase 5.1 plumbing missed a path.

**What success looks like.**
- 10+ blocks mined in sequence, each validating cleanly.
- Block-rewards match the YOLO emission curve exactly (use the Python model in `01-emission-tests/emission_model.py` as the oracle).
- The storage-rent rejection test produces the expected error code.

**Pitfalls.**
- The YOLO block time is **20 seconds**, not Ergo's 120. Your mining loop may need a faster cadence than upstream defaults.
- Difficulty adjustment epoch is **6,144 blocks** (vs Ergo mainnet 1,024). Don't expect the chain to behave at low heights.
- The `i64` overflow safety margin on emission is 52× — fine for testnet, but compute it explicitly before claiming you've verified it.

### 5.4 — YoloDAO integration test against the live node

**Goal.** Port `06-governance/tests/integration_test.rs` (in-memory sigma-rust evaluation) into a variant that submits real transactions to the running node and asserts the deposit → vote → execute → redeem lifecycle completes correctly.

**Concrete steps.**

1. Read the existing in-memory test to understand the expected lifecycle. Each test step constructs a transaction, evaluates inputs/outputs against the contract semantics, and asserts box-state invariants.
2. For each step, replace the in-memory `evaluate` call with: build an `UnsignedTransaction`, sign with the proving interpreter, POST to the node's `/transactions` endpoint, wait for the next block, fetch the resulting box state via `/utxo`, assert.
3. The `01-emission-tests/SKILL-rust-test-harness.md` doc shows the harness pattern for the in-memory variant. The node-talking variant adds an HTTP layer; otherwise it's the same shape.
4. Cover the same 7 spending paths the existing test covers (approval, execution, cancellation, freeze, migration approval, migration execute, plus deposit and redeem).
5. **Crucial:** make sure every transaction preserves ALL tokens from input boxes in some output. Per `feedback_never_burn_tokens`, this is a critical class of bug — sigma-rust's evaluator will accept token-burning but the node validator might or might not, and even if it does, you've destroyed the user's tokens. Always include change outputs for any tokens not consumed by the script.

**What success looks like.**
- The full lifecycle runs end-to-end against a real testnet, with state-root advancing block by block.
- All 7 governance contracts behave identically on the live node and in the existing in-memory test.
- The test runs deterministically (re-running gives identical box ids and digests, given a fixed initial state).

**Pitfalls.**
- The vYOLO ↔ YOLO ratio in the vault is a moving target if other transactions interleave. The test should either run on a quiet chain (no other txs) or pin a known ratio at each step.
- The voting epoch is 6,144 blocks. The test should NOT require advancing through a real epoch — fast-forward via `setHeight` or test-mode hooks if those exist, or pin a shorter epoch via a test-only config field.
- Per memory rule `feedback_audit_second_order`: ask "what does this look like if a malicious user knows the trick?" for each step. Document any non-obvious assumptions.

---

## What NOT to touch

These exist; leave them alone unless you discover a real bug. If you do, surface it before fixing.

1. **The bridge research arc** — `relay-sketch.es`, `lock-sketch.es`, `mint-sketch.es`, `burn-sketch.es`, `dsp-sketch.es`, `peg-flow.html`, and `07-sigmachain-node/.../tests/bridge_sketches_parse.rs` are all research artifacts from a deliberately shelved spike. They parse-test cleanly through the node but are NOT production contracts and should NOT be deployed or evaluated against the live chain. They will be picked up post-Phase-5.

2. **arkadianet/ergo PR #13** (`https://github.com/arkadianet/ergo/pull/13`) — a v6 method-call tree-version gate fix is open upstream. YOLO's local fork already has the fix on `sigmachain-node-fork` (commit `b45a405`). Do NOT revert it; do NOT reintroduce the gate. Once the PR merges upstream, the next manual rebase per `UPSTREAM.md` will dedup it cleanly.

3. **The on-chain contracts** under `01-emission-tests/`, `02-treasury-tests/`, `03-lp-fund-tests/`, `06-governance/contracts/` — these are audited and tested. They are inputs to your work, not outputs of it. Do not modify them; if your testnet finds a real bug in one, escalate to the user, do not patch silently.

4. **`Network::Mainnet` / `Network::Testnet`** — must remain byte-parity with upstream Ergo. Your work touches `SigmaChainTestnet` only.

5. **Compiled ErgoTree bytes in `yolo_genesis_scripts.rs`** — they were vendored byte-canonical against the Scala oracle in Phase 4.1–4.2. Do not recompile and overwrite unless explicitly asked.

6. **Storage rent params** — pinned in Phase 3.4. The 1-year cycle (1,577,880 blocks) and 312,500 nanoYOLO/byte/period are deliberate.

---

## Workflow conventions

These are the user's rules (from memory). Follow them exactly.

1. **Branch first, push after testing.** Work on a feature branch off `sigmachain-node-fork`. Do not push to `main`. Do not push until the user has tested locally and confirmed.
2. **No `Co-Authored-By` in commits.** Use plain commit messages. Use a HEREDOC for multi-line bodies.
3. **No emojis** in code, commits, or docs unless the user explicitly asks.
4. **Run the full test suite after every change.** `cargo test -p ergo-ser -p ergo-sigma -p ergo-chain-spec -p ergo-node -p ergo-mining -p ergo-validation` at minimum. CSE-style optimizations silently break contracts; the user has been burned by this before.
5. **Never echo or log secrets.** No `curl -v` against authenticated endpoints. No printing of mnemonics, API keys, or wallet passwords to terminal output.
6. **Diagnose from code, not from a running node.** If a test fails, find the root cause in source first. Don't iterate against a live testnet to make a failing test pass by chance.
7. **Ask before state-changing actions.** Booting a node, generating wallet keys, submitting transactions — confirm with the user before doing these for the first time. Approval persists for the scope granted, not beyond.
8. **Use Plan mode for non-trivial implementations.** If a sub-phase reveals more complexity than this doc accounts for, write a plan and get the user's signoff before coding.

---

## Definition of done (the whole of Phase 5)

The minimum bar to call Phase 5 complete:

- [ ] `cargo run -p ergo-node -- --config <sigmachain-testnet.toml>` boots without errors.
- [ ] First-block production verified; the 4-output emission tx structure matches the Python model byte-for-byte at heights 1, 2, ..., 10.
- [ ] Genesis state digest captured + pinned + reproducible from `genesis_boxes.json`.
- [ ] Miner-only storage rent enforced: a non-coinbase rent-claim tx in any block causes block rejection with the expected error.
- [ ] YoloDAO integration test (node-talking variant) passes for all 7 governance paths.
- [ ] Full `cargo test -p ergo-ser -p ergo-sigma -p ergo-chain-spec -p ergo-node -p ergo-mining -p ergo-validation` suite green.
- [ ] No regressions on the Ergo mainnet/testnet paths.
- [ ] STATUS.html updated to reflect Phase 5 done + remaining phases re-prioritized.
- [ ] One commit per logical step (5.1, 5.2, 5.3, 5.4) on a feature branch, awaiting the user's test pass before any push.

---

## When to ask the user

- **Before booting the node for the first time.** State-changing.
- **Before generating wallet keys / mining keys.** Touches local secrets.
- **If the storage-rent params or emission curve appears to need adjustment.** They were pinned with deliberation; any change is a re-do of Phase 3.4 / 3.3.
- **If you find a real bug in the on-chain contracts.** Escalate; do not patch.
- **If a test failure appears non-deterministic** (different result on rerun). The user has had bad experiences with flaky tests; surface and discuss before adding retries or sleeps.
- **Before committing or pushing anything.** Per `feedback_no_push_before_test`.

---

## Closing context

The bridge research arc that immediately preceded this handoff was useful but should not bleed into Phase 5. The chain-side critical path is what unblocks mainnet readiness; the bridge is a v2 concern. The spike already proved the bridge is buildable on the current substrate — that's enough for now.

Phase 5 done means: YOLO is a real chain, producing blocks, with on-chain governance working end-to-end. That's the gate to Phase 6 (docs + STATUS rollover) and then mainnet planning.

Good luck. Read the memory directory. Diagnose from code. Don't push before the user tests.
