# Phase 5.4 — YoloDAO Integration Against the Live Node — Handoff

**Goal:** Fork `06-governance/tests/integration_test.rs` from in-memory sigma-rust evaluation into a node-talking variant. Each test step builds a real transaction, signs it via the running SigmaChain node's wallet, POSTs it to `/transactions`, waits for the next block, and asserts the resulting box state through `/utxo`. Cover the full YoloDAO lifecycle: deposit → vote initiation → counting → validation → execution → redeem.

**End state:**
- A new test crate (or test module under the existing `yolo_governance` crate) that drives the running node through the YoloDAO contracts end-to-end.
- The test passes against a `cargo run -p ergo-node -- --config sigmachain-testnet.toml` with `mining.enabled = true` and the bundled CPU miner producing blocks on demand.
- The seven canonical spending paths covered: deposit, redeem, vote initiation, vote counting, vote validation, treasury withdrawal, proposal execution. Plus the two edge paths the in-memory test currently glosses over: vote cancellation and the elevated 90 % threshold for withdrawals exceeding 10 % of treasury.
- A test-only mechanism to fast-forward height through the contract-enforced voting/counting/execution windows without grinding ~20 000 real blocks.

You are inheriting a project where Phase 5 is **merged** (PR #1, commit `0aefb57` on `sigmachain-node-fork`). A single-node SigmaChain testnet boots, mines, and produces a byte-correct YOLO coinbase against the Python emission oracle. **3,843 tests green workspace-wide.** Your job is to make the YoloDAO governance system run end-to-end against that live chain.

---

## Required reading before you touch anything (~30 min)

In this order:

1. **`/home/cq/.claude/projects/-home-cq-working-files-yolo-chain/memory/MEMORY.md`** — auto-loaded user-feedback memory. Read every linked file. These are non-negotiable rules from prior incidents. The most critical for Phase 5.4:
   - `feedback_never_burn_tokens.md` — **CRITICAL.** Every YoloDAO tx involves multiple token classes (vault NFT, reserve NFT, vYOLO, counter NFT, proposal state token, vote NFT). For each input box, every token must appear either consumed by the contract or in a change output. A test that burns tokens passes in-memory but loses real value on the live chain.
   - `feedback_listen_first.md` — diagnose from CODE not production infra. NEVER run state-changing node/wallet commands without explicit approval. Booting the node, generating wallet keys, and submitting transactions are all state-changing.
   - `feedback_audit_second_order.md` — for every spending path: "what does this look like if half the voters know the trick?" The YoloDAO contracts have subtle invariants (split-math for treasury withdrawals, validation-vote tallies vs total-vote tallies, deadline windows) that are easy to verify wrong against a friendly test.
   - `feedback_no_price_guessing.md` — vYOLO peg is currently 1:1 by contract; if you ever find code computing a ratio, the contract is the oracle, not the test.
   - `feedback_run_all_tests.md` — full `cargo test` after every node-side edit. The Phase 5.3 candidate-builder branches are now load-bearing for SigmaChain bring-up; any regression there breaks the chain.
   - `feedback_no_coauthor.md` — no `Co-Authored-By` lines in commits.
   - `feedback_never_print_secrets.md` — **CRITICAL.** The integration test needs a signing wallet. Never echo, `curl -v`, or log the mnemonic / password.
   - `feedback_branch_before_main.md` — push to a feature branch first, never directly to `sigmachain-node-fork`. Phase 5.4 will likely want its own branch + PR.

2. **`STATUS.html`** (repo root) — current state of the entire project after the Phase 5 rollover. The "SigmaChain Fork — Phase Progress" section is the most important; Phases 5.1 → 5.3 are now in the done track, Phase 5.4 sits under "What's Next."

3. **`overall-documents/PHASE-5-HANDOFF.md`** — the Phase 5 handoff. Read sections "Where things live" and "Workflow conventions"; both still apply. Section "5.4" describes the original Phase 5.4 sketch (less detailed than this doc supersedes).

4. **The Phase 5 merge commit (`0aefb57`)** — `git show 0aefb57 --stat`. It rolls up the 8 commits that brought up the chain. Pay attention to `93dab09` (5.3d-i) — the YOLO emission dispatch in `MiningHandle` is consequential for what block 1's coinbase looks like, and the bootstrap `last_headers` fallback in `candidate.rs` is the kind of gate Phase 5.4 may re-encounter (see "Known patterns" below).

5. **`06-governance/tests/integration_test.rs`** — the existing in-memory integration test. It is the **specification** for what Phase 5.4 must reproduce against the live node. Note that the file is at `06-governance/tests/integration_test.rs` (not under `contracts/` as the Phase 5 handoff suggested). It contains exactly one test function — `full_lifecycle_deposit_propose_vote_execute_redeem` — that chains four `eval()` calls: deposit, treasury withdrawal, proposal execution, redeem. Read the full file. Note the four `/tmp/*_tree.hex` constants on lines 31–34: those are pre-compiled placeholder ErgoTrees that the test assumes already exist on disk. The token-ID baking issue (see §5.4.2 below) is the reason.

6. **`06-governance/contracts/`** — the seven YoloDAO contract sources (`.es` files). Read each one once end-to-end. The cheat sheet in §2 below is no substitute for reading the actual scripts; the contracts ARE the spec.

7. **`06-governance/PARAMETERS.md`** and **`06-governance/HANDOFF.md`** — the YoloDAO-side handoffs that produced the contracts. Useful for the per-contract reasoning (why the elevated 90 % threshold, why the 12 960-block voting window, why the 1 080-block counting phase).

8. **`01-emission-tests/SKILL-rust-test-harness.md`** — the test-harness pattern used for the in-memory variant: compile ErgoScript via a running Ergo node 6.1.2 at `localhost:9053`, evaluate compiled trees in Rust via `ergo-lib` 0.28's `TestProver`/`TestVerifier`. Phase 5.4's node-talking variant follows the same shape on the contract-compilation side and replaces evaluation with HTTP round-trips to the SigmaChain node.

---

## Where things live (cheat sheet)

| What | Where |
|---|---|
| Existing in-memory integration test | `06-governance/tests/integration_test.rs` (lines 1–239) |
| YoloDAO contracts | `06-governance/contracts/{vault,reserve,treasury,counting,proposal,userVote,timeValidator}.es` |
| Pre-compiled placeholder ErgoTrees | `/tmp/*_tree.hex` (paths hardcoded in `integration_test.rs:31–34`) |
| YoloDAO Cargo manifest | `06-governance/Cargo.toml` |
| YoloDAO handoffs / params | `06-governance/HANDOFF.md`, `06-governance/PARAMETERS.md` |
| Live SigmaChain node | `07-sigmachain-node/sigmachain-node/` |
| Sample testnet config | `07-sigmachain-node/sigmachain-node/ergo-node/sigmachain-testnet.toml` |
| Bundled CPU miner | `07-sigmachain-node/sigmachain-node/ergo-mining/examples/sigmachain_cpu_miner.rs` |
| Wallet API surface | `07-sigmachain-node/sigmachain-node/ergo-api/src/wallet/` (lifecycle, reads, sending) |
| Transaction submit | `07-sigmachain-node/sigmachain-node/ergo-api/src/compat/transactions.rs` (POST `/transactions`, `/transactions/bytes`) |
| Box lookup | `/utxo/byId/{boxId}`, `/utxo/genesis` (`ergo-api/src/server.rs:332-333`); indexer-backed `/blockchain/box/byId/{boxId}` at `server.rs:585` |
| Memory directory | `/home/cq/.claude/projects/-home-cq-working-files-yolo-chain/memory/` |

---

## Sub-phases (in order)

### 5.4.1 — Live single-tx smoke test

**Goal.** Before touching governance: prove the round-trip `build → wallet sign → POST /transactions → mine → /utxo confirms the new box` works against the live SigmaChain node. This is the harness the rest of Phase 5.4 is built on; nailing it standalone keeps the YoloDAO integration debuggable.

**Concrete steps.**
1. Boot a clean SigmaChain testnet per `sigmachain-testnet.toml`. Init a fresh wallet via `POST /wallet/init` (see the Phase 5.3b session for the exact dance). Mine ~10 blocks via `cargo run -p ergo-mining --example sigmachain_cpu_miner -- --max-blocks 10` so the miner reward box at height 1 is past its 720-block maturity gate (the reward script `reward_output_script` locks the box for 720 blocks; **this matters** — see Pitfalls).

   Wait actually: the 720-block gate is mainnet Ergo's default. SigmaChain's `monetary.miner_reward_delay` is 4,320 blocks (`MonetaryParams::sigmachain_testnet()` at `ergo-chain-spec/src/lib.rs:422`). At 20 s blocks that's a day. **For testing**, the easiest path is either (a) bump the chain past 4,320 blocks with the CPU miner before the smoke test (~24 minutes at 1 block/s), or (b) fund the wallet via a one-off tx that spends from a different source (e.g. the bootstrap mint, see §5.4.2). Confirm which path you're using before writing the test.
2. Build a tx that sends `1 nanoYOLO` from the wallet's change address to itself. Use the wallet API path: `POST /wallet/transaction/send` with a `PaymentRequestDto { address, value: 1, assets: [] }`. The wallet handles `build → sign → submit` internally. Capture the returned `tx_id`.
3. Wait for the next block. The CPU miner is the simplest pump — `cargo run … --max-blocks 1`. The tx should be included in the very next block once the mempool sees it.
4. Confirm via `GET /transactions/byId/{tx_id}` that the tx is mined, and via `GET /utxo/byId/{output_box_id}` that the new box exists. Hash the response, compare against the bytes you constructed.
5. Wrap all of the above in a single Rust test (use `reqwest` or the `ureq` blocking client — neither is in workspace deps so this lives in a new test crate with its own `Cargo.toml`).

**What success looks like.**
- One test function: `live_node_single_tx_round_trip`.
- Run via `cargo test -p yolo-governance-node-tests live_node_single_tx_round_trip --features live` (or similar) — gated behind a feature flag so CI without a running node skips it cleanly.
- Test docs explicitly say "requires `ergo-node --config sigmachain-testnet.toml` running on `127.0.0.1:9054`."

**Pitfalls.**
- The 4,320-block reward-delay gate is the single biggest "surprise nothing works" trap. If `/wallet/balances` reports 0 confirmed YOLO, you can't spend.
- The wallet API's `api_key` header is lowercase-underscore (`api_key: hello`), not `X-Api-Key`. Already documented in 5.3's session log.
- The wallet's `change_address` only exists AFTER `/wallet/unlock`; before unlock it's the empty string. Don't cache it across boots.

### 5.4.2 — Token-ID bake: deploy the YoloDAO genesis tokens

**Goal.** The seven YoloDAO contracts embed token-ID constants at compile time (`StateNftId`, `ReserveNftId`, `VYoloId`, `CounterNftId`, `ValidVoteId`, etc.). The in-memory test gets away with placeholder values because `/tmp/*_tree.hex` was compiled once with stubs. The live-node test cannot use stubs — every token referenced has to actually exist on chain. So Phase 5.4 needs a "deploy the YoloDAO genesis tokens" step that creates them and captures their IDs.

**Concrete steps.**
1. Read `06-governance/PARAMETERS.md` for the full list of token classes and required quantities (vYOLO supply target, counter singleton, 5 vault/reserve pairs, etc.).
2. Build a single funding tx that mints all the YoloDAO genesis tokens in one shot. The Ergo convention is "the tx id of the box that creates a token IS the token id" — so all token IDs are derivable from this one tx id.
3. Submit the funding tx, mine a block, capture all token IDs from the resulting outputs.
4. Re-compile each `.es` contract via the running Ergo node at `localhost:9053` (per `01-emission-tests/SKILL-rust-test-harness.md`), substituting the captured token IDs for the placeholder constants. Cache the resulting hex bytes — these are the **production-shaped** ErgoTrees for SigmaChain.
5. Persist the bake in a way the integration test can read: either re-write `/tmp/*_tree.hex` (matching the in-memory test's convention) or — better — write a deployment JSON file under `06-governance/test-vectors/sigmachain-deployment.json` that the test reads.

**What success looks like.**
- One Rust binary or test helper that: (a) builds the funding tx, (b) submits it, (c) waits for inclusion, (d) reads back the token IDs, (e) shells out to the Ergo node to compile each contract with substituted constants, (f) persists the deployment artifact.
- Re-running the binary on a fresh chain produces a *different* set of token IDs (token IDs are tx-id-derived, and the funding tx's id depends on the parent state), but the contract shapes are identical.
- The in-memory test's `/tmp/*_tree.hex` constants either stay as a separate "stub" deployment for the in-memory variant, OR get replaced by the production deployment with the in-memory test calling the same helper.

**Pitfalls.**
- The Ergo node compile path at `/script/p2sAddress` uses `treeVersion: 0` for backward compatibility. SigmaChain runs on `block_version = INTERPRETER_60_VERSION = 4`. Verify the resulting hex parses cleanly through SigmaChain's `ergo-ser::read_ergo_tree` before trusting it.
- Don't burn tokens. The funding tx mints 5 vault NFTs, 5 reserve NFTs, vYOLO supply, etc. Every minted token must appear in an output box owned by someone — typically the wallet that mined block 1 holds them until they're moved into the contract-locked vault/reserve pairs.
- The miner-only-storage-rent rule (Phase 4.5 / 5.1) is now ACTIVE on SigmaChain. Don't accidentally craft a non-coinbase tx with input variable 127 + empty proof.

### 5.4.3 — Vault deposit + redeem (the 1:1 peg)

**Goal.** Walk through the four-actor cycle for ONE vault/reserve pair: wallet locks YOLO into the vault, receives vYOLO from the reserve, then later returns the vYOLO and recovers the YOLO. Mirrors `integration_test.rs` STEP 1 and STEP 4 (`full_lifecycle_deposit_propose_vote_execute_redeem`).

**Concrete steps.**
1. After §5.4.2, the wallet should hold all 5 vault NFTs (qty 1 each), 5 reserve NFTs (qty 1 each), and the full vYOLO supply. Move ONE vault NFT into a box guarded by `vault.es` (with value 0 nanoYOLO at first — the vault is empty). Move the matched reserve NFT + full vYOLO supply into a box guarded by `reserve.es`.
2. Construct the deposit tx: input = (vault box, reserve box, wallet funding box with enough YOLO); outputs = (new vault box with value += deposit_amount, new reserve box with vYOLO -= deposit_amount, recipient box holding the vYOLO, miner fee, change).
3. The vault and reserve contracts each enforce conservation independently — `deltaVaultYolo + deltaReserveVYolo == 0`. Verify by computing both sides from the constructed tx before submitting.
4. Submit via `/transactions`, mine a block, verify the deposit succeeded by reading `/utxo/byId/{vault_box_id}` and confirming the value increased.
5. Reverse the operation: redeem path. Wallet returns vYOLO to the reserve, reserve releases the YOLO from the vault. Same conservation check.

**What success looks like.**
- Two test functions: `deposit_increases_vault_yolo_and_burns_vyolo_from_reserve` and `redeem_returns_yolo_and_increments_reserve_vyolo`.
- Both run against the live node, both pass.
- Post-redeem, the vault is back to zero value and the reserve has the full vYOLO supply. **Peg invariant verified end-to-end:** `circulating_vyolo == sum(vault_values)`.

**Pitfalls.**
- The vault and reserve both check `singleVaultInput` / `singleReserveInput` — only one of each may be spent per tx. Don't try to bundle multi-vault operations.
- The reserve box has *two* token entries: `tokens(0) = (ReserveNftId, 1)` and `tokens(1) = (VYoloId, N)`. The contract checks `tokens.size == 2` at line 73 of `reserve.es`. Forgetting the second token or adding a third both reject.
- vYOLO is a regular `id_bytes` Ergo token, NOT a wrapped native asset. The wallet API treats it the same as any other token. Sign/submit as normal.

### 5.4.4 — The voting lifecycle: initiation → counting → validation

**Goal.** Walk the counter box through phases 1 → 2 → 3 of `counting.es`. Requires a successfully-submitted proposal box at qty 1, voter boxes that stake vYOLO, the counter box accumulating tallies, and finally the proposal advancing to qty 2 when thresholds pass. Phase 5.4's hardest piece because of the height-window arithmetic.

**Concrete steps.**
1. **Test-mode height plumbing.** The voting/counting/validation windows in `counting.es` are 12 960 / 1 080 / 4 320 blocks (lines 44–47). Grinding through real blocks at 20 s/block takes ~3.7 days for one full cycle. Two viable shortcuts:
   - **Recommended:** Add a `[testing]` section to the node config with a `voting_window_override: u32` that, when present, replaces the contract-pinned constant at *contract-compilation* time. Compile a test-only build of each `.es` with `votingWindow = 5` (or similar). This is the cleanest path because nothing on the node side needs to change.
   - **Alternative:** Compile a test-only ChainSpec variant with the regular windows but expose a CPU-miner flag `--instant-mine N` that mines N blocks back-to-back without the 1 000 ms `block_candidate_generation_interval_ms` debounce. At 100 H/s on CPU (genesis difficulty) you can crank ~600 blocks per second; even 12 960 blocks takes ~25 s. This is more invasive.
   - The recommended approach lets you keep production-shaped contracts on the live testnet for any real use case and *only* use the test-compiled variant for §5.4.4 onward. Decide which path before coding.
2. Initiation. Proposer wallet builds a tx with: input = (counter box, vYOLO stake of 100 000+, proposer's change box); outputs = (counter box at phase 2, proposer-locked stake box, proposal box at qty 1, change). Counter's R4 advances to `HEIGHT + votingWindow`. Submit + mine + verify counter NFT moved to the phase-2 successor.
3. Voting. Voter wallets each build a tx with the timeValidator contract: input = (timeValidator box with locked vYOLO, vote NFT); output = (userVote box that the counter will consume). Submit at least 2-3 voter boxes so phase 3's quorum and support thresholds can be exercised.
4. Counting. Fast-forward to within the counting window (`HEIGHT >= voteDeadline && HEIGHT < voteDeadline + countingPhase`). Build a tx with: inputs = (counter box phase 2, all voter boxes); outputs = (counter box with R7/R9 updated, voter vYOLO returned to each voter, vote NFTs burned). Submit + mine.
5. Validation. Fast-forward into the validation window. Read the counter's R7 (total) and R9 (yes). Construct the proposal-advancement tx: input = (counter phase 2, proposal qty 1); outputs = (counter phase 3 with tallies reset, proposal qty 2). The contract checks `validationVotes * 10_000 >= totalVotes * supportBps` (lines 187–193 counting.es). Submit + mine + verify the proposal's token qty is now 2.

**What success looks like.**
- Test functions: `proposal_initiation_advances_counter`, `vote_counting_burns_vote_nfts_and_accumulates_tallies`, `validation_passes_proposal_to_qty_2_when_thresholds_met`, `validation_keeps_proposal_at_qty_1_when_quorum_fails`, `validation_uses_elevated_threshold_for_high_proportion_proposals`.
- All pass against the live node.
- Per the memory rule: every test asserts that all input tokens are accounted for in outputs OR explicitly consumed by the contract logic. The "vote NFTs burned during counting" path is the ONE case where a token is intentionally not in any output; assertion goes "this NFT no longer exists" by checking `/utxo/byId/{vote_nft_token_id}` returns 404.

**Pitfalls.**
- The counter's R5 stores `(proportion, votesInFavor)`. Watch the second element — phase 3 reads `R5._2` to compute the threshold, and an off-by-one in your test fixture's vote tally will produce a confusing "validation rejected" error.
- `counting.es` line 95 checks `totalVotes == 0 && validationVotes == 0` before allowing initiation. If a prior test left the counter in a partially-reset state, you can't restart. Each test should set up its own counter box state via direct creation if needed (skip the contract on setup).
- The "elevated 90 %" path (line 195 counting.es: `isProportionElevated`) fires when the proposal's R4 proportion > 1 000 000 (= 10 % of `Denom`). Write the test fixture for this case carefully; the proportion encoding is `proportion / Denom` where `Denom = 10_000_000`.

### 5.4.5 — Treasury withdrawal + proposal execution

**Goal.** Once a proposal reaches qty 2, the treasury can be spent against it. This is the on-chain governance outcome — the test's whole point. Covers the in-memory test's STEP 2 (treasury withdrawal) and STEP 3 (proposal execution).

**Concrete steps.**
1. Carry forward from §5.4.4: the proposal box is at qty 2, treasury holds some YOLO. Build the withdrawal tx: inputs = (proposal qty 2, treasury); outputs = (treasury successor with reduced value, recipient box at the address from proposal R5, change). The treasury contract enforces split-math (lines 107–109) for proportional disbursement; verify your test's expected output values use the same arithmetic.
2. Submit + mine + read back: `GET /utxo/byId/{treasury_successor_id}` confirms the new treasury value, `GET /utxo/byId/{recipient_box_id}` confirms the recipient received the right amount.
3. The proposal execution path (`proposal.es` lines 86–103) requires the proposal state token to be burned. The treasury withdrawal tx is the burn point: the proposal box is consumed, its token doesn't reappear in any output. Add the assertion.
4. The "new-treasury-mode" path (treasury.es lines 138–158) fires when `proportion == Denom` (full migration). Worth a separate test once the basic withdrawal works.

**What success looks like.**
- Test functions: `treasury_withdrawal_against_passed_proposal_disburses_proportionally`, `proposal_execution_burns_state_token`, `treasury_migration_at_full_proportion_transfers_nft`.
- All pass.
- Split-math correctness: the test fixture computes expected disbursement using the same `wholePart + remainderPart` formula treasury.es uses (lines 107–109), not the naive `value * proportion / Denom`. The two diverge when `value > 10_000_000` and rounding kicks in.

**Pitfalls.**
- The `if (hasProposalToken)` guard at treasury.es line 96 exists to prevent eager ValDef hoisting in the Scala compiler. Your test fixture must include the proposal box as `INPUTS(0)` (not `INPUTS(1)`) for withdrawals; the order matters for the contract's box-resolution.
- The recipient address in proposal R5 is a `Coll[Byte]` of the `blake2b256` of the recipient's ErgoTree, not the ErgoTree itself. Easy to construct wrong.

### 5.4.6 — Vote cancellation + edge paths

**Goal.** Cover the remaining spending paths the in-memory test ignores: voter-initiated cancellation (paths from `userVote.es:55-74`), the cooldown lockout, and any failure modes the success-path tests don't exercise (counting-phase outside its window, validation without quorum, etc.).

**Concrete steps.** Per-path tests modeled on the §5.4.4 / §5.4.5 patterns. Each test sets up a known fixture state, builds the corresponding tx, asserts either success or a specific contract-level rejection.

**What success looks like.** Each spending path identified in the trace (`vault.es`, `reserve.es`, `treasury.es` (3), `counting.es` (4), `proposal.es` (2), `userVote.es` (2), `timeValidator.es` (1)) has at least one positive and at least one negative test against the live node. Total ≈ 25–30 test functions.

---

## What NOT to touch

These exist; leave them alone unless you discover a real bug. If you do, surface it before fixing.

1. **The seven on-chain YoloDAO contracts** under `06-governance/contracts/`. They are audited and tested (63 + Python semantic tests pass on the in-memory side). They are inputs to Phase 5.4, not outputs of it.

2. **The Phase 5.3a `validate_supported` carve-out** for SigmaChain. Reasonable to revisit post-5.4 once chain identity is fully locked.

3. **The Phase 5.3b synced-tip predicate's bootstrap flag.** SigmaChain is now post-genesis, but the flag stays in place because the predicate has to handle the "fresh data dir" case for any dev wiping state.

4. **The Phase 5.3c synthesized genesis parent header** and the Phase 5.3d-i YOLO emission dispatch. Both are consensus-bearing now; any change requires re-validating block 1 + 2 against the pinned constants.

5. **The chain-spec pins** — `state_digest` (Phase 5.2), `header_id` (cleanup commit), `block_version` (5.3d-i). Touching any of these means a new chain identity.

6. **The `Network::Mainnet` / `Network::Testnet` paths** — must remain byte-parity with upstream Ergo.

7. **The on-chain contracts under `01-emission-tests/`, `02-treasury-tests/`, `03-lp-fund-tests/`, `06-governance/contracts/`** — audited, tested, do not modify.

---

## Known patterns from Phase 5.3

Phase 5.3 surfaced a class of issue you should recognize on sight: **node code paths that assume a fully-bootstrapped chain and break on the fresh-genesis edge.** Five concrete examples we already burned context on, in case 5.4 surfaces a sixth:

| Symptom (where you'll see it) | Root cause | Fix shape |
|---|---|---|
| `EarlyIBD { needed_min: 10, observed: N }` while applying a tx-bearing block | `last_applied_chain_window_10()` requires depth ≥ 10 | Conditional fallback to a synthesized window |
| `best_full_block_id 000…0 not in HEADERS` from the candidate builder | Code assumed a parent block always exists | Synthesize an in-memory pseudo-parent at `parent_height == 0` |
| `UnexpectedEnd { pos: 209, needed: 33 }` re-parsing a fresh-mined header | `active_params.block_version` defaulted to 1 (Autolykos v1 wire shape) but the serializer always emits Autolykos v2 | Override `block_version` to `INTERPRETER_60_VERSION` at launch |
| `attempt to divide by zero` in `miners_reward_at_height` | Ergo `MonetarySettings` curve fields all zero on SigmaChain (YOLO uses a different curve) | Dispatch on `chain_spec.emission_curve` before computing reward |
| `sigmachain-testnet genesis header_id not embedded` at node boot | `validate_supported` required `Some(header_id)` on every network | SigmaChain carve-out |

The pattern: SigmaChain hits an edge case that mainnet/testnet never trigger (because they bootstrap differently). The fix is almost always a network-aware branch that preserves the Ergo path byte-for-byte and adds the SigmaChain case. **Don't change the Ergo path.**

For Phase 5.4, the analog is likely to be in the wallet path or the indexer: code that assumes a populated mempool, a populated indexer, or a non-zero token universe. Same playbook applies.

---

## Workflow conventions

These are the user's rules (from memory). Follow them exactly.

1. **Branch first, push after testing.** Work on a feature branch off `sigmachain-node-fork`. Suggested name: `phase-5.4-yolodao-live`. Do not push to `main` or to `sigmachain-node-fork` directly. Do not push until the user has tested locally and confirmed.
2. **No `Co-Authored-By` in commits.** Use plain commit messages. Use a HEREDOC for multi-line bodies.
3. **No emojis** in code, commits, or docs unless the user explicitly asks.
4. **Run the full test suite after every change.** `cargo test` from the node repo root at minimum. CSE-style optimizations silently break contracts; the user has been burned by this before.
5. **Never echo or log secrets.** No `curl -v` against authenticated endpoints. No printing of mnemonics, API keys, wallet passwords, or proving keys to terminal output.
6. **Diagnose from code, not from a running node.** If a test fails, find the root cause in source first. Don't iterate against a live testnet to make a failing test pass by chance.
7. **Ask before state-changing actions.** Booting a node, generating wallet keys, submitting transactions — confirm with the user before doing these for the first time. Approval persists for the scope granted, not beyond.
8. **Use Plan mode for non-trivial implementations.** If a sub-phase reveals more complexity than this doc accounts for (and 5.4.4 in particular is likely to), write a plan and get the user's signoff before coding.

---

## Definition of done (the whole of Phase 5.4)

The minimum bar to call Phase 5.4 complete:

- [ ] A new test crate (or test module) drives the YoloDAO contracts end-to-end against `cargo run -p ergo-node -- --config sigmachain-testnet.toml`.
- [ ] `live_node_single_tx_round_trip` (5.4.1) passes — proves the harness works.
- [ ] YoloDAO genesis tokens are deployed and the deployment artifact (token IDs + contract hex) is persisted under version control.
- [ ] Deposit and redeem (5.4.3) pass — the 1:1 peg invariant verified on chain.
- [ ] Initiation, counting, validation (5.4.4) pass — the counter advances through all 4 phases and a proposal advances qty 1 → 2.
- [ ] Treasury withdrawal and proposal execution (5.4.5) pass — proportional disbursement matches the contract's split-math, proposal state token is burned.
- [ ] Vote cancellation + at least one negative test per spending path (5.4.6).
- [ ] Test-mode height plumbing decided + implemented — either a contract-recompile path with shorter windows or a fast-forward miner flag. Either way: documented.
- [ ] Full `cargo test` workspace-wide passes. Tally ≥ 3,843 (Phase 5 baseline) + however many Phase 5.4 adds.
- [ ] No regressions on the Ergo mainnet/testnet paths.
- [ ] STATUS.html updated to reflect Phase 5.4 done + Phase 6 promoted to "Next."
- [ ] One commit per logical sub-phase on a feature branch, awaiting the user's test pass before any push.

---

## When to ask the user

- **Before booting the node for the first time** in a fresh data dir. State-changing.
- **Before generating wallet keys or showing a mnemonic.** The Phase 5.3b session has the option-A pattern (in-session display); reuse it if appropriate, or offer option-B (file-based) if that's safer for the session's context.
- **Before deploying the YoloDAO genesis tokens.** This is a one-shot operation that determines all contract addresses; getting it right matters.
- **If a contract or chain-spec change appears necessary.** They are pinned for a reason. Surface, don't patch.
- **If a test failure appears non-deterministic.** The user has had bad experiences with flaky tests; surface and discuss before adding retries or sleeps.
- **Before committing or pushing anything.**

---

## Before SigmaChain goes live (must-fix punch list)

These are bring-up shortcuts taken during Phase 5.4 that are correct for the
current operating mode (single CPU miner that always emits `votes=[0;3]`)
but would silently miscompute or reject blocks if any non-zero votes are
ever cast. All three live in [ergo-mining/src/candidate.rs](07-sigmachain-node/sigmachain-node/ergo-mining/src/candidate.rs)
at the epoch-boundary `compute_next_params` call:

1. **Vote tally is hardcoded `empty_votes: Vec<(i8, i32)>`.** A correct
   implementation scans the prior voting-epoch's headers (6,144 of them on
   SigmaChain testnet), reads each header's `votes: [u8; 3]` field, and
   accumulates a `(param_id, count)` tally. The current code treats every
   epoch as "no votes were cast," which is true only because our miner sets
   all vote bytes to zero. If anyone — a different miner, a wallet
   contributor — ever casts non-zero votes, the candidate would miscompute
   the next-epoch active params and the resulting block would be rejected by
   `exMatchParameters` (validation rule 409 in `ergo-validation::voting`).

2. **`fork_vote: bool` is hardcoded `false`.** Same shape — a real
   implementation reads the boundary block's own `votes[2]` (the soft-fork
   slot) to decide whether THIS block is voting to start / continue a
   soft-fork window. Hardcoded false means no soft-fork voting can ever
   succeed.

3. **`proposed_update: &ErgoValidationSettingsUpdate` is hardcoded
   empty.** This is what the boundary block PROPOSES for the next voting
   window's validation-settings update. We don't propose anything because
   the chain has no mechanism yet to surface proposals. If you ever want to
   propose a rule change, this needs operator plumbing.

The encoder, builder, validator wiring, and `compute_next_params` itself
are all complete and correct — only the three inputs above are stubbed.
The fix is purely in the candidate-builder caller, not in the consensus or
encoder paths. Estimate: ~1-2 hours including a header-scan oracle test
against captured headers.

**This must be fixed before any SigmaChain network is launched with
multiple independent miners.** Until then, single-miner-zero-votes is a
safe operating mode and Phase 5.4 testing requires nothing more.

---

## Closing context

Phase 5.3 burned ~6 hours of careful work cleaning up the bootstrap gates that the Phase 5 handoff didn't anticipate. The biggest lesson: when something doesn't work, the answer is usually "there's a network-aware branch missing" rather than "the design is wrong." Apply the same lens to Phase 5.4 — if a wallet or indexer path doesn't work, look for the SigmaChain edge case before assuming the API is broken.

Phase 5.4 done means: a SigmaChain testnet runs the YoloDAO governance system end-to-end. That's the gate to Phase 6 (docs + STATUS rollover) and then mainnet planning. The chain is real now; you're proving the on-chain contracts work on it.

Good luck. Read the memory directory. Diagnose from code. Don't push before the user tests. Don't burn tokens.
