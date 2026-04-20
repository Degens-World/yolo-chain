# EDGE_CASES.md — Attack Vectors, Mitigations & Test Coverage

Each edge case is documented here as it is identified during development.
Every entry maps to at least one test case.

---

## Peg Layer

### EC-01: Token contamination in vault box
- **Attack**: Attacker adds extra tokens to vault box output
- **Mitigation**: `tokens.size == 1` check in vault.es
- **Test**: `vault_test.rs::test_reject_extra_tokens`

### EC-02: Token contamination in reserve box
- **Attack**: Attacker adds extra tokens to reserve box output
- **Mitigation**: `tokens.size == 2` check in reserve.es
- **Test**: `reserve_test.rs::test_reject_extra_tokens`

### EC-03: Conservation violation (deposit more vYOLO than YOLO deposited)
- **Attack**: Get more vYOLO out of reserve than YOLO deposited into vault
- **Mitigation**: `deltaVaultYolo + deltaReserveVYolo == 0` in both contracts
- **Test**: `twin_pair_test.rs::test_conservation_violation`

### EC-04: No-op transaction
- **Attack**: Submit a TX that changes nothing (waste chain resources)
- **Mitigation**: `nonTrivial` check: `deltaVaultYolo != 0`
- **Test**: `vault_test.rs::test_reject_noop`

### EC-05: Multi-vault merge attack
- **Attack**: Consume two vault boxes in one TX to merge them
- **Mitigation**: `singleVaultInput` / `singleReserveInput` checks (filter.size == 1)
- **Test**: `vault_test.rs::test_reject_multi_vault_input`

### EC-06: Vault script replacement
- **Attack**: Replace vault contract script in successor output
- **Mitigation**: `out.propositionBytes == SELF.propositionBytes`
- **Test**: `vault_test.rs::test_reject_script_change`

### EC-07: NFT theft from vault
- **Attack**: Move state NFT to a different box
- **Mitigation**: `out.tokens(0)._1 == StateNftId && out.tokens(0)._2 == 1L`
- **Test**: `vault_test.rs::test_reject_nft_theft`

### EC-08: Cross-pair manipulation
- **Attack**: Pair vault_1 with reserve_2 (wrong pairing)
- **Mitigation**: NFT IDs are compile-time constants unique per pair. Vault_1 only recognizes Reserve_NFT_1.
- **Test**: `twin_pair_test.rs::test_reject_cross_pair`

### EC-09: Reserve ERG drain
- **Attack**: Reduce reserve box's ERG value (drain minimum box value)
- **Mitigation**: `out.value == SELF.value` in reserve.es
- **Test**: `reserve_test.rs::test_reject_erg_drain`

### EC-10: Over-redeem (redeem more than vault holds)
- **Attack**: Redeem 100 vYOLO when vault only holds 50 YOLO
- **Mitigation**: Conservation check forces vault output value = vault input - amount. If vault input < amount, output would be negative → rejected by protocol (box value must be ≥ 0).
- **Test**: `twin_pair_test.rs::test_reject_over_redeem`

### EC-11: Storage rent peg drift
- **Attack**: Not an attack — natural storage rent collection reduces vault YOLO
- **Impact**: Bounded drift of ~0.078 coins per vault per year (5 vaults × 0.078 = 0.39 coins/year max)
- **Mitigation**: Voting bot heartbeat resets rent clock; operational, not contractual
- **Test**: Documented only — cannot be tested in contract unit tests

### EC-12: Governance attack on peg (drain vaults via proposal)
- **Attack**: Pass a governance proposal that drains vault YOLO
- **Mitigation**: CONSTITUTIONAL — vault.es has NO governance-controlled spend path. Proposals can only disburse from treasury box. Vaults and treasury are structurally separate.
- **Test**: Integration test verifies no proposal TX shape can satisfy vault.es

---

## Voting Layer

> Populated during Phase 2 development.

### EC-20: Proportion math i64 overflow
- **Attack**: Treasury value × proportion exceeds i64 max
- **Mitigation**: Split-math pattern in treasury.es (§7.8 of implementation plan)
- **Test**: `treasury_test.rs::test_overflow_protection`

### EC-21: Vote double-counting
- **Attack**: Submit same voter box to counting twice
- **Mitigation**: Vote NFT burned during counting (removed from all outputs)
- **Test**: `voting_adversarial_test.rs::test_double_count`

### EC-22: Flash-loan voting
- **Attack**: Borrow vYOLO, vote, return within same block
- **Mitigation**: vYOLO locked in voter box for full voting window
- **Test**: `voting_adversarial_test.rs::test_flash_loan_vote`

### EC-23: Surprise proposal timing
- **Attack**: Submit proposal during low-attention window
- **Mitigation**: Mandatory discussion window (R9: discussionDeadline) before voting opens
- **Test**: `voting_happy_path_test.rs::test_discussion_window`

### EC-24: Parameter drift via governance
- **Attack**: Lower quorum via governance, then pass weak proposals
- **Mitigation**: V1 does NOT allow governance to change parameters. Bounded ranges hardcoded.
- **Test**: `voting_adversarial_test.rs::test_parameter_bounds`

---

## Social & Economic Attacks

See §15 of `11-YOLO_GOVERNANCE_IMPLEMENTATION_PLAN.md` for the full threat model.
These cannot be tested in contract unit tests but are documented for completeness:

- Whale concentration (§15.2) — inherent to stake-weighted governance
- Vote borrowing / Curve Wars (§15.2) — partially mitigated by locking period
- On-chain bribery (§15.2) — cannot be prevented technically
- Low turnout minority capture (§15.3) — mitigated by quorum floor
- Proposal spam (§15.3) — mitigated by initiation hurdle + forfeit
- Voting bot censorship (§15.5) — mitigated by open participation
- Frontend capture (§15.5) — mitigated by open-source + verification flow
