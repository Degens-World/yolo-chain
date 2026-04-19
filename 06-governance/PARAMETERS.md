# PARAMETERS.md — YOLO Governance Magic Numbers & Token IDs

All magic numbers, token IDs, and configuration values are recorded here
immediately upon introduction. This file is the single source of truth
for the voting bot, frontend, and all test code.

---

## Chain Parameters (from emission_model.py)

| Parameter | Value | Source |
|-----------|-------|--------|
| Block time | 20 seconds | SigmaChain spec |
| Blocks per year | 1,577,880 | 365.25 * 24 * 3600 / 20 |
| NANOCOIN | 1,000,000,000 | 1 coin = 10^9 nanocoins |
| Total YOLO supply | 177,412,882,500,000,000 nanocoins | emission_model.py |
| Total YOLO supply (coins) | 177,412,882.5 | emission_model.py |
| i64 headroom | 52x | TOTAL_SUPPLY vs i64::MAX |
| Decimals | 9 | Matching native YOLO |

## Peg Layer Parameters

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Vault/reserve pairs | 5 | Concurrency — 5 independent pairs |
| vYOLO per reserve (initial) | TOTAL_SUPPLY / 5 | Equal distribution |
| Minimum box value | 360,000 nanocoins | Matching storage rent spec |

## Voting Layer Parameters

| Parameter | SigmaChain blocks | Duration | Source |
|-----------|-------------------|----------|--------|
| Voting window (default) | 12,960 | 3 days | DuckDAO × 6 |
| Cancellation cooldown | 4,320 | 24 hours | DuckDAO × 6 |
| Counting phase | 1,080 | 6 hours | DuckDAO × 6 |
| Execution grace period | 4,320 | 24 hours | DuckDAO × 6 |
| Discussion window (default) | 2,880 | 16 hours | §15.3 threat model |

### Bounded ranges (enforced in proposal.es)

| Parameter | Min | Max | Unit |
|-----------|-----|-----|------|
| Quorum threshold | 500 | 5000 | basis points |
| Support threshold | 5000 | 10000 | basis points |
| Voting window | 4,320 | 43,200 | blocks (1-10 days) |
| Discussion window | 2,880 | 17,280 | blocks (16h-4 days) |

### Tiered support thresholds

| Condition | Required support |
|-----------|------------------|
| proportion ≤ 1,000,000 (≤10% of treasury) | 50% (5000 bps) |
| proportion > 1,000,000 (>10% of treasury) | 90% (9000 bps) |
| proportion = 10,000,000 (new-treasury-mode) | 90% (auto, since >10%) |

### Other governance constants

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Proportion denominator | 10,000,000 | DuckDAO convention |
| Quorum floor | 1,000,000 vYOLO | ~0.56% of total supply |
| Initiation hurdle | max(1% circulating, 100,000 vYOLO) | Anti-spam |
| Forfeit destination | Treasury box | Forfeited proposer stakes |

## Administrative Properties

- No admin keys
- No pause mechanism
- No privileged upgrade path (governance migration via new-treasury-mode requires 90% super-majority)
- No emergency powers

## Token IDs

> Populated during genesis deployment. Each ID is recorded immediately after minting.

### State NFTs (vault markers)

| Pair | Token Name | Token ID |
|------|-----------|----------|
| 1 | YOLO_VAULT_STATE_NFT_1 | _TBD_ |
| 2 | YOLO_VAULT_STATE_NFT_2 | _TBD_ |
| 3 | YOLO_VAULT_STATE_NFT_3 | _TBD_ |
| 4 | YOLO_VAULT_STATE_NFT_4 | _TBD_ |
| 5 | YOLO_VAULT_STATE_NFT_5 | _TBD_ |

### Reserve NFTs (reserve markers)

| Pair | Token Name | Token ID |
|------|-----------|----------|
| 1 | YOLO_VAULT_RESERVE_NFT_1 | _TBD_ |
| 2 | YOLO_VAULT_RESERVE_NFT_2 | _TBD_ |
| 3 | YOLO_VAULT_RESERVE_NFT_3 | _TBD_ |
| 4 | YOLO_VAULT_RESERVE_NFT_4 | _TBD_ |
| 5 | YOLO_VAULT_RESERVE_NFT_5 | _TBD_ |

### vYOLO Token

| Token Name | Token ID | Total Supply |
|-----------|----------|--------------|
| vYOLO | _TBD_ | 177,412,882,500,000,000 nanocoins |

### Governance Tokens

| Token Name | Token ID | Purpose |
|-----------|----------|---------|
| Treasury NFT | _TBD_ | Authenticates treasury box |
| Counter NFT | _TBD_ | Authenticates counting box |
| Valid Vote NFT | _TBD_ | Authenticates voter boxes (burned on counting) |

## Script Hashes

> Populated after contract compilation with token IDs baked in.

| Contract | Script Hash | Address |
|----------|-------------|---------|
| vault.es (pair 1) | _TBD_ | _TBD_ |
| vault.es (pair 2) | _TBD_ | _TBD_ |
| vault.es (pair 3) | _TBD_ | _TBD_ |
| vault.es (pair 4) | _TBD_ | _TBD_ |
| vault.es (pair 5) | _TBD_ | _TBD_ |
| reserve.es (pair 1) | _TBD_ | _TBD_ |
| reserve.es (pair 2) | _TBD_ | _TBD_ |
| reserve.es (pair 3) | _TBD_ | _TBD_ |
| reserve.es (pair 4) | _TBD_ | _TBD_ |
| reserve.es (pair 5) | _TBD_ | _TBD_ |
| treasury.es | _TBD_ | _TBD_ |
| counting.es | _TBD_ | _TBD_ |
| proposal.es | _TBD_ | _TBD_ |
| userVote.es | _TBD_ | _TBD_ |
| timeValidator.es | _TBD_ | _TBD_ |

## Gas / JitCost Measurements

> Populated during testing. Document per TX type.

| TX Type | Estimated JitCost | Notes |
|---------|-------------------|-------|
| Deposit (vault + reserve) | _TBD_ | |
| Redeem (vault + reserve) | _TBD_ | |
| Create proposal | _TBD_ | |
| Cast vote | _TBD_ | |
| Count vote | _TBD_ | |
| Execute proposal | _TBD_ | |
