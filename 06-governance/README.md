# YoloDAO — Native Governance for SigmaChain

YoloDAO is an on-chain governance system for SigmaChain, enabling YOLO holders to vote on treasury spending proposals. It adapts the battle-tested [DuckDAO](https://github.com/duckpools/lend-protocol-contracts) governance system from Ergo mainnet to work with YOLO as a native coin.

## How It Works

### The Problem

YOLO is SigmaChain's native coin (like ERG on Ergo). ErgoScript contracts can't directly read native coin balances across arbitrary UTXOs, so a proxy token is needed for governance.

### The Solution: vYOLO

1. **Lock YOLO, get vYOLO.** Users deposit YOLO into a vault contract and receive vYOLO (a 1:1 peg-backed proxy token) from a paired reserve contract.
2. **Vote with vYOLO.** Lock vYOLO in a voter box to vote yes or no on treasury spending proposals.
3. **Proposals pass or fail on-chain.** A counting contract tallies votes, checks quorum and support thresholds, and advances passed proposals.
4. **Treasury disburses proportionally.** Passed proposals authorize the treasury to send a percentage of its YOLO to a specified recipient.
5. **Redeem anytime.** Return vYOLO to get your YOLO back. The peg is structurally enforced — vaults have no governance-controlled spend path.

### Key Properties

- **No admin keys.** No multisig, no pause button, no emergency powers. All spending is controlled by on-chain votes.
- **Proportion-based.** Proposals specify a fraction of the treasury (e.g., 5%), not a fixed amount. Can never propose more than exists.
- **Flash-loan resistant.** vYOLO must be locked for the full voting window.
- **Anti-spam.** Proposers stake 100,000+ vYOLO. Stake returns on completion, forfeited to treasury on cancellation.
- **Tiered thresholds.** Normal proposals need 50% support. Large proposals (>10% of treasury) need 90%.
- **Upgradeable via governance.** A 90% super-majority vote can migrate the entire treasury to a new contract. No hard fork needed.

## Architecture

```
User deposits YOLO          User votes with vYOLO        Treasury disburses
     │                            │                            │
     ▼                            ▼                            ▼
┌─────────┐  1:1 peg  ┌──────────────┐  counting  ┌───────────────┐
│ Vault   │◄─────────►│   Reserve    │            │   Counting    │
│ (YOLO)  │           │   (vYOLO)    │            │  (4-phase SM) │
└─────────┘           └──────────────┘            └───────┬───────┘
  × 5 pairs             × 5 pairs                        │
                                                    pass/fail
                                                         │
                                              ┌──────────▼──────────┐
                                              │     Proposal        │
                                              │  (state token 1→2)  │
                                              └──────────┬──────────┘
                                                         │ token qty=2
                                              ┌──────────▼──────────┐
                                              │     Treasury        │
                                              │ (proportional send) │
                                              └─────────────────────┘
```

## Proposal Lifecycle

1. **Discussion** — Proposal exists on-chain for 16+ hours before voting opens. Community reviews.
2. **Voting** — vYOLO holders lock tokens in voter boxes (1-10 day window). Yes or no.
3. **Counting** — Bot processes voter boxes one at a time. Vote NFTs are burned to prevent double-counting.
4. **Validation** — Quorum checked (1M+ vYOLO participation). Support threshold checked (50% or 90%).
5. **Execution** — If passed, proposal state token advances (qty 1→2). Treasury recognizes this as authorization to disburse.
6. **Reset** — Counter clears for next proposal.

## Contracts

| Contract | Purpose | Size |
|----------|---------|------|
| `vault.es` | Holds locked YOLO + singleton NFT | 393 bytes |
| `reserve.es` | Holds vYOLO + singleton NFT | 467 bytes |
| `treasury.es` | Governance-controlled treasury (proportional withdrawal, split-math) | 496 bytes |
| `counting.es` | 4-phase vote counting state machine | 957 bytes |
| `proposal.es` | Proposal lifecycle (state token qty 1→2) | 268 bytes |
| `userVote.es` | Per-voter box (cancel with sig, submit with NFT burn) | 423 bytes |
| `timeValidator.es` | Gates vote creation by height window | 357 bytes |

## Deployment Model

At genesis, the existing 2-of-3 multisig treasury (`treasury_governance.es`) manages funds. YoloDAO deploys alongside as an on-chain approval layer:

1. **Phase 1 (launch):** Community votes on proposals. Multisig signers review on-chain results and execute.
2. **Phase 2 (migration):** Governance votes to migrate treasury to `treasury.es`. Multisig executes the migration. From this point, all spending is governance-controlled.

This two-step approach lets the community observe governance working before the multisig hands over control.

## Testing

60 tests across 10 files, all passing in <1 second:

```
cargo test
```

Tests cover:
- Peg layer: deposit, redeem, conservation violations, token contamination, script replacement
- Treasury: deposit, proportional withdrawal (split-math), new-treasury-mode, overclaim rejection
- Counting: phase transitions, vote NFT burn, re-initiation guard, timing
- Proposal: state advancement, token burn (nested forall), immutability
- User vote: submit with burn, deadline enforcement, vYOLO theft protection
- Integration: full lifecycle deposit → withdraw → execute → redeem with peg invariant

## Voting Bot

A stateless Python bot (`bot/voting_bot.py`) monitors the chain and advances proposals through their lifecycle. Anyone can run one — first valid TX wins the mempool race, others fail harmlessly.

```
python bot/voting_bot.py --config bot/config.yaml
```

## Key Numbers

| Parameter | Value |
|-----------|-------|
| Total YOLO supply | 177,412,882.5 coins |
| vYOLO decimals | 9 (matches YOLO) |
| Vault pairs | 5 (concurrent) |
| Voting window | 1-10 days (default 3) |
| Discussion window | 16h-4 days (default 16h) |
| Quorum floor | 1,000,000 vYOLO |
| Normal support | 50% |
| Elevated support (>10% treasury) | 90% |
| Initiation hurdle | 100,000 vYOLO (forfeited on cancel) |
| Proportion denominator | 10,000,000 |
