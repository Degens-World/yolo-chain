# SigmaChain Parameter Inventory

**Source:** arkadianet/ergo HEAD `1a68cd9` (4 commits past `v0.3.0` tag — small CI + p2p fixes)
**Cloned to:** `07-sigmachain-node/arkadianet-ergo-reference/`
**Build:** clean on Rust 1.95.0 (`cargo build --release -p ergo-node -p ergo-wallet`, exit 0)
**Date:** 2026-06-04

This is the Phase 1 deliverable from `overall-documents/RUST-NODE-FORK-HANDOFF.md`. Every hardcoded consensus parameter we'll need to change is listed below with its current Ergo value, target YOLO/SigmaChain value, and exact file:line.

## Key architectural finding

`ergo-chain-spec/src/lib.rs` is the **single source of truth** for nearly every network-level parameter (its own doc-comment says so: *"Single source of truth for 'what does network X look like'"*). It exposes a `Network::Mainnet | Testnet` enum and per-network constructors on each narrow params struct, all aggregated by `ChainSpec::for_network`.

This means the SigmaChain fork is structurally tiny: add a `Network::SigmaChainTestnet` variant + `sigmachain_testnet()` constructors on each params struct, and almost every consumer downstream picks it up via the existing `for_network` dispatch.

The parameters that live **outside** `ergo-chain-spec` are:
- **Storage rent fee logic** — `ergo-validation/src/storage_rent.rs` (arithmetic) + `ergo-validation/src/tx/script.rs::check_storage_rent` (consensus rule)
- **Miner-side storage rent self-claim** — `ergo-mining/src/storage_rent_claim.rs` (already exists as opt-in!)
- **Default REST API port** — `ergo-node/ergo-node.toml` (config, not consensus)

## The novel YOLO consensus change

arkadianet **already implements** an opt-in miner storage-rent self-claim (`ergo-mining/src/storage_rent_claim.rs`). The YOLO-unique rule is to make this the **only** legal path by rejecting non-coinbase rent claims in `check_storage_rent`. That's a single localized consensus rule addition — much less work than the original fork checklist implied.

## Parameter table

Legend:
- **CHAIN-SPEC** = lives in `ergo-chain-spec/src/lib.rs` (this is most of them)
- **VALIDATION** = lives in `ergo-validation/`
- **CONFIG** = lives in `ergo-node/ergo-node.toml` (operator-set, not consensus)
- 🆕 = new SigmaChain-only consensus rule
- ❓ = needs decision before implementing

### Network identity (`NetworkParams`)

| Parameter | Location | Ergo mainnet | Ergo testnet | YOLO testnet target |
|---|---|---|---|---|
| Network magic bytes | CHAIN-SPEC L101-114 | `[1, 0, 2, 4]` | `[2, 3, 2, 3]` | `[ 'Y', 'O', 'L', 'O' ]` = `[0x59, 0x4F, 0x4C, 0x4F]` ❓ |
| Address prefix byte | CHAIN-SPEC L101-114 | `0x00` (Mainnet) | `0x10` (Testnet) | ❓ new variant needed in `ergo-ser::address::NetworkPrefix` |
| P2P port (seed peers) | CHAIN-SPEC L602-616, L637-641 | 9030 | 9023 | `9120` ❓ |
| Default REST API port | CONFIG ergo-node.toml:25 | 9099 (arkadianet default; Scala uses 9053) | same | `9153` ❓ |

### Block timing (`DifficultyParams`, `BlockTimingParams`)

| Parameter | Location | Ergo mainnet | Ergo testnet | YOLO target |
|---|---|---|---|---|
| Target block interval (ms) | CHAIN-SPEC L186, L203, L475, L488 | 120_000 | 45_000 | **20_000** (per emission.es) |
| Pre-EIP-37 epoch length (blocks) | CHAIN-SPEC L178, L198 | 1024 | 128 | **6144** (= 1024 × 120/20, preserves wall-clock) |
| EIP-37 epoch length | CHAIN-SPEC L179 | `Some(128)` | `None` | `None` (no EIP-37 on YOLO) |
| EIP-37 activation height | CHAIN-SPEC L180 | `Some(844_673)` | `None` | `None` |
| V1→V2 hard fork descriptor | CHAIN-SPEC L181-184 | `Some(@417_792)` | `None` | `None` (start at v2-or-later from genesis, like testnet) |
| Initial difficulty (genesis) | CHAIN-SPEC L185, L202 | `0x011765000000` | `0x01` | `0x01` (low for testnet bootstrap) |
| `header_chain_diff` (sync tolerance) | CHAIN-SPEC L476, L489 | 100 | 800 | **600** (~600 × 20s = 200 min staleness) ❓ |

### Voting (`VotingParams`)

| Parameter | Location | Ergo mainnet | Ergo testnet | YOLO target |
|---|---|---|---|---|
| Voting epoch length (blocks) | CHAIN-SPEC L246, L264 | 1024 | 128 | **6144** (match difficulty epoch) |
| Soft-fork epochs | CHAIN-SPEC L247, L265 | 32 | 32 | 32 (keep) |
| Activation epochs | CHAIN-SPEC L248, L266 | 32 | 32 | 32 (keep) |
| V2 activation height (voting state machine) | CHAIN-SPEC L249, L267 | `Some(417_792)` | `None` | `None` |

### Monetary / Emission (`MonetaryParams` — **structural fork required**)

**Reconciliation complete** (Phase 1.5). The YOLO `emission.es` uses a **geometric halving curve with a tail floor**, which cannot be expressed in Ergo's linear-reduction `MonetaryParams { fixed_rate, fixed_rate_period, epoch_length, one_epoch_reduction }` schema.

YOLO emission per [emission.es](../01-emission-tests/emission.es):
- Initial reward: **50 × 10⁹ nanoYOLO** (50 coins)
- Halving every **1_577_880 blocks** (~1yr @ 20s)
- Halving sequence: 50 → 25 → 12.5 → 6.25 → 3.125 → 1.5625 → **1.0 forever** (tail emission floor)
- Per-block split (consensus-enforced via emission.es OUTPUTS(0..2) + coinbase OUTPUTS(3)):
  - 10% treasury (output 1)
  - 5% LP fund (output 2)
  - 85% miner (output 3 — paid via coinbase layer, not contract)

Ergo's emission curve (for contrast):
- 75 ERG flat for 525_600 blocks, then linear reduction of 3 ERG every 64_800 blocks until 0
- Per-block split: 90% miner, 10% founders, no LP
- 2 coinbase outputs: new emission box + miner box

**Fork implementation (scope of change):**

1. **`ergo-chain-spec/src/lib.rs`** — Add a new `EmissionCurve` enum or replace `MonetaryParams` with a discriminated variant:
   ```rust
   pub enum EmissionCurve {
       ErgoLinear(MonetaryParams),
       YoloGeometricHalving {
           initial_reward: u64,        // 50_000_000_000
           blocks_per_halving: u32,    // 1_577_880
           min_reward: u64,            // 1_000_000_000 (tail floor)
           max_halvings: u32,          // 6 (after which tail kicks in)
           treasury_share_percent: u8, // 10
           lp_share_percent: u8,       // 5
           miner_reward_delay: u32,    // 4320 (1 day at 20s)
       },
   }
   ```

2. **`ergo-mining/src/emission_rules.rs`** (181 lines today) — Add YOLO branches to `emission_at_height` and `miners_reward_at_height`; add new `treasury_reward_at_height` and `lp_reward_at_height` functions. Keep the Ergo functions verbatim for parity tests.

3. **`ergo-mining/src/coinbase.rs::build_pre_eip27_emission_tx`** (L67-140) — Currently builds 2 outputs (new emission box, miner box). For YOLO needs 4 outputs:
   - Output 0: new emission box, value = `input.value - blockReward(h)` (emission.es L89)
   - Output 1: treasury, value = `blockReward * 10 / 100`, lock = `treasury_accumulation.es` script (emission.es L101-104)
   - Output 2: LP, value = `blockReward * 5 / 100`, lock = `lp_accumulation.es` script (emission.es L107-110)
   - Output 3: miner reward, value = `blockReward - treasury - lp` (≈85%), lock = `reward_output_script(miner_pk)`

4. **Genesis emission box registers** — must have:
   - `tokens[0]` = emission NFT (amount 1)
   - `R4` = blake2b256(treasury_accumulation script)
   - `R5` = blake2b256(lp_accumulation script)
   - Value: 177_412_882_500_000_000 nanoYOLO (177,412,882.5 coins)

**The `miner_reward_delay` decision is now confirmed:** bump to **4320** (1 day at 20s).

**Note:** the YOLO contract enforces `nextBox.creationInfo._1 == HEIGHT` (Gap B at emission.es L95) and `HEIGHT > SELF.creationInfo._1` (Gap A at L48). Ergo coinbase builder already sets `creation_height = next_height` on the new emission box (coinbase.rs L91) — already compatible.

### Reemission / EIP-27 (`ReemissionParams`)

| Parameter | Location | Ergo mainnet | YOLO target |
|---|---|---|---|
| Reemission section | CHAIN-SPEC L368-382, ChainSpec.reemission | `Some(ReemissionParams { … })` | **`None`** (YOLO has no EIP-27) |

Testnet already sets this to `None` (L579), so the pattern is established.

### Genesis (`GenesisParams`)

| Parameter | Location | Ergo mainnet | YOLO target |
|---|---|---|---|
| State digest (33 bytes) | CHAIN-SPEC L412-414 | `a5df…02` | computed from genesis UTXO set |
| Genesis header id | CHAIN-SPEC L415-417 | `b024…` | computed from first mined block |
| Genesis boxes JSON | CHAIN-SPEC L418-420 | `test-vectors/mainnet/genesis_boxes.json` | new `test-vectors/sigmachain-testnet/genesis_boxes.json` with emission/treasury/LP boxes |

Genesis box construction (Phase 4): emission box guarded by `emission.es`, treasury accumulation box at `treasury_accumulation.es` script hash, LP accumulation box at `lp_accumulation.es` script hash. Total nanoYOLO at genesis = 177_412_882_500_000_000 (177,412,882.5 coins per handoff).

### Bootstrap (`BootstrapParams`)

| Parameter | Location | Ergo mainnet | YOLO target |
|---|---|---|---|
| Seed peers | CHAIN-SPEC L602-616 | 13 IPs on :9030 | ❓ TBD — likely empty initially; operator provides via config |
| Script-validation checkpoint | CHAIN-SPEC L622-627 | `(1_231_454, ca5a…)` | **`None`** (new chain, no historical checkpoint) |

### Storage rent (the YOLO-unique consensus change)

| Parameter | Location | Ergo behavior | YOLO target |
|---|---|---|---|
| Storage fee factor (nano/byte) | VALIDATION `storage_rent.rs:30`; default voted value 1_250_000 | 1_250_000 nano/byte over 4yr | **312_500 nano/byte over 1yr** (handoff: same per-byte annualized cost; storage_rent.md spec) — but this is a voted protocol param, so configured as default min/max/initial in `ergo-validation/src/voting/recompute.rs` |
| Storage rent eligibility period | VALIDATION `tx/script.rs::is_storage_rent_eligible` (L339); Scala uses `StoragePeriod = 1_051_200` blocks (4yr @ 120s) | 1_051_200 blocks | **1_577_880 blocks** (1yr @ 20s) |
| Rent collection authorization | VALIDATION `tx/script.rs::check_storage_rent` (L353) | anyone may construct a tx that spends a rent-eligible box | 🆕 **CONSENSUS RULE: rent claim must appear in the coinbase tx (or be authored by the block miner)** — block validation rejects non-coinbase storage-rent claims |
| Min value per byte increase | VALIDATION `header.rs:403` | 2 (linear pricing param) | keep |
| Min value voting step / range | VALIDATION `voting/recompute.rs:345,354,365` | step 10, min 0, max 10_000 | keep |
| Miner-side self-claim opt-in flag | `ergo-mining/src/config.rs` `storage_rent_self_claim` (default `false`) | opt-in for miners | on YOLO, **make this default `true` and the only legal path** |

### Block version / ErgoTree activation

| Parameter | Location | Ergo behavior | YOLO target |
|---|---|---|---|
| Genesis block version | inferred — Scala `LaunchParameters` / `TestnetLaunchParameters` | mainnet starts at v1, transitions v1→v2; testnet starts at v4 (`Interpreter60Version`) | **start at v4 from genesis** (like testnet) — no transition needed |

## Decisions locked (2026-06-04)

1. **Network magic bytes** — `[0x59, 0x4F, 0x4C, 0x4F]` ASCII "YOLO" ✅
2. **Address prefix byte** — `0x20` for sigmachain-testnet; requires extending `ergo-ser::address::NetworkPrefix` enum with a new variant ✅
3. **P2P port** — `9120`; **REST API port** — `9153` ✅
4. **Monetary curve** — geometric halving (50→25→…→1 tail), structural fork of `MonetaryParams` required; see Monetary / Emission section above ✅
5. **Miner reward delay** — `4320` blocks (1 day at 20s) ✅
6. **Repo layout** — `07-sigmachain-node/{arkadianet-ergo-reference, sigmachain-node}` ✅

## Still open (lower-priority — can decide during implementation)

- **Initial seed peers** — likely empty Vec for the testnet; operator provides via local config for single-node dev. Defer to Phase 5.
- **header_chain_diff** — propose 600 (200 min staleness at 20s). Defer to Phase 3.
- **EmissionCurve API shape** — enum vs. trait object vs. extending `MonetaryParams`. Will pick during Phase 3 implementation; chain-spec's "narrow params" pattern points toward a new struct + `Option` on `ChainSpec`.

## What's NOT in this inventory (deferred / N/A)

- **PoW algorithm** — handoff explicitly defers Autolykos2 → Ethash decision.
- **DeFi stack params** — out of scope (Phase 10 in old checklist, post-mainnet).
- **Bridge / AEther params** — out of scope.
- **Mining pool integration** — out of scope.

## Crate layering recap (from `ARCHITECTURE.md`)

17 crates in a strict DAG. The ones we'll touch:
- L2 `ergo-chain-spec` — almost all parameter changes land here
- L2 `ergo-ser` — extend `NetworkPrefix` enum (1 small change)
- L3 `ergo-validation` — storage rent eligibility period + miner-only rule
- L5 `ergo-mining` — flip storage-rent self-claim default to `true`; verify coinbase builder against emission.es
- L7 `ergo-node` — wire `Network::SigmaChainTestnet` into config parser; add a sigmachain genesis profile loader

Crates that should not be touched: `ergo-sigma` (the interpreter — point of fork is to keep it identical), `ergo-primitives`, `ergo-ser` (beyond NetworkPrefix), `ergo-state`, `ergo-p2p`, `ergo-sync`, `ergo-mempool`, `ergo-api`, `ergo-indexer*`, `ergo-rest-json`, `ergo-crypto` (until we decide on PoW algo).
