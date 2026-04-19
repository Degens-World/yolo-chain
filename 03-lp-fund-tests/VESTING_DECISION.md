# Vesting Decision: Option A — No Vesting

## Decision

The LP Fund contract does **not** implement on-chain vesting. All accumulated funds are available for spending immediately, subject to 2-of-3 multisig approval and whitelist enforcement.

## Rationale

The handoff doc presented three options:

| Option | Description | Verdict |
|--------|-------------|---------|
| A — No vesting | Fully available from day one | **Selected** |
| B — Linear vesting | 1/24th per month for 2 years | Rejected |
| C — Milestone vesting | Unlock on specific conditions | Rejected |

### Why Option A

1. **Existing constraints are sufficient.** The whitelist restricts funds to DEX LP pools, bridge liquidity, and market-making contracts only. The 2-of-3 multisig prevents unilateral action. These two mechanisms together prevent the "day-one dump into one pool" scenario that vesting was designed to address.

2. **Vesting adds complexity for marginal benefit.** On-chain vesting requires tracking `totalReceived` and `totalSpent` in registers, introduces rounding edge cases with integer division, and increases ErgoTree size and JIT cost. The LP fund's primary risk (misallocation) is already mitigated by the whitelist.

3. **Operational flexibility.** Early-stage liquidity needs are unpredictable. If a high-quality DEX pool or bridge opportunity appears in month 1, vesting would artificially constrain the response. The multisig signers are better positioned to make timing decisions than a rigid schedule.

4. **Consistency with treasury.** The treasury governance contract also has no vesting — both fund contracts rely on multisig governance rather than time-based restrictions.

## Mitigations Without Vesting

- **Whitelist enforcement**: funds cannot go to arbitrary addresses
- **2-of-3 multisig**: requires agreement of at least 2 signers
- **Freeze path**: emergency stop if signers detect misuse (permanent, nuclear option)
- **Migration path**: contract can be upgraded with timelock if vesting is later deemed necessary
