# YOLO Governance — Implementation Plan

**Target chain:** SigmaChain (Rust node fork, Ethash PoW, 20-second blocks)
**Contract language:** ErgoScript (.es)
**Test/deployment language:** Rust (sigma-rust), matching the emission contract pattern
**Reference model language:** Python (matches `emission_model.py` pattern)
**Off-chain voting bot language:** Python (talks to node REST API, no contract compilation needed)
**Governance model:** DuckDAO-style sequential on-chain vote counting — no AVL, no proof server, no VPS liveness dependency
**Scope of V1:** Lock YOLO, mint vYOLO, vote on proposals that disburse a proportion of treasury YOLO to arbitrary recipients

---

## 1. System Overview

YOLO is SigmaChain's native coin (the equivalent of ERG on Ergo). A governance contract cannot directly read YOLO balances across arbitrary UTXOs, so governance operates on a proxy token — vYOLO — that is 1:1 peg-backed by locked YOLO.

### Key properties

| Property | How it's enforced |
|---|---|
| vYOLO supply ≤ YOLO emitted | Structural: vYOLO can only be minted by depositing YOLO into vaults; deposits are 1:1; cap = emission cap |
| No token contamination in peg boxes | `tokens.size` checks reject any extra tokens in vault or reserve |
| Concurrent TXs | 5 independent vault/reserve pairs with 5 state NFTs and 5 reserve NFTs |
| Vault isolation from vYOLO | Twin-contract pattern: vault holds only YOLO + state NFT; reserve holds only vYOLO + reserve NFT |
| Flash-loan vote resistance | vYOLO must be locked in voter boxes for the full voting window |
| No off-chain proof server | Sequential vote counting — each TX consumes one voter box, no AVL accumulator |

---

## 2. Repository Structure

Matches the pattern established by the emission contract work (`emission.es`, `emission_model.py`, `emission_test.rs`, `PARAMETERS.md`, `EDGE_CASES.md`).

```
yolo-governance/
├── contracts/
│   ├── vault.es                  # Holds YOLO + state NFT
│   ├── reserve.es                # Holds vYOLO + reserve NFT
│   ├── userVote.es               # Per-voter vote box
│   ├── timeValidator.es          # Height-window guard for voting
│   ├── counting.es               # 4-phase vote tally state machine (DuckDAO port)
│   ├── proposal.es               # Proposal metadata + execution
│   └── treasury.es               # Treasury box guard (deposit + proportional withdrawal)
├── tests/
│   ├── vault_test.rs             # Deposit, redeem, fuzz, adversarial
│   ├── reserve_test.rs           # Mirror of vault tests
│   ├── twin_pair_test.rs         # Atomic pair TXs (deposit + redeem paths)
│   ├── voting_happy_path_test.rs
│   ├── voting_adversarial_test.rs
│   └── integration_test.rs       # Full cycle: lock → propose → vote → execute
├── models/
│   ├── vault_model.py            # Python reference model for peg invariants
│   └── voting_model.py           # Proposal lifecycle simulation
├── scripts/
│   ├── genesis/
│   │   ├── 01_mint_state_nfts.rs
│   │   ├── 02_mint_reserve_nfts.rs
│   │   ├── 03_mint_vYolo.rs
│   │   ├── 04_deploy_vaults.rs
│   │   └── 05_deploy_reserves.rs
│   ├── deploy_voting_system.rs
│   └── compile_contracts.rs      # Compile all .es files with NFT IDs baked in
├── bot/
│   ├── voting_bot.py             # Python state machine (DuckDAO pattern)
│   ├── config.yaml
│   ├── requirements.txt
│   └── README.md
├── frontend/                     # Phase 4 placeholder
├── Cargo.toml
├── PARAMETERS.md                 # All magic numbers + genesis token IDs
├── EDGE_CASES.md                 # Attack vectors + mitigations + test coverage
├── ARCHITECTURE.md               # Box diagrams + TX shapes
└── README.md
```

### Cargo dependencies (matching emission contract)

```toml
[dependencies]
ergo-lib = "0.28"        # sigma-rust; pinned to emission contract version
ergotree-ir = "0.28"
sigma-test-util = "0.28"
thiserror = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[dev-dependencies]
proptest = "1.0"         # Property-based fuzzing for conservation invariants
```

If the emission contract is pinned to a specific sigma-rust version, match it exactly. Cross-referencing versions avoids compile-time API drift between the two workstreams.

---

## 3. Token Genesis Specification

### 3.1 State NFTs (5 singletons, vault markers)

- Supply: 1 each
- Naming: `YOLO_VAULT_STATE_NFT_1` … `YOLO_VAULT_STATE_NFT_5`
- Minted via 5 separate TXs, each consuming a unique genesis input → deterministic token IDs
- IDs baked into `vault.es` and `reserve.es` at compile time

### 3.2 Reserve NFTs (5 singletons, reserve markers)

- Supply: 1 each
- Naming: `YOLO_VAULT_RESERVE_NFT_1` … `YOLO_VAULT_RESERVE_NFT_5`
- Paired 1:1 with state NFTs (state NFT 1 ↔ reserve NFT 1, etc.)
- IDs baked into both scripts

### 3.3 vYOLO Token

- Supply: Exactly equal to YOLO total emission cap (in smallest unit)
- Decimals: 9 (matching YOLO: 1 coin = 1,000,000,000 nanocoins)

**Overflow check — RESOLVED:**

| Value | Amount |
|-------|--------|
| YOLO total supply | 177,412,882.5 coins |
| In nanocoins | 177,412,882,500,000,000 |
| i64 max | 9,223,372,036,854,775,807 |
| **Headroom** | **52x** |

vYOLO supply fits comfortably in i64. No decimal adjustment needed.

**⚠️ Proportion math overflow — CRITICAL (see §7.8):** While supply itself fits i64, the treasury's proportion-based distribution formula (`value * proportion / denominator`) overflows when multiplying nanocoin-scale values. Split-math pattern required. See §7.8 for the mandatory solution.

### 3.4 Genesis sequence (strict ordering)

1. Mint 5 state NFTs (record all IDs immediately in `PARAMETERS.md`)
2. Mint 5 reserve NFTs (record all IDs)
3. Mint vYOLO at emission cap (record ID)
4. Compile `vault.es` with state NFT + reserve NFT IDs hardcoded (5 versions, one per pair)
5. Compile `reserve.es` with reserve NFT + state NFT + vYOLO IDs hardcoded (5 versions)
6. Create 5 vault boxes: each holds `{state_nft_i: 1, value: 0}`
7. Create 5 reserve boxes: each holds `{reserve_nft_i: 1, vYOLO: cap/5, value: min_box_value}`

Persist all IDs, script hashes, and box IDs to `PARAMETERS.md` after each step. The voting bot and frontend both need these.

---

## 4. Vault Contract Specification (`vault.es`)

### 4.1 Box shape

| Field | Value |
|---|---|
| `value` | Locked YOLO (nano units) |
| `tokens(0)` | (`state_nft_i`, 1L) |
| `tokens.size` | Exactly 1 |
| Registers | None used |

### 4.2 Compile-time constants (per vault instance)

```
STATE_NFT_ID    = <state_nft_i>
RESERVE_NFT_ID  = <reserve_nft_i>
```

### 4.3 Full invariant

```scala
{
    // CONSTITUTIONAL INVARIANT: This vault has NO governance-controlled spend path.
    // Governance proposals can only disburse from the treasury box, never from vaults.
    // The peg (vYOLO supply ≤ locked YOLO) is structurally enforced, not governable.

    val StateNftId   = fromBase16("...")
    val ReserveNftId = fromBase16("...")

    // ---- Self integrity ----
    val selfValid =
        SELF.tokens.size == 1 &&
        SELF.tokens(0)._1 == StateNftId &&
        SELF.tokens(0)._2 == 1L

    // ---- Output (continued vault) integrity ----
    val out = OUTPUTS(0)
    val outValid =
        out.propositionBytes == SELF.propositionBytes &&
        out.tokens.size == 1 &&
        out.tokens(0)._1 == StateNftId &&
        out.tokens(0)._2 == 1L

    // ---- Locate paired reserve by NFT filter ----
    val reserveIn = INPUTS.filter { (b: Box) =>
        b.tokens.size > 0 && b.tokens(0)._1 == ReserveNftId
    }
    val reserveOut = OUTPUTS.filter { (b: Box) =>
        b.tokens.size > 0 && b.tokens(0)._1 == ReserveNftId
    }
    val pairingValid = reserveIn.size == 1 && reserveOut.size == 1

    // ---- Conservation ----
    val deltaVaultYolo    = out.value - SELF.value
    val deltaReserveVYolo = reserveOut(0).tokens(1)._2 - reserveIn(0).tokens(1)._2
    val conservation      = deltaVaultYolo + deltaReserveVYolo == 0L
    val nonTrivial        = deltaVaultYolo != 0L

    // ---- Single vault input (defense in depth) ----
    val singleVaultInput = INPUTS.filter { (b: Box) =>
        b.tokens.size > 0 && b.tokens(0)._1 == StateNftId
    }.size == 1

    sigmaProp(
        selfValid && outValid && pairingValid &&
        conservation && nonTrivial && singleVaultInput
    )
}
```

### 4.4 Storage rent impact — RESOLVED (not blocking)

Storage rent is active on SigmaChain (12-month cycle, 312,500 nano/byte/year, miner-only collection). There is no rent exemption mechanism — vault and reserve boxes are subject to rent like all other boxes.

**Why this is not a problem in practice:**

1. **Constant activity resets the clock.** Every deposit or redeem TX recreates the vault/reserve box, resetting the 12-month rent timer. Any governance system with non-trivial usage will never see rent collected.
2. **Negligible drift even at worst case.** If a vault pair sits completely idle for a full year, rent on a ~250-byte box is 0.078 coins — trivial compared to the YOLO locked inside.
3. **Bounded peg drift.** If rent is collected, `circulating_vYOLO` exceeds `locked_YOLO` by at most 0.078 coins per vault per year (5 vaults × 0.078 = 0.39 coins/year max total drift). This is economically negligible.
4. **Operational mitigation.** The voting bot (or any user) can periodically deposit a trivial amount (1 nanocoin) into idle vault pairs to reset the rent clock. This is a routine operational task, not a protocol concern.

**Action:** Document the bounded peg drift in `EDGE_CASES.md`. Add vault-touch heartbeat to the voting bot's responsibilities. No protocol-level changes required.

---

## 5. Reserve Contract Specification (`reserve.es`)

### 5.1 Box shape

| Field | Value |
|---|---|
| `value` | Minimum box value (rent buffer only) |
| `tokens(0)` | (`reserve_nft_i`, 1L) |
| `tokens(1)` | (`vYolo_token_id`, N) |
| `tokens.size` | Exactly 2 |
| Registers | None used |

### 5.2 Compile-time constants

```
RESERVE_NFT_ID  = <reserve_nft_i>
STATE_NFT_ID    = <state_nft_i>
VYOLO_TOKEN_ID  = <vYolo_token_id>
```

### 5.3 Full invariant

```scala
{
    val ReserveNftId = fromBase16("...")
    val StateNftId   = fromBase16("...")
    val VYoloId      = fromBase16("...")

    // ---- Self integrity ----
    val selfValid =
        SELF.tokens.size == 2 &&
        SELF.tokens(0)._1 == ReserveNftId &&
        SELF.tokens(0)._2 == 1L &&
        SELF.tokens(1)._1 == VYoloId

    // ---- Output integrity ----
    val out = OUTPUTS(1)
    val outValid =
        out.propositionBytes == SELF.propositionBytes &&
        out.tokens.size == 2 &&
        out.tokens(0)._1 == ReserveNftId &&
        out.tokens(0)._2 == 1L &&
        out.tokens(1)._1 == VYoloId &&
        out.value == SELF.value

    // ---- Locate paired vault ----
    val vaultIn = INPUTS.filter { (b: Box) =>
        b.tokens.size > 0 && b.tokens(0)._1 == StateNftId
    }
    val vaultOut = OUTPUTS.filter { (b: Box) =>
        b.tokens.size > 0 && b.tokens(0)._1 == StateNftId
    }
    val pairingValid = vaultIn.size == 1 && vaultOut.size == 1

    // ---- Conservation (mirrors vault) ----
    val deltaReserveVYolo = out.tokens(1)._2 - SELF.tokens(1)._2
    val deltaVaultYolo    = vaultOut(0).value - vaultIn(0).value
    val conservation      = deltaVaultYolo + deltaReserveVYolo == 0L
    val nonTrivial        = deltaReserveVYolo != 0L

    // ---- Single reserve input ----
    val singleReserveInput = INPUTS.filter { (b: Box) =>
        b.tokens.size > 0 && b.tokens(0)._1 == ReserveNftId
    }.size == 1

    sigmaProp(
        selfValid && outValid && pairingValid &&
        conservation && nonTrivial && singleReserveInput
    )
}
```

Conservation is checked from both sides (defense-in-depth). Both scripts must agree for any TX to validate.

---

## 6. Atomic TX Shapes

### 6.1 Deposit (user locks X YOLO, receives X vYOLO)

```
INPUTS:
  0: Vault_i      [state_nft_i, V YOLO]
  1: Reserve_i    [reserve_nft_i, R vYOLO]
  2: User funds   [X YOLO + miner fee]

OUTPUTS:
  0: Vault_i'     [state_nft_i, (V + X) YOLO]
  1: Reserve_i'   [reserve_nft_i, (R - X) vYOLO]
  2: User receipt [X vYOLO] @ user_pk
  3: Miner fee
```

### 6.2 Redeem (user burns X vYOLO, receives X YOLO)

```
INPUTS:
  0: Vault_i      [state_nft_i, V YOLO]
  1: Reserve_i    [reserve_nft_i, R vYOLO]
  2: User vYOLO   [X vYOLO + miner fee]

OUTPUTS:
  0: Vault_i'     [state_nft_i, (V - X) YOLO]
  1: Reserve_i'   [reserve_nft_i, (R + X) vYOLO]
  2: User payout  [X YOLO] @ user_pk
  3: Miner fee
```

### 6.3 Frontend vault routing

Frontend must pick which of the 5 pairs to use per TX:
- For deposit: any pair with state NFT present in mempool (not already being spent)
- For redeem: pair with sufficient YOLO balance for the requested redemption

Mempool collision handling: retry with next vault pair if TX fails due to vault already spent. Standard Spectrum-style router logic.

---

## 7. Voting Layer (DuckDAO pattern, ported)

All four contracts adapted from DuckPools `treasury-system-voting-*` documented in the EKB. Changes: swap QUACKS → vYOLO, retune height constants for 20s blocks, generalize proposal execution.

### 7.1 Block time retuning

SigmaChain 20s blocks produce 6× more blocks per unit time than Ergo's ~2min blocks. Every height constant inherited from DuckPools multiplies by 6.

| Window | Target duration | Ergo blocks | SigmaChain blocks |
|---|---|---|---|
| Voting window | 3 days | 2,160 | 12,960 |
| Cancellation cooldown | 24 hours | 720 | 4,320 |
| Counting phase | 6 hours | 180 | 1,080 |
| Execution grace period | 24 hours | 720 | 4,320 |

Final values in `PARAMETERS.md`. All durations configurable at proposal creation time with sane floors/ceilings.

### 7.2 `timeValidator.es`

Gates voter box creation by height window. Height constants passed via compile-time or counter box register reference. Structure preserved from DuckPools `timeValidator.md`.

### 7.3 `userVote.es`

Per-voter box. Registers:
- `R4`: Vote direction (Int, 1 = yes, 0 = no)
- `R5`: Proposal ID (Coll[Byte])
- `R6`: Voter public key (SigmaProp)
- `R7`: Cancellation unlock height (Int)
- `R8`: Submission deadline (Int) — must precede `nextVoteDeadline` in counter box

Tokens:
- `tokens(0)`: Vote NFT (`validVoteId`) — authenticates this as a valid voter box
- `tokens(1)`: Locked vYOLO (any quantity meeting minimum stake)

Two spend paths (matching DuckDAO `userVote` contract):
- **Cancel** (after cooldown AND before counter box deadline): voter signs (Sigma-protocol authentication via R6), vYOLO returned to voter address. Vote NFT returned to voter (not burned — vote was never counted, so no anti-double-vote concern).
- **Submit** (during counting phase): consumed by counting contract. Vote NFT is **burned** (removed from all outputs) to prevent double-counting. vYOLO released to voter's address.

### 7.4 `counting.es`

4-phase state machine, ported from DuckDAO's `counting.md`. Singleton counter box holds state, authenticated by a state token. Phase transitions are gated by block height (`HEIGHT`).

**Registers:**

| Register | Type | Purpose |
|----------|------|---------|
| `R4` | Long | Vote deadline (countdown timer for current voting period) |
| `R5` | (Long, Long) | Vote tally tuple: (nominated proportion, votes in favor) |
| `R6` | Coll[Byte] | Recipient ergotree hash (160-bit blake2b256 commitment) |
| `R7` | Long | Total votes accumulated in counting period |
| `R8` | Long | Initiation amount (vYOLO staked by proposer — see §7.6) |
| `R9` | Long | Validation votes count (yes-votes subset of total) |

**State machine phases:**

1. **Before counting (`isBeforeCounting`):** Accepts initiation of a new vote. Validates that initiator provides sufficient vYOLO to meet `initiationHurdle`. Resets tallies, stores nominated proportion and recipient. Initiator's staked vYOLO amount recorded in R8.

2. **Counting period (`isCountingPeriod`):** Accepts individual voter boxes as inputs. Accumulates vote tallies — sums vYOLO amounts from voters, tracks validation votes (yes-votes). **Burns vote NFTs** (`validVoteId`) from each consumed voter box to prevent double-counting. Validates each voter box holds valid vote token and vYOLO.

3. **Vote validation period (`isVoteValidationPeriod`):** Loads proposal box, checks against thresholds:
   - Minimum total votes: V1 floor of 1,000,000 vYOLO (see §7.9)
   - **Tiered support thresholds:** If proportion > 1,000,000 (>10% of treasury): requires **90% support**. Otherwise: requires **50% support**. Support = (validation votes / total votes) × 1000.
   - If approved: transitions proposal state token qty from 1 → 2. If failed: resets for next cycle.

4. **New proposal period (`isNewProposalPeriod`):** Allows new proposals to be initiated in the next voting round.

Each counting TX consumes one voter box + counter box, increments tallies by voter's staked vYOLO, returns voter's vYOLO to their address, emits updated counter box.

### 7.5 `proposal.es`

V1 proposal type: "disburse a proportion of treasury YOLO to recipient address Y." Ported from DuckDAO's `proposal.md` with proportion-based distribution (inherently safe — can never propose more than treasury holds).

**State tracking:** Token-quantity pattern (DuckDAO-style). Proposal box holds a state token:
- Quantity **1** = pending/voting phase
- Quantity **2** = execution-ready (passed validation)

This is more tamper-resistant than register-based state (token quantities are protocol-enforced; registers can be set to anything in outputs).

**Proposal box registers:**

| Register | Type | Purpose |
|----------|------|---------|
| `R4` | Long | `proportion` — numerator out of 10,000,000 denominator (e.g., 500,000 = 5% of treasury) |
| `R5` | Coll[Byte] | `recipient` — recipient's ErgoTree script bytes |
| `R6` | Long | `validationHeight` — block height at which voting concludes |
| `R7` | Int | `supportBps` — basis points needed in favor (auto-set: 5000 for proportion ≤ 1M, 9000 for proportion > 1M) |
| `R8` | Int | `votingWindowEnd` — last block accepting votes |
| `R9` | Int | `discussionDeadline` — voting cannot begin before this height (mandatory discussion period) |

**Bounded parameter ranges (enforced in contract):**
- `proportion`: 1 – 10,000,000 (0.00001% – 100%)
- `supportBps`: 5000 – 10000 (50% – 100%)
- Voting window: 4,320 – 43,200 blocks (1 – 10 days)
- Discussion window: 2,880 – 17,280 blocks (16 hours – 4 days)

**Branch 1 — State advancement (normal case):** Counting box validates vote passed thresholds. Successor proposal box preserves identical script, R4, R5 (immutable). State token quantity increments from 1 → 2.

**Branch 2 — Proposal execution (terminal case):** Treasury box (INPUTS carrying treasury NFT) distributes proportional YOLO to recipient. State token burned from all outputs (proposal cannot be reused). Uses split-math pattern (§7.8) for overflow-safe proportion calculation.

**New-treasury-mode (governance migration):** When `proportion == 10,000,000` (100%), the entire treasury transfers to a new governance script specified in `recipient` (R5). This is the on-chain governance upgrade mechanism — no hard fork needed. Triggers the 90% elevated support threshold automatically (proportion > 1M). No single actor can trigger it; requires super-majority vote.

**On fail or timeout:** Proposal self-destructs, treasury untouched.

V2 scope (out of V1): arbitrary script execution, multi-recipient splits, parameter proposals (quorum changes, etc.). V1 does **not** allow governance to change governance parameters. Keep `proposal.es` simple enough that V2 extends via new proposal-type contracts.

### 7.6 Initiation hurdle (anti-spam)

Proposer must stake N vYOLO to create a proposal. V1 default: max(1% of circulating vYOLO, 100,000 vYOLO floor). Stake lifecycle:

- **On proposal completion (pass or fail):** Staked vYOLO returned to proposer's address. No penalty for failed proposals — honest participation is encouraged.
- **On proposer-initiated cancel:** Staked vYOLO **forfeited to the treasury box**. This diverges from DuckDAO (which does not forfeit). The forfeit makes proposal spam expensive and ensures cancelled proposals have a real cost.

**Implementation in `counting.es`:** During `isBeforeCounting` phase, initiator's vYOLO stake amount is stored in R8. The initiator's return address is stored in the initiation box (or a dedicated register). The bot handles the refund TX (on completion) or forfeit TX (on cancel, sending vYOLO to treasury).

### 7.7 `treasury.es`

Single YOLO-holding box guarded by treasury contract, ported from DuckDAO's `treasury.md`. Authenticated by a singleton treasury NFT at `tokens(0)`.

**Funding (no genesis allocation — earned from block 1):**
- Emission contract's 10% treasury split (per `emission.es`) — ~5 coins/block initially, ~21,600 coins/day
- Voluntary contributions (anyone can add YOLO via deposit branch)
- Forfeited proposer stakes (see §7.6)

First meaningful proposal is viable within weeks of launch.

**Deposit branch (`INPUTS(0) == SELF`):** Accepts YOLO additions. Script invariant: successor retains identical proposition bytes. Value monotonicity: successor holds ≥ current value.

**Withdrawal branch (`INPUTS(0)` carries counter token qty = 2):** Proportional distribution matching DuckDAO:
- Read `R4[Long]` → `proportion` (numerator; denominator = 10,000,000)
- Read `R5[Coll[Byte]]` → `recipient` (recipient's ErgoTree)
- Compute `valueAwarded` using split-math pattern (§7.8): overflow-safe proportional calculation
- Successor treasury retains NFT, remaining YOLO after disbursement
- Recipient box receives awarded YOLO at recipient script

**New-treasury-mode (`proportion == 10,000,000`):** Full treasury transfer to new governance script. Recipient field (R5) is the new treasury's ErgoTree. All assets transferred intact. Treasury NFT stays with the successor box. This is the governance migration mechanism.

### 7.8 Split-math pattern (mandatory — i64 overflow protection)

**Problem:** DuckDAO's naive proportion formula `valueAwarded = value * proportion / 10,000,000` overflows i64 at YOLO's nanocoin scale. A treasury holding 17.7M coins (~1.77 × 10^16 nanocoins) multiplied by any proportion exceeds i64 max (9.22 × 10^18).

**Solution:** Split the multiplication to avoid intermediate overflow:

```scala
// NEVER do: val awarded = value * proportion / denom  (OVERFLOWS)
// Instead:
val denom = 10000000L
val wholePart     = (value / denom) * proportion
val remainderPart = ((value % denom) * proportion) / denom
val awarded       = wholePart + remainderPart
```

**Safety proof:**
- `value / denom` ≤ 1.77 × 10^10 — fits i64
- `(value / denom) * proportion` ≤ 1.77 × 10^17 — fits i64
- `value % denom` < 10^7 — fits i64
- `(value % denom) * proportion` < 10^14 — fits i64
- All intermediates safe even at worst case (full supply, 99.99999% proportion)
- Rounding error: at most 1 nanocoin (negligible)

**This pattern must be used in `treasury.es` (withdrawal branch) and anywhere else proportion math appears.** Add dedicated overflow tests in `treasury_test.rs`.

### 7.9 Quorum threshold

DuckDAO hardcodes 300B quacks minimum. For YOLO:

| Metric | Value |
|--------|-------|
| Total YOLO supply | ~177.4M coins |
| Year 1 emission | ~78.9M coins |
| V1 quorum floor | **1,000,000 vYOLO** (~0.56% of total supply) |

The quorum floor is hardcoded in V1. Dynamic "% of circulating vYOLO" is impractical on-chain (counting contract cannot easily read circulating supply). The bounded range in `proposal.es` (500–5000 bps) gives proposers flexibility within guardrails for individual proposal quorum requirements.

Adjustable post-V1 via governance migration (new-treasury-mode) if the floor proves too low or high.

---

## 8. Off-Chain Voting Bot

Python, modeled on DuckPools `voting-bot`. Runs on any lightweight host (VPS, Raspberry Pi, laptop).

### 8.1 Responsibilities

- Monitor chain for new proposals
- Transition proposals through lifecycle stages (before-counting → counting → validation → new-proposal)
- Submit counting TXs for each voter box during counting phase (one voter box per TX)
- Submit proposal execution TX on pass (treasury withdrawal)
- Emit refund TXs for voter boxes after counting complete
- Handle proposer stake refund (on completion) or forfeit to treasury (on proposer cancel)
- **Vault heartbeat:** Periodically touch idle vault/reserve pairs (trivial deposit) to reset storage rent clock. Frequency: at least once every 6 months per pair (well within the 12-month rent cycle).

### 8.2 Stateless + resumable

Bot reads all state from chain. No local database required. If bot restarts mid-operation, it resumes by querying current counter box state.

### 8.3 Multi-bot support

Anyone can run a bot. No single-bot dependency. First valid TX wins the mempool race — subsequent attempts from other bots fail harmlessly on double-spend.

### 8.4 Config

- Node REST API endpoint
- Wallet credentials (for TX signing fees)
- Polling interval
- Gas/fee policy

---

## 9. Frontend (Phase 4 — out of V1 scope, noted for planning)

React app, reuses patterns from Etcha V3 wizard. Screens:
- Lock/Redeem (vault interaction)
- Proposal list + detail
- Vote (create userVote box)
- Cancel vote
- Proposal creation wizard

Not in V1 deliverables. Spec frozen when contracts + bot pass integration tests.

---

## 10. Testing Plan

### 10.1 Unit tests per contract (Rust, sigma-rust)

Matching the pattern in `emission_test.rs`. Each contract gets its own file with:
- Happy path TXs
- Invariant violation cases (each check rejected individually)
- Edge cases (zero amounts, max amounts, boundary heights)
- Adversarial attempts (script replacement, token contamination, double-input)

### 10.2 Property-based tests (proptest)

For vault and reserve: generate random deposit/redeem sequences and assert the peg invariant (`sum of circulating vYOLO == sum of locked YOLO across all 5 vaults`) holds after every TX.

### 10.3 Integration test

Full cycle in one test file:
1. Genesis setup (mint tokens, deploy 5 vault/reserve pairs, deploy treasury + counting + proposal contracts)
2. 3 users deposit various YOLO amounts across different vault pairs
3. User creates proposal staking vYOLO (proportion-based: e.g., 500,000 = 5% of treasury)
4. Discussion window passes (advance height past `discussionDeadline`)
5. 3 users vote (2 yes, 1 no) with different vYOLO weights
6. Bot advances through counting TXs (vote NFTs burned)
7. Proposal passes quorum + tiered threshold (50% for normal, 90% for large)
8. Execution TX disburses proportional treasury YOLO to recipient (using split-math)
9. Proposer stake refunded
10. Users redeem vYOLO → recover YOLO
11. Assert final peg invariant across all 5 vault pairs

### 10.4 Acceptance criteria

- All unit tests pass
- Property tests run 10,000 iterations without invariant violation
- Integration test completes end-to-end
- `audit_contract` + `audit_verify` clean pass on all 7 contracts (`vault.es`, `reserve.es`, `counting.es`, `proposal.es`, `userVote.es`, `timeValidator.es`, `treasury.es`)
- Gas / JitCost measurements documented per TX type in `PARAMETERS.md`

---

## 11. Build Sequence

### Phase 1 — Peg layer (1 week)
1. Write `vault.es` and `reserve.es`
2. Run `audit_contract` + `audit_verify` on both
3. Write `vault_test.rs`, `reserve_test.rs`, `twin_pair_test.rs`
4. Write `vault_model.py` reference
5. Write genesis scripts (01–05)
6. **Gate:** 100% test pass + clean audit

### Phase 2 — Voting layer (2 weeks)
1. Port `counting.es` from DuckDAO — faithful 4-phase state machine with height gates, vote NFT burning, tiered support validation (50%/90%), initiation stake forfeit logic
2. Port `proposal.es` from DuckDAO — token-quantity state progression (1→2), immutable R4/R5, discussion window (R9), bounded parameter ranges, new-treasury-mode branch (proportion=10,000,000)
3. Port `userVote.es` from DuckDAO — cancel (cooldown + deadline gate + Sigma auth) and submit (counting consumption + NFT burn) paths
4. Port `timeValidator.es` — height-window guard, reject vote box creation before discussion window
5. Write `treasury.es` — deposit branch (value monotonicity), withdrawal branch (proportional distribution with split-math), new-treasury-mode branch
6. Retune all height constants ×6 for 20s blocks
7. Swap all QUACKS references → vYOLO
8. Implement and test split-math pattern (§7.8) in `treasury.es` with dedicated overflow tests
9. `audit_contract` + `audit_verify` all five voting contracts
10. Write per-contract Rust tests + adversarial cases
11. **Gate:** 100% test pass + clean audit on all contracts

### Phase 3 — Bot + integration (1 week)
1. Write Python voting bot
2. Write integration test covering full cycle
3. **Gate:** Integration test passes, bot runs stateless recovery

### Phase 4 — Frontend (out of V1)

### Phase 5 — Testnet deployment (1 week after SigmaChain testnet exists)
1. Deploy genesis tokens to testnet
2. Deploy all 7 contracts
3. Run voting bot against testnet
4. Submit dry-run proposals and executions
5. Stress test with concurrent deposits across all 5 pairs

### Phase 6 — Mainnet (post-testnet sign-off)

**Total V1 calendar time: 4 weeks.** No protocol-level changes required (storage rent operates normally, no exemption needed).

---

## 12. Open Questions / Blockers — Resolution Status

| # | Question | Status | Resolution |
|---|----------|--------|------------|
| 1 | Storage rent exemption | **RESOLVED** | No exemption. Rent is active; vault/reserve boxes reset clock on every TX. Bounded drift ~0.078 coins/vault/year is negligible. Bot heartbeat as operational hygiene. See §4.4. |
| 2 | vYOLO decimals vs emission cap | **RESOLVED** | 9 decimals, 177.4M coin supply = 1.77 × 10^17 nanocoins. 52x headroom under i64 max. Split-math required for proportion calculations (§7.8). |
| 3 | Treasury genesis funding | **RESOLVED** | No genesis allocation. Treasury starts at zero, funded from 10% emission split (~21,600 coins/day from block 1). First viable proposal within weeks. |
| 4 | Initiation hurdle formula | **CONFIRMED** | max(1% of circulating vYOLO, 100,000 vYOLO floor). Forfeit on proposer cancel → forfeited stake goes to treasury. |
| 5 | Quorum / thresholds | **RESOLVED** | Hardcoded 1,000,000 vYOLO floor. Tiered support: 50% normal, 90% for proportion > 1M (>10% of treasury). Bounded ranges enforced in contract. |
| 6 | sigma-rust version pin | **CONFIRMED** | ergo-lib 0.28, matching emission contract. Note: sigma-rust 0.28 cannot compile nested typed-lambdas. If governance contracts hit this limitation, Scala AppKit test harness is the fallback (same path as emission.es). |
| 7 | Vault rebalancing | **DEFERRED** | Skipped in V1 (confirmed). |

**No remaining blockers for Phase 1 kickoff.**

---

## 13. Known Risks

| Risk | Severity | Status |
|---|---|---|
| Storage rent drains vault peg | Low | **Resolved.** Bounded drift ~0.39 coins/year total. Bot heartbeat resets clock. §4.4 |
| i64 overflow on vYOLO supply | N/A | **Resolved.** 52x headroom confirmed. §3.3 |
| **i64 overflow on proportion math** | **High** | **Mitigated via split-math pattern (§7.8).** Must be used in `treasury.es` and tested. |
| Voting bot unavailability stalls proposals | Medium | Stateless design, anyone can run a bot, unlimited parallel bots |
| Mempool collision across 5 vault pairs | Low | Frontend retry logic; 5 pairs gives practical headroom |
| DuckDAO height constants ported incorrectly | Medium | Explicit 6× multiplier table (§7.1); integration test covers full cycle |
| `proposal.es` V1 only supports disbursement | Low (by design) | V2 extensibility via new proposal-type contracts; new-treasury-mode covers governance migration |
| Flash-loan voting attack | Mitigated | vYOLO must be locked in voter box for full window |
| Vote-buying / sybil | Inherent | Out of scope for V1; document as known governance limitation (§15.8) |
| sigma-rust compiler limitation | Medium | Nested typed-lambdas don't compile. Fallback: Scala AppKit test harness. |

---

## 14. Handoff Notes for Claude Code

- Follow file structure in §2 exactly (now 7 contracts including `treasury.es`)
- Match sigma-rust version to emission contract: **ergo-lib 0.28** (don't upgrade unilaterally)
- Every contract gets `audit_contract` + `audit_verify` before writing tests
- Document every magic number in `PARAMETERS.md` immediately on introduction
- Document every attack consideration in `EDGE_CASES.md` as tests are written
- Python reference models should be independent reimplementations (not translations of the Rust tests) — catches logic errors
- **Split-math pattern (§7.8) is mandatory** in any contract performing proportion calculations. Test with worst-case values (full supply × max proportion)
- **Vote NFT burn on counting** — ensure burned (removed from all outputs), not returned. This is the anti-double-vote mechanism.
- **Token-quantity state tracking** — proposal state is qty 1 (pending) → qty 2 (executed). Do not use register-based phase tracking.
- **Discussion window** — `timeValidator.es` must reject vote box creation before `discussionDeadline` height
- If sigma-rust 0.28 cannot compile a contract (nested lambda limitation), pivot to Scala AppKit test harness (same path as emission.es — see `01-emission-tests/HANDOFF.md`)

## 15 Threat Model & Social Attack Surface

This section catalogs external and social forces that can degrade or subvert the governance system, with mitigations labeled as **[in-contract]**, **[in-frontend]**, **[social]**, **[accepted]**, or **[V2]**.

### 15.1 Vote-duplication attempts

**Unlock-and-relock vote doubling.** Voter cancels vote during cooldown, receives vYOLO back, locks it again in a new vote box.
- Status: **prevented by design**
- Mechanism: cancelled votes are removed from tally before ever being counted. Cancel + revote is not "undo plus redo" — the original vote never entered the tally. Liquid vYOLO supply is fixed by deposits, so cancel-and-revote does not inflate voting power.

**Cancel → transfer → recipient votes.** Alice votes, cancels, sells vYOLO to Bob, Bob votes.
- Status: **known behavior, not a bug**
- Aggregate voting power does not grow; ownership of voting power shifts mid-cycle. Matches liquid democracy in every major DAO. Document in EDGE_CASES.md.

### 15.2 Economic attacks

**Whale concentration.** Single actor accumulates >50% of circulating vYOLO.
- Status: **inherent to stake-weighted governance**
- Mitigation: top-20 holder dashboard in frontend **[in-frontend]**, publish concentration metrics regularly **[social]**. Sunlight deters accumulation.

**Rental / long-window vote borrowing.** Actor rents vYOLO from holders to swing a vote.
- Status: **partially mitigated**
- V1 mitigation: locking period prevents flash-loan attacks **[in-contract]**.
- Unmitigated: week-long OTC rentals work fine. This is the Curve Wars pattern.
- V2 option: veYOLO (longer lock = more weight), reducing value of short-term rental **[V2]**.

**On-chain bribery contracts.** Third party deploys contract that pays YOLO to vYOLO holders who vote a specified way on a specified proposal.
- Status: **cannot be technically prevented**
- Legal in most jurisdictions, fully trustless once deployed. Social norms and holder self-policing are the only defense.
- Mitigation: **[social]** — establish community norms against bribe-taking; make bribe contracts visible if they emerge.

**Governance attack on the peg itself.** Majority passes proposal to drain vault YOLO or mint new vYOLO, breaking the peg that backs their own voting power.
- Status: **prevented by design**
- Mitigation: `vault.es` has **no governance-controlled spend path**. Treasury is a separate box from vaults. Proposals can only disburse *treasury* YOLO, never touch vault reserves, reserve boxes, or vYOLO supply **[in-contract]**.
- Add explicit comment to `vault.es` documenting this constitutional separation.

### 15.3 Participation attacks

**Low turnout → minority capture.** 95% of holders don't vote; 6% decides outcomes.
- Status: **standard DAO problem**
- Mitigation: quorum threshold (V1 default 10% of circulating vYOLO) **[in-contract]**. Too low enables capture; too high paralyzes governance. Tunable per-proposal within enforced bounds.

**Proposal spam.** Attacker floods proposals to exhaust voter attention.
- Status: **mitigated**
- Mitigation: initiation hurdle — proposer stakes max(1% circulating vYOLO, 100k vYOLO floor). Returned on lifecycle completion, forfeited on proposer-initiated cancel **[in-contract]**.
- The absolute floor matters most in early chain life; set it where spammers feel the cost but honest proposers don't.

**Surprise timing (holidays / distracted periods).** Proposals quietly submitted during low-attention windows.
- Status: **mitigated in V1 via discussion window**
- Mitigation: mandatory discussion window before voting opens — proposal exists in "pending" phase for N blocks during which voting is not yet open. V1 default: 2,880 blocks (~16 hours on 20s blocks) **[in-contract]**.
- Implementation: add `discussionDeadline` register to `proposal.es`; `userVote` box creation rejected until height ≥ `discussionDeadline`.

### 15.4 Metagovernance attacks

**Parameter drift.** Faction gains majority, lowers quorum to 5%, then ramrods remaining agenda under weaker rules.
- Status: **prevented in V1**
- Mitigation: V1 does **not** allow governance to change governance parameters. Quorum, thresholds, and windows are set at proposal creation with bounded ranges enforced in `proposal.es` **[in-contract]**.
- V2 extension: parameter changes require a separate proposal type with higher thresholds (suggested: 75% support, 30% quorum). Write the constraint into the V1 spec so V2 cannot silently remove it **[V2]**.

**Bounded parameter ranges.** Proposals cannot set absurd values.
- Implementation in `proposal.es`:
  - `quorumThreshold`: 500 ≤ basis points ≤ 5000 (5%–50%)
  - `supportBps`: 5000 ≤ value ≤ 10000 (50%–100%)
  - Voting window: 4,320 ≤ blocks ≤ 43,200 (1–10 days)
  - Discussion window: 2,880 ≤ blocks ≤ 17,280 (16 hours – 4 days)

**Trojan-horse proposals.** Proposal metadata displayed in frontend differs from on-chain execution data.
- Status: **mitigated via frontend discipline**
- Mitigation: frontend computes recipient + amount from proposal box R4/R5 directly, never from off-chain metadata or proposal description strings **[in-frontend]**.
- Add explicit proposal verification flow: user can paste a proposal box ID and see computed execution details independent of any description.

### 15.5 Infrastructure attacks

**Voting bot censorship.** Single bot operator refuses to process specific voter boxes.
- Status: **mitigated by open participation**
- Mitigation: anyone can run a bot; no whitelist; first valid TX wins mempool race. Subsequent attempts from other bots fail harmlessly on double-spend **[in-contract]** + **[social]**.
- Publish bot source, encourage multiple independent operators.

**Miner-level TX censorship.** Miner excludes counting TXs for specific voter boxes.
- Status: **partial mitigation**
- Mitigation: honest miners include the TX in subsequent blocks. Only fails if majority of hashrate colludes, at which point chain-level problems dominate.

**Frontend capture.** Single dominant frontend compromised; shows different proposal than what is on-chain.
- Status: **mitigated via openness**
- Mitigation: frontend is open-source, alternative UIs encouraged, canonical "verify proposal from box ID" flow published as a reference **[in-frontend]** + **[social]**.

**Node RPC centralization.** Users depend on one RPC provider; provider becomes a censor.
- Status: **mitigated via configurability**
- Mitigation: frontend supports user-configurable RPC endpoints; ship with a list of known nodes **[in-frontend]**.

### 15.6 Social / off-chain attacks

**Key-person capture.** Privileged role (admin, multisig signer) is targeted, coerced, or compromised.
- Status: **prevented in V1 by having no privileged roles**
- V1 has no admin keys, no pause mechanism, no upgrade authority, no emergency multisig.
- If any privileged role is added in V2, it must be documented explicitly in `PARAMETERS.md` with a time-locked revocation path and governance override **[V2]**.

**Coercion / real-world threats against large holders.** Whale is physically threatened to vote a specific way.
- Status: **inherent**
- Partial mitigation: high quorums on critical proposal types reduce any single holder's decisiveness **[in-contract]**.

**Fork threat as leverage.** Contentious proposal passes; minority faction threatens chain fork.
- Status: **feature, not bug** (exit as voice)
- Mitigation: V1 governance scope is deliberately narrow — treasury disbursement only. Tokenomics changes, emission changes, and other contentious proposals are out of scope. Save controversial territory for V2 with super-majority thresholds **[social]** + **[V2]**.

**Discord / Telegram misinformation.** Community is misled about proposal contents by bad actors.
- Status: **not technically solvable**
- Mitigation: canonical source of truth is on-chain proposal box, not forum summaries. Community norms around "read the actual proposal" **[social]**.
- Frontend's proposal verification flow (§15.4) is the technical assist to this social norm.

### 15.7 Crisis response

**Emergency response lag.** A bug is discovered; funds are at risk; normal voting takes days.
- Status: **V1 accepts this risk**
- V1 has no emergency powers, no pause mechanism, no admin override. If contracts have bugs, the community relies on contract correctness.
- Rationale: every emergency mechanism is a centralization vector. V1 prioritizes credibility of the "no privileged roles" claim.
- If V2 adds emergency powers, the proposal to do so must itself clear super-majority governance, forcing the decision to be explicit rather than smuggled in **[V2]**.

### 15.8 Inherent limitations (documented, not mitigated)

The following cannot be prevented by any technical means and should be accepted and disclosed:

- Stake-weighted governance concentrates power with capital
- On-chain bribery markets can emerge
- Voter apathy enables minority capture at the quorum floor
- Forks are an inherent exit option in open systems
- Real-world coercion is outside the threat model
- Markets will price vYOLO by their own logic, independent of design intent

The goal is not to prevent these — it is to make them **visible**, make them **expensive**, and ensure the system **degrades gracefully** rather than catastrophically when they occur.

### 15.9 Summary of V1 contract additions driven by this section

Consolidated list of spec additions to implement in Phase 2:

1. `vault.es`: add constitutional comment documenting no governance-controlled spend path
2. `proposal.es`: add `discussionDeadline` register (R9) enforcing discussion window before voting
3. `proposal.es`: enforce bounded ranges on quorum, support threshold, and window durations (see §15.4)
4. `timeValidator.es`: reject `userVote` box creation before discussion window ends
5. `PARAMETERS.md`: dedicated section documenting no admin keys, no pause mechanism, no privileged upgrade path, no emergency powers in V1 (governance migration via new-treasury-mode requires 90% super-majority)
6. `EDGE_CASES.md`: add "Social & Economic Attacks" section mapping this appendix to test coverage where applicable
7. Frontend spec (Phase 4): add top-N holder dashboard, proposal-from-box-id verification flow, RPC swap capability

### 15.10 Design philosophy

V1 governance is deliberately **boring and narrow**:

- Treasury disbursement only (proportion-based, inherently safe)
- No parameter changes (bounded ranges hardcoded in contracts)
- No emergency powers
- No privileged roles
- No privileged upgrade path — governance migration (new-treasury-mode) requires **90% super-majority vote**. No single actor, admin key, or multisig can trigger it.

This narrowness is the mitigation strategy. Every governance action V1 cannot perform is an attack surface V1 does not expose. V2 can expand scope deliberately, with each expansion gated by super-majority governance — forcing the community to explicitly accept each new attack surface rather than inheriting them by default.