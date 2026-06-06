# Phase 5.4 — Progress Checkpoint

**Date:** 2026-06-06  
**Branch:** `phase-5.4-yolodao-live` (4 commits ahead of `phase-5.4-handoff-doc`)  
**Status:** Milestones 1–3 complete + Milestone 4 prep landed. Chain at h≈8,610.

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

**Two infrastructure pieces still needed before any of the above can land:**

#### A. Counter + initial-proposal genesis setup
The counter box must exist on chain at "Phase 0 / no active proposal" state:
- value: `1_000_000` (storage rent floor)
- tokens: `[(COUNTER_NFT, 1)]`
- script: `counting.es` compiled (test-windows variant from `deployment-trees-test.json`)
- registers:
  - R4 (vote deadline): a height comfortably in the future so `isBeforeCounting` is true
  - R5 (proportion, votes_for): `(0L, 0L)`
  - R6 (recipient hash): `Coll[Byte]` of 32 zero bytes
  - R7 (total votes): `0L`
  - R8 (initiation stake): `0L`
  - R9 (validation votes): `0L`

**The blocker:** the wallet's `PaymentRequestDto` doesn't carry registers. Two paths:
1. **Extend `PaymentRequestDto`** with an optional `additionalRegisters: Option<Map<String, String>>` field, decode in `wallet_bridge::build_unsigned_tx`. Mirrors what Scala's wallet supports. ~30 lines.
2. **Custom tx construction** in test code — build the `UnsignedTransaction` manually with `ergo-ser` types (NOT `ergo-lib`; the wire formats differ on input encoding — see "Gotchas" below).

Path 1 is the cleanest and unblocks every subsequent contract test (treasury, counter, proposal, userVote, timeValidator all need register-bearing boxes). Recommend doing this BEFORE writing any 5.4.4+ test.

#### B. Per-voter `timeValidator` box bootstrap
Each voter needs a `timeValidator.es`-guarded box holding their vYOLO stake + a `ValidVote NFT`. Same `PaymentRequestDto`-with-registers blocker; once A lands, this is mechanical.

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

## Recommended next-session opener

1. Boot node + unlock wallet + start miner (above).
2. Run the existing live suite to sanity-check (`cargo test --features live` from `06-governance/node-tests/`). Everything in 5.4.1–5.4.3 should still pass.
3. Land `PaymentRequestDto`-with-registers patch in `wallet_bridge` (the one infrastructure blocker for 5.4.4). Mirror Scala's `additionalRegisters` field.
4. Write the counter + initial-proposal genesis setup test (`tests/setup_dao_genesis.rs`). Output the counter box id to `sigmachain-dao-state.json`.
5. Tackle 5.4.4 tests in the order called out above.

The PR description for `phase-5.4-yolodao-live` should bundle commits 1–4 (the epoch fix, wallet patches, supporting changes, and live tests) as a single Phase 5.4.1–5.4.3 deliverable. Milestone 4+ work goes onto the same branch as additional commits or a new branch off the merge — operator's call.
