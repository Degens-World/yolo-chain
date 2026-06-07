# Phase 5.4 — Progress Checkpoint

**Date:** 2026-06-07 (updated)  
**Branch:** `phase-5.4-yolodao-live` (4 commits + uncommitted Milestone 4 prep landed)  
**Status:** Milestones 1–3 complete; Milestone 4 wallet-bridge blocker resolved + counter genesis test written (uncommitted, awaiting live-node validation). Chain at h≈8,610.

This doc supersedes the "What's left" section of
[PHASE-5.4-HANDOFF.md](PHASE-5.4-HANDOFF.md) only; the rest of that
handoff (required reading, where things live, workflow conventions)
still applies verbatim.

---

## What's done (proven on the live SigmaChain)

| Milestone | Status | Tests |
|---|---|---|
| **5.4.1** smoke (single-tx round-trip) | ✅ | `live_node_single_tx_round_trip` |
| **5.4.2** genesis token bake | ✅ | `bake_yolodao_genesis_tokens` (15 tokens) |
| **5.4.2** contract recompile (prod windows) | ✅ | `compile_yolodao_contracts` (15 trees) |
| **5.4.2** contract recompile (test windows) | ✅ | `compile_yolodao_contracts_test_windows` |
| **5.4.3** vault deposit + redeem (pair 1) | ✅ | `setup_pair_1` + `deposit_then_redeem_round_trip_pair_1` |

**Chain-level proof landed alongside:**
- First real SigmaChain epoch boundary mined at h=6,144 (block carries the canonical Ergo extension: 11 interlinks + 8 numeric params + subblocks_per_block=30 + block_version=4 + empty proposed_update). See commit `9ee73de`.

**Workspace tests:** 3,850 passing, 0 failing across the sigmachain-node workspace. +7 new tests vs the Phase 5 baseline (encoder round-trips + builder shape + extension shape).

**Deployment artifacts** (committed under [06-governance/test-vectors/](../06-governance/test-vectors/)):
- `sigmachain-deployment.json` — 15 minted token ids + tx ids + owner address
- `sigmachain-deployment-trees.json` — 15 compiled trees with production block-count windows
- `sigmachain-deployment-trees-test.json` — same 15 trees with 5-block voting / counting / validation windows (for Milestone 4)
- `sigmachain-pair-1-state.json` — initial vault + reserve box ids for pair 1

---

## What's left

### Milestone 5.4.4 — voting lifecycle (NEXT)

**Goal:** Walk the counter NFT through phases 1→3 of `counting.es` and advance a proposal box from qty 1 to qty 2 via real on-chain validation.

**Tests called out by the original handoff (in priority order):**
1. `proposal_initiation_advances_counter` — single happy-path tx that creates a proposal box at qty 1 and moves the counter into the voting phase
2. `vote_counting_burns_vote_nfts_and_accumulates_tallies` — voter boxes consumed, vote NFTs burned in OUTPUTS, R7/R9 updated
3. `validation_passes_proposal_to_qty_2_when_thresholds_met`
4. `validation_keeps_proposal_at_qty_1_when_quorum_fails`
5. `validation_uses_elevated_threshold_for_high_proportion_proposals`

**Infrastructure pieces:**

#### A. `PaymentRequestDto`-with-registers patch — **LANDED (uncommitted)**

Path 1 from the prior plan. `PaymentRequestDto` now carries an optional
`additionalRegisters: Option<BTreeMap<String, String>>` field; keys are
`"R4"`..`"R9"`, values are hex-encoded per-register payload bytes
(`ValueSerializer` shape — same wire format the on-chain decoder reads).
Wired through both `build_unsigned_tx` paths (override-inputs +
auto-selection), with strict dense-packing enforcement and 10 unit
tests covering the round-trips + every error path.

Touched files:
- `07-sigmachain-node/sigmachain-node/ergo-api/src/wallet/sending.rs` — new optional field on `PaymentRequestDto`.
- `07-sigmachain-node/sigmachain-node/ergo-wallet/src/tx_builder.rs` — `PaymentRequest` carries `AdditionalRegisters`, builder threads it into the payment output candidate.
- `07-sigmachain-node/sigmachain-node/ergo-node/src/node/wallet_bridge.rs` — new `decode_additional_registers` helper; both build paths use the per-request registers instead of hardcoding empty.
- `06-governance/node-tests/src/{client.rs,registers.rs,lib.rs}` — client DTO mirrors the wire field; `registers.rs` exposes `slong_hex`, `slong_pair_hex`, `coll_byte_hex` backed by the node's own `ergo-ser` writers (path-dep).
- Existing call sites updated mechanically: `wallet_admin_roundtrip`, `wallet_send_e2e`, `tx_builder_oracle`, `smoke`, `bake_genesis`, `setup_pair_1`, `deposit_redeem_pair_1`.

Test status: workspace `cargo test` from `07-sigmachain-node/sigmachain-node`
reports **3,860 passing, 0 failing** (+10 register-decode tests vs the
3,850 baseline). `06-governance/node-tests` lib tests: **4 passing**
(register-helper round-trips). The live-feature node-tests build
clean under `cargo check --tests --features live`.

#### B. Counter genesis box setup — **WRITTEN (uncommitted, not yet run against the node)**

`06-governance/node-tests/tests/setup_dao_genesis.rs`:

- script: counting.es test-windows variant (from `sigmachain-deployment-trees-test.json`)
- value: `1_000_000`
- tokens: `[(COUNTER_NFT, 1)]`
- R4 = `Long(1_000_000_000)` — vote deadline; far past any plausible test height
- R5 = `(Long, Long)` tuple `(0L, 0L)`
- R6 = `Coll[Byte]` of 32 zero bytes
- R7 = R8 = R9 = `Long(0)`

Sends one tx via `/wallet/transaction/send`, waits for inclusion,
cross-checks every register hex against what the indexer returned for
the new box (catches any silent re-encoding drift), persists
`06-governance/test-vectors/sigmachain-dao-state.json`.

Pre-flight refuses to run if `COUNTER_NFT` already sits in an unspent
box (would burn the singleton on the next op).

#### C. Per-voter `timeValidator` box bootstrap

Each voter needs a `timeValidator.es`-guarded box holding their vYOLO stake + a `ValidVote NFT`. Now mechanical given (A) + the helpers in `registers.rs`.

**Then the lifecycle test** drives the counter through:
1. **Initiation** (Phase 1) — proposer tx: inputs = (counter box, vYOLO stake box, change box); outputs = (counter at phase 2 with `R4 = HEIGHT + votingWindow`, locked-stake box, proposal box at qty 1, change). vYOLO stake ≥ `initiationHurdle = 100_000 vYOLO`.
2. **Vote casting** — for 2–3 voters: tx with input = (timeValidator box) → output = (userVote box that the counter will consume during counting).
3. **Counting** — mine ~5 blocks (test windows) to enter `[voteDeadline, voteDeadline + countingPhase)`. Tx with inputs = (counter phase 2, all voter boxes); outputs = (counter with R7/R9 updated, voter vYOLO returned to each voter, vote NFTs burned — assert `/utxo/byId/{vote_nft}` returns 404).
4. **Validation** — mine ~5 more blocks. Tx with inputs = (counter phase 2, proposal qty 1); outputs = (counter phase 3 with tallies reset, proposal qty 2). Contract checks `validationVotes * 10_000 >= totalVotes * supportBps` (counting.es:187-193).

**Estimated effort:** ~3-5 hours of focused work after the `PaymentRequestDto`-with-registers patch lands. Each tx is non-trivial register-aware construction.

### Milestone 5.4.5 — treasury withdrawal + proposal execution

Tests: `treasury_withdrawal_against_passed_proposal_disburses_proportionally`, `proposal_execution_burns_state_token`, `treasury_migration_at_full_proportion_transfers_nft`.

Depends on: 5.4.4 leaves the chain with a `qty=2` proposal box and a populated treasury box. Treasury needs to be created via the same register-bearing-payment path as the counter.

Watch out for the split-math (`treasury.es:107-109`): fixtures must compute expected disbursement as `wholePart + remainderPart`, NOT the naive `value * proportion / Denom`. They diverge when `value > 10_000_000` and rounding kicks in.

### Milestone 5.4.6 — vote cancellation + negative paths

~10 tests, mostly mechanical given the 5.4.4 + 5.4.5 patterns. Each test sets up a known fixture state, builds a tx, asserts either a specific contract-level rejection or success.

### Vote-tally must-fix (before SigmaChain goes live)

Separate from Phase 5.4 testing but documented in [PHASE-5.4-HANDOFF.md](PHASE-5.4-HANDOFF.md) under "Before SigmaChain goes live". The candidate builder at the epoch boundary currently hardcodes `epoch_votes = []`, `fork_vote = false`, `proposed_update = empty`. Correct for single-miner-zero-votes; would silently miscompute if any miner ever casts non-zero votes. ~1-2 hours of focused work; not blocking Phase 5.4 tests.

---

## Gotchas discovered (so next session doesn't re-burn them)

1. **`ergo-lib::UnsignedTransaction::bytes_to_sign` is NOT the wallet's unsigned wire format.** It produces signed-tx-with-empty-proofs bytes. The wallet's `/wallet/transaction/sign` expects bytes from `ergo-ser::write_unsigned_transaction` — same shape minus the per-input empty-proof-length byte. If you build custom txs, either use the sigmachain-node's `ergo-ser` directly (path dep) or write a thin serializer in the test crate.

2. **The default box selector burns through ALL mining-reward boxes before finding small token-bearing ones.** Already fixed (see commit `e95a8de`), but worth knowing: the wallet's UTXO has ~7k+ small boxes by the time the chain is past the maturity gate, and any greedy-by-ERG-desc strategy hits the tx init-cost cap before a small token-bearing box gets selected.

3. **`/wallet/boxes/unspent` is paginated by box-id ascending.** Freshly-minted output boxes can sit on a page far beyond the default `limit=200`. The deposit/redeem test sidesteps this by recording the recipient box id from the deposit's outputs and feeding it directly to the redeem path. For tests that need to look up by NFT, prefer `/blockchain/box/unspent/byTokenId/{token}`.

4. **The Ergo mainnet node at `127.0.0.1:9053` (instance "QualityControl") is the contract compiler.** It accepts `/script/p2sAddress` without an api_key. The SigmaChain node at `127.0.0.1:9054` uses api_key `hello` (placeholder, matches the toml hash). Don't conflate them.

5. **Wallet self-verify init-cost cap is `max_block_cost * 10 = 10_000_000` JIT units.** Bundling vault + reserve creation into one tx blew this cap because of how many token-bearing inputs the selector ended up touching. Splitting into two back-to-back txs landed under the cap. If you hit this, split.

6. **`/wallet/transaction/sign` rejects non-bare-P2PK inputs unless the prover gate is patched.** Already fixed (commit `e95a8de`), but if you ever see "input N has an unsupported script family," that gate has returned.

7. **State-drift recovery.** After a deposit consumes the original vault box, the pair-state file on disk is stale. The deposit/redeem test handles this via `reconcile_pair_with_chain` — it queries `/blockchain/box/unspent/byTokenId/{state_nft}` (singleton NFT = one current holder). Mirror this pattern for any contract-box-tracking test.

8. **`ChainStateAccessorImpl::tip_height` used to return a boot-time cached snapshot, not the live tip.** Fixed in the 2026-06-07 patch alongside the `additionalRegisters` work — `tip_height()` now reads `reader.committed_tip()` live and falls back to the boot value only when no committed snapshot exists. The old behaviour silently broke rule 124 (`txMonotonicHeight`) for any tx the wallet built against a box scanned after boot: `chain.tip_height()` would return the boot height, but the input box's `creation_height` could be hundreds of blocks higher, and the resulting output's `creation_height` would be below max input height. Surfaced by the first `setup_dao_genesis` attempt against a chain that had advanced from h=12348 (boot) to h=14651: `OutputCreationHeightBelowInputs { creation_height: 12348, max_input_height: 13661 }`. If you ever see this rejection again, the live read regressed.

---

## Operator setup (re-running from a fresh shell)

`06-governance/node-tests/SETUP.md` is authoritative. TL;DR:

```sh
# Terminal 1 — node
cd 07-sigmachain-node/sigmachain-node
target/release/sigmachain-node --config ergo-node/sigmachain-testnet.toml

# Terminal 2 — unlock wallet (one-shot)
curl -s -X POST http://127.0.0.1:9054/wallet/unlock \
  -H 'api_key: hello' -H 'Content-Type: application/json' \
  --data @- <<'JSON'
{ "pass": "yolo-testnet-dev-only" }
JSON

# Terminal 3 — miner (keep running for the duration of tests)
target/release/examples/sigmachain_cpu_miner \
  --node http://127.0.0.1:9054 --api-key hello --max-blocks 5000

# Terminal 4 — tests
cd 06-governance/node-tests
export YOLO_NODE_API_KEY=hello
export YOLO_WALLET_PASSWORD=yolo-testnet-dev-only
export YOLO_TX_TIMEOUT_MS=60000
cargo test --features live -- --nocapture --test-threads=1
```

Tests assume:
- The Ergo mainnet node at `127.0.0.1:9053` is up (for contract recompilation).
- The miner is producing blocks for the duration of any test that submits a tx.
- The wallet is unlocked.

---

## Upstream (`arkadianet/ergo`) drift

Our fork point is `1a68cd9b` (`v0.3.0 + 4 commits`, per [UPSTREAM.md](../07-sigmachain-node/UPSTREAM.md)). Upstream has since merged **PRs #7 through #15** (HEAD `08ee11e` as of 2026-06-07). The ones most worth pulling in next sync pass, ranked by relevance to Phase 5.4 work:

- **#13** (`1e139b4`) `ergo-ser: drop v6 method-call tree-version gate` — **functionally already present in our fork** (types.rs:556 has the `_tree_version: u8` unused param, gate body removed). Doc comments differ, semantics match.
- **#14** (`2e173cf`) `#13 follow-up: Scala oracle vectors + embedded-surface coverage` — NOT yet in our fork. Adds golden parse vectors under `test-vectors/scala/sigma/v6_methodcall_typeargs_v0_header/`, plus `decode_mode_routing` / `ergotrees_roundtrip` / evaluator-side coverage. Cleans up the `_tree_version` parameter (removes it entirely from both the function signature and the two `parse.rs` call sites). Consensus-bar evidence the upstream maintainer asked for; we should pull it.
- **#7** (`8d3289a`) `Mining: minimal-first candidate publish kills the post-block 503 window` — directly relevant to the `/mining/candidate` 503s observed during fast mining; would quiet the operator UI log.
- **#8** (`2585eb0`) `fix(ergo-node): survive digest-mode handshake, sync, and API seams` — not exercised by our single-node testnet but worth pulling for parity.
- **#9** (`9e9f3dd`) `Mining: per-tip pristine AVL base cache collapses the 20-second candidate dry-run` — perf, not correctness; lower priority.
- **#11** (`f16f308`) `surface node mode identity on the overview page + wire real mining flag` — touches the same dashboard files we rebranded in commit `c1bbcd4`; merge needs a 3-way reconcile.
- **#12** (`9444640`) `Mining: single-step incremental advance of the dry-run base` — perf.
- **#15** (`08ee11e`) `Scala-compat /emission/at + fix auth-layer capture` — API-surface addition.

Recommended sync pass (separate task from Phase 5.4):
1. Branch `upstream-sync-pr7-15` off `phase-5.4-yolodao-live` post-merge.
2. Apply #14 first (test coverage for our existing #13-equivalent fix).
3. Apply #7 (the 503 quieting maps cleanly onto the symptom we just documented).
4. Apply #8, #9, #11, #12, #15 in order, reconciling the dashboard / mining-handle conflicts each time. SigmaChain-specific branches that landed in Phase 5 may rebase under each merge.
5. Re-run the full workspace + the Phase 5.4 live suite.

---

## Recommended next-session opener

1. Boot node + unlock wallet + start miner (above).
2. Run the existing live suite to sanity-check (`cargo test --features live` from `06-governance/node-tests/`). Everything in 5.4.1–5.4.3 should still pass.
3. Run `setup_dao_genesis_creates_counter_box_at_phase_zero` (the new test). Expected: tx confirms in one block, the indexer reports R4-R9 matching the hex the test sent, `sigmachain-dao-state.json` lands under test-vectors. Failure modes worth watching for:
   - tx rejected by the wallet self-verifier — likely a register-encoding shape mismatch; cross-check the per-register hex in the failing assertion against `ergo-ser/src/register.rs` test fixtures.
   - tx accepted but `additionalRegisters` empty in the indexer view — the wire field rename or the wallet's output-construction path skipped the field; verify [wallet_bridge.rs:1118-1129](07-sigmachain-node/sigmachain-node/ergo-node/src/node/wallet_bridge.rs#L1118-L1129) and [tx_builder.rs:88-96](07-sigmachain-node/sigmachain-node/ergo-wallet/src/tx_builder.rs#L88-L96) use `req.additional_registers.clone()`.
4. Commit the wallet patch + counter setup work on `phase-5.4-yolodao-live` once the operator confirms it works. Per the user's "no push before user tests" rule, leave the commit step to the operator.
5. Tackle the 5.4.4 voting-lifecycle tests in the order called out above (initiation → counting → validation), starting from the counter box id in `sigmachain-dao-state.json`.

The PR description for `phase-5.4-yolodao-live` should bundle commits 1–4 (the epoch fix, wallet patches, supporting changes, and live tests) as a single Phase 5.4.1–5.4.3 deliverable. Milestone 4+ work (this patch + the genesis setup test + subsequent 5.4.4 lifecycle tests) goes onto the same branch as additional commits, or a new branch off the merge — operator's call.
