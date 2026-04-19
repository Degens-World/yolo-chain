# Emission Contract Test Pipeline — Claude Code Handoff 02

**From:** Handoff 01 close-out (claude.ai session, 2026-04-19)
**To:** Claude Code, running locally with `cargo`/`sbt` access
**Goal:** Land on-chain ErgoTree evaluation tests for `emission.es` v1.1. The semantic logic is already proven by 62 Python tests; this handoff is purely about compiled-ErgoTree confirmation.

---

## Current state

### What works
- `emission.es` v1.1 (Ergo-parity pass applied: strict `==` on treasury/LP, `heightIncreased`, `heightCorrect`)
- 62 Python tests passing in `contract_sim.py` + `test_emission_exhaustive.py` (semantic validation complete)
- Rust project at `~/working-files/yolo-chain/emission-tests/` builds cleanly under rustc 1.85+, ergo-lib 0.28, with the "compiler" + "arbitrary" features
- `Context` construction via `force_any_val::<PreHeader>()` and `force_any_val::<[Header; 10]>()` works (the arbitrary feature is enabled in Cargo.toml)
- `Prover::prove` / `Verifier::verify` round-trip wired correctly (0.28 uses 4-arg signatures, no `Env`)

### Hard blocker identified
**sigma-rust's `ergoscript-compiler` cannot compile `emission.es` v1.1.** It's a limited subset of the ErgoScript language. The specific blocker is the nested typed-lambda in the terminal path:

```ergoscript
val nftBurned: Boolean = OUTPUTS.forall { (o: Box) =>
  o.tokens.forall { (t: (Coll[Byte], Long)) => t._1 != emissionNftId }
}
```

Evidence: compile returns `ParseError { expected: [ValKw, IntNumber, LongNumber, Ident, Minus, LParen], found: Some(RBrace), span: 2891..2892 }`. Position 2891 is the *final* closing brace of the script, meaning the parser bailed early (at the nested lambda) and reports EOF as "unexpected `}`".

Confirmed by the sigma whitepaper: "implementations should avoid the use of lazy-evaluation constructs, such as 'forall' and 'exists'." sigma-rust's Rust compiler has less coverage than Scala's sigmastate-interpreter.

**This is not a bug I can fix in test scaffolding — it's a capability gap in sigma-rust's compiler.**

---

## Two paths forward (decision point)

### Path A — Scala AppKit harness (recommended)

Build a Scala test harness following the `scala-test-harness` skill template. This is what Ergo itself uses for `emissionBoxProp`. The sigmastate-interpreter Scala compiler fully supports nested lambdas and typed-lambda syntax, so `emission.es` v1.1 compiles as-is.

**Why this is the right call:**
- Runs against a real Ergo node (local or testnet) — hits full consensus validation that sigma-rust tests would miss anyway (MIN_BOX_VALUE, block-value constraints, etc.)
- Uses an already-working toolchain: ergo-appkit 5.0.4, Scala 2.12.18, sbt
- The `scala-test-harness` skill in the EKB MCP documents every gotcha (ErgoTree size bit, HEIGHT off-by-one, scrypto SNAPSHOT fix, fixOutBoxR4 reflection helper for GroupElement registers if needed)
- Same pattern audited at 9/10 for NftRentalV3
- Expected effort: 2-4 hours total including node connectivity

**Reference files CQ has:**
- `~/working-files/nft-rental-test/` (NftRentalV3 harness) — copy the project layout
- `~/working-files/.secrets` — API keys, mnemonic
- Local node: `192.168.110.16:9053`

**Desired phases:**
```
phase0          # compile emission.es, print ErgoTree size
phase1          # deploy mock emission box (creation_height=0, arbitrary nanoERG value, fake NFT)
phase2 <boxId>  # normal spend at h=1 → expect accept
phase3 <boxId>  # spend at halving boundary → expect accept
phase4 <boxId>  # underpay treasury → expect reject
phase5 <boxId>  # overpay treasury → expect reject (v1.1 strict ==)
phase6 <boxId>  # same-block spend → expect heightIncreased fail
phase7 <boxId>  # wrong successor creation_height → expect heightCorrect fail
phase8 <boxId>  # drain to terminal, spend via terminal path → expect accept
phase9 <boxId>  # terminal with NFT not burned → expect reject
```

For cost efficiency, phases 1-9 can use low-value (~0.01 ERG) mock emission boxes — the contract logic validates identically regardless of value.

### Path B — Ship as-is with documented Rust blocker

Python 62-test suite is the authoritative validation. The Rust pipeline is blocked by compiler limitations, not contract bugs. Update the handoff package to note this and move on.

**When to pick B:** if mainnet deployment isn't imminent and Claude Code can revisit compiled-tree validation later when sigma-rust's compiler improves (or after pivoting to a real Ergo node test).

---

## Working Rust setup (for reference, even if pivoting)

The Rust project is at `~/working-files/yolo-chain/emission-tests/`. The build pipeline works — only contract compilation fails.

**Cargo.toml:**
```toml
[package]
name = "emission_tests"
version = "0.1.0"
edition = "2021"
rust-version = "1.85"

[dependencies]
ergo-lib = { version = "0.28", features = ["compiler", "arbitrary"] }
blake2 = "0.10"

[dev-dependencies]
sigma-test-util = { git = "https://github.com/ergoplatform/sigma-rust", tag = "ergo-lib-v0.28.0" }
```

**Key API learnings from 0.28 source (`~/.cargo/registry/src/index.crates.io-*/ergotree-interpreter-0.28.0/src/eval/context.rs`):**
- `compile(src, ScriptEnv::default())` returns `ErgoTree` directly
- `Context` fields: `height, self_box, outputs, data_inputs, inputs, pre_header, headers, extension`
- `inputs: TxIoVec<&'ctx ErgoBox>` — use `[&ErgoBox; 1].into()`
- `Prover::prove(tree, ctx, msg, hints)` — 4 args, no Env
- `Verifier::verify(tree, ctx, proof, msg)` — 4 args, no Env
- Use `force_any_val::<PreHeader>()` and `force_any_val::<[Header; 10]>()` (contract never reads them)

Passing 2/4 tests: `model_tests::genesis_value_matches_spec`, `model_tests::reward_at_epoch_boundaries`. The other 2 fail on `compile()` with the ParseError above.

---

## Contract files (v1.1 canonical)

All under `~/working-files/yolo-chain/emission-tests/`:
- `emission.es` — contract source v1.1
- `contract_sim.py` — Python mirror of the contract (source of truth for semantic logic)
- `test_emission_exhaustive.py` — 62 Python tests, all passing
- `emission_model.py` — reference reward/split math oracle
- `ERGO_COMPARISON.md` — line-by-line cross-ref vs Ergo's `emissionBoxProp`
- `EDGE_CASES.md` — 14 handled edge cases + 5 design decisions
- `tests/emission_test.rs` — Rust harness (builds but contract compile fails)

## v1.1 contract changes from v1.0 (for context)

1. Treasury/LP value checks: `>=` → `==` (matches Ergo's EQ convention)
2. Added `heightIncreased`: `HEIGHT > SELF.creationInfo._1`
3. Added `heightCorrect`: `nextBox.creationInfo._1 == HEIGHT`

Deliberate difference from Ergo: in-contract miner output enforcement is delegated to consensus layer (not belt-and-suspenders like Ergo's `correctMinerOutput`).

---

## Recommended next action for Claude Code

1. **Ask CQ**: path A (Scala harness) or path B (ship as-is)?
2. **If path A**: call the `scala-test-harness` skill via `EKB MCP:get_skill` to get the full Scala pattern reference. Then call `ergoscript-appkit-mainnet` skill. Copy NftRentalV3 project layout, write `EmissionTest.scala` with phase0-phase9, test against local node at 192.168.110.16:9053. Budget: 2-4 hours. Deliverable: green run log of all phases.
3. **If path B**: update `EDGE_CASES.md` and `README.md` in the handoff package to note "sigma-rust compilation blocked on nested-lambda limitation (see terminal path nftBurned check); recommended compile-confirmation path is Scala AppKit against sigmastate-interpreter." Commit and close out.

## Anti-patterns to avoid (lessons from handoff 01 session)

- **Don't strip the contract's syntax** to make sigma-rust compile it — you'd be testing a different contract than what deploys. This was considered and rejected.
- **Don't add governance/voting surface** for treasury/LP removal — CQ explicitly decided against this. v1.1 ships as-is.
- **Don't re-open the Ergo-parity review** — already completed, 3 changes applied, documented in ERGO_COMPARISON.md.
- **Don't rewrite Python tests** — 62/62 green, they're the authoritative semantic validation.

## One-line context for a fresh Claude Code session

"Emission contract v1.1 for YoloChain (Ergo-derived L1) needs on-chain ErgoTree eval confirmation. 62 Python tests pass. Rust path blocked on sigma-rust compiler limitation (can't parse nested typed-lambda in terminal path `nftBurned` check). Pivot to Scala AppKit harness following `scala-test-harness` skill — test against local node 192.168.110.16:9053, phases 0-9 matching the Python test categories."
