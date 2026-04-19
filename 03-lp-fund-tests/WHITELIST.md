# LP Fund Whitelist — Initial Entries

## Overview

The LP Fund governance contract stores a whitelist in R4 as `Coll[Coll[Byte]]` — a collection of 32-byte blake2b256 hashes of allowed destination ErgoTree scripts. Funds can only be sent to contracts whose `blake2b256(propositionBytes)` matches an entry in this whitelist.

New entries can be added via the Whitelist Update path (2-of-3 multisig, add-only, superset enforced). Entries cannot be removed — migration to a new contract is required to shrink the whitelist.

## Initial Whitelist (at launch)

The initial whitelist should be populated once the target DeFi contracts are deployed on YoloChain. Until then, the entries below are **placeholders** — the actual blake2b256 hashes will be computed from the deployed ErgoTree bytes.

| # | Destination | Rationale | Hash |
|---|-------------|-----------|------|
| 1 | Spectrum-style AMM Pool | Primary DEX liquidity for the native token. Seeding LP here enables price discovery and trading from day one. | TBD — compute from deployed pool contract ErgoTree |
| 2 | AEther Bridge Lock | Cross-chain liquidity for bridged assets. Enables the native token to be traded on Ergo mainnet and other chains. | TBD — compute from deployed bridge lock contract ErgoTree |
| 3 | Staging/Hold Contract | Simple P2S contract for temporarily holding funds before deployment to a specific pool. Useful when a pool isn't ready yet but funds need to leave the governance box. | TBD — deploy a simple hold contract and compute hash |

## How to Compute a Hash

Given a deployed contract's ErgoTree hex (from `/script/addressToTree/{addr}`):

```bash
python3 -c "
import hashlib
tree_hex = 'ERGOTREE_HEX_HERE'
tree_bytes = bytes.fromhex(tree_hex)
h = hashlib.blake2b(tree_bytes, digest_size=32)
print(h.hexdigest())
"
```

Or in Rust (used in the test suite):

```rust
use blake2::{digest::consts::U32, Blake2b, Digest};
type Blake2b256 = Blake2b<U32>;

fn proposition_hash(tree: &ErgoTree) -> Vec<u8> {
    let bytes = tree.sigma_serialize_bytes().expect("serialize tree");
    let mut h = Blake2b256::new();
    h.update(&bytes);
    h.finalize().to_vec()
}
```

## Adding Entries Post-Launch

To add a new destination via the Whitelist Update path:

1. Deploy the new destination contract on-chain
2. Compute its `blake2b256(propositionBytes)` hash
3. Construct a transaction with:
   - Input: current LP Fund governance box
   - Output: same box with R4 containing the old whitelist + the new hash
   - R5, R6 preserved, value preserved, NFT preserved
4. Sign with 2-of-3 multisig

The contract enforces that the new whitelist is a strict superset (all old entries present, at least one new entry added).

## Deployment Notes

The LP Fund governance box must be initialized with:
- `R4`: the initial whitelist (vector of 32-byte hashes)
- `R5`: `0L` (normal state)
- `R6`: `0L` (no migration pending)
- `tokens(0)`: singleton NFT (amount = 1)

The accumulation contract has the LP Fund governance script hash baked in:
`f7e52722204eab03cf13bea0772e0e00ee48f4a6f81396514d9a3b692d61b1e4`
