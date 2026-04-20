# Governance System — Handoff for Phase 4/5/6

**From:** Phase 1-3 implementation (contracts, tests, bot, models)
**Status:** 60/60 tests green, 7 contracts audited (8.5-9/10), full lifecycle proven
**Branch:** `06-governance-peg-layer`

---

## What's Built

| Component | File | Audit | Tests |
|-----------|------|-------|-------|
| Vault (peg layer) | `contracts/vault.es` | 9/10 | 9 |
| Reserve (peg layer) | `contracts/reserve.es` | 9/10 | 9 + 5 twin |
| Treasury (governance-controlled) | `contracts/treasury.es` v1.2 | 9/10 | 12 |
| Counting (4-phase state machine) | `contracts/counting.es` v1.1 | 8.5/10 | 10 |
| Proposal (state token lifecycle) | `contracts/proposal.es` v1.1 | 9/10 | 8 |
| User Vote (cancel/submit) | `contracts/userVote.es` v1.1 | 9/10 | 6 |
| Time Validator (height gate) | `contracts/timeValidator.es` v1.0 | 8/10 | — |
| Voting Bot | `bot/voting_bot.py` | — | structural |
| Peg Model | `models/vault_model.py` | — | runs clean |
| Voting Model | `models/voting_model.py` | — | runs clean |
| Integration Test | `tests/integration_test.rs` | — | 1 lifecycle |

## Known Issues to Address Before Deployment

1. **Counting.es Phase 1 re-entry after first round.** After Phase 4 resets tallies, `isBeforeCounting` (HEIGHT < voteDeadline) is permanently false because the old deadline is in the past. The genesis counter box must be created with a far-future deadline. Subsequent rounds work if Phase 4 or a bot helper updates R4. Document in the deployment script.

2. **Bot TX builders are stubs.** The state machine logic, threshold calculations, and phase detection are implemented. The actual TX construction + node signing calls need to be wired when a SigmaChain node is available. Use the node's `/wallet/transaction/sign` endpoint with `inputsRaw` (see `01-emission-tests/HANDOFF.md` for the pattern).

3. **Eager ValDef pattern.** The Scala compiler hoists shared register reads across OR branches. Tests must provide dummy data inputs / voter inputs / outputs for paths not being tested. This is documented in `01-emission-tests/SKILL-rust-test-harness.md` (Issues 4 and 7). Treasury.es v1.2 already handles this with `if(hasProposalToken)` guard.

4. **sigma-rust 0.28 cannot compile these contracts.** All ErgoTrees were compiled via the Ergo node's `/script/p2sAddress` + `/script/addressToTree` endpoints. Compiled hex is stored in `/tmp/*_tree.hex` during the test session. For CI, either embed the hex as constants or add a compilation step to the build pipeline.

---

## Phase 4: Frontend

**React app, reuses patterns from Etcha V3 wizard.**

Screens needed:
- **Lock/Redeem** — vault pair selector, deposit YOLO / redeem vYOLO
- **Proposal List** — active proposals with status, proportion, recipient, vote counts
- **Proposal Detail** — vote breakdown, threshold meter (50%/90%), timeline
- **Vote** — lock vYOLO, pick yes/no, creates userVote box via timeValidator
- **Cancel Vote** — reclaim vYOLO (requires wallet signature)
- **Create Proposal** — wizard: proportion slider, recipient address, discussion window

Key integration points:
- Read counter box state for phase detection (same logic as `voting_bot.py`)
- Read vault/reserve boxes for available vYOLO and locked YOLO
- TX construction: use Fleet SDK (TypeScript) or ergo-lib-wasm
- Vault routing: pick from 5 pairs, retry on mempool collision
- Proposal verification: compute recipient + awarded from box R4/R5, never from off-chain metadata

Security requirements (from §15 threat model):
- Top-20 vYOLO holder dashboard
- Proposal-from-box-ID verification flow
- User-configurable RPC endpoints

---

## Phase 5: Testnet Deployment

**Prerequisite:** SigmaChain testnet node running.

Deployment sequence (strict ordering):
1. `source .secrets` for API_KEY and wallet mnemonic
2. Run `scripts/genesis/01_mint_state_nfts.rs` — mint 5 state NFTs, record IDs in `PARAMETERS.md`
3. Run `02_mint_reserve_nfts.rs` — mint 5 reserve NFTs, record IDs
4. Run `03_mint_vYolo.rs` — mint vYOLO at emission cap (177,412,882,500,000,000 nanocoins), record ID
5. Compile all contracts with real token IDs baked in (replace `*_PLACEHOLDER` constants)
6. Run `04_deploy_vaults.rs` — create 5 vault boxes (state NFT + 0 YOLO)
7. Run `05_deploy_reserves.rs` — create 5 reserve boxes (reserve NFT + vYOLO cap/5)
8. Deploy counting box (counter NFT, R4=far-future deadline, R5-R9=zeros)
9. Deploy treasury.es box (treasury NFT, dormant — activated by multisig migration later)
10. Record ALL box IDs, script hashes, and addresses in `PARAMETERS.md`
11. Configure `bot/config.yaml` with deployed token IDs
12. Start voting bot: `python voting_bot.py --config config.yaml`

Testing on testnet:
- 3 users deposit various YOLO amounts across different vault pairs
- User creates proposal (5% of treasury)
- 3 users vote (2 yes, 1 no)
- Bot counts votes, validates, advances proposal
- Execute treasury withdrawal
- Users redeem vYOLO
- Verify peg invariant
- Stress: concurrent deposits across all 5 pairs
- Stress: bot restart mid-counting (stateless recovery)

---

## Phase 6: Mainnet

**Prerequisite:** Testnet sign-off.

Mainnet-specific checklist:
- [ ] All token IDs in `PARAMETERS.md` are mainnet values
- [ ] Wallet mnemonic is mainnet wallet (NOT testnet)
- [ ] Bot config points to mainnet node
- [ ] Genesis scripts use mainnet fee policy
- [ ] treasury.es deployed dormant (multisig migrates later)
- [ ] Multiple independent bot operators confirmed
- [ ] Frontend deployed with mainnet RPC endpoints
- [ ] Community announcement: governance is live, how to lock/vote

Migration sequence (multisig → governance):
1. Governance system runs alongside multisig for observation period
2. Community confidence established via successful proposal cycles
3. Governance votes to migrate treasury (proportion=10,000,000, new-treasury-mode)
4. 90% super-majority required (elevated threshold for >10% of treasury)
5. Multisig signers review the passed on-chain vote
6. Multisig executes migration path (treasury_governance.es Path 6+7)
7. Governance NFT transfers to treasury.es — governance-controlled from this point
8. Same process available for LP fund migration

---

## File Map

```
06-governance/
├── contracts/          # 7 ErgoScript contracts
├── tests/              # 10 Rust test files (60 tests)
├── models/             # 2 Python reference models
├── bot/                # Voting bot + config
├── Cargo.toml          # ergo-lib 0.28, sigma-test-util
├── PARAMETERS.md       # All magic numbers, token ID placeholders
└── EDGE_CASES.md       # 17 attack vectors + mitigations
```
