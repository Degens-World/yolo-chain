# Handoff: Etchash Stratum Protocol Research

**Checklist Reference**: Phase 8.1, 8.2
**Owner**: You (research + spec writing), Rust dev (implementation)
**Blockers**: None — public documentation
**Dependencies**: None
**Deliverable**: Stratum integration spec document that a Rust dev can implement against

---

## Objective

Research and document exactly how the Etchash stratum mining protocol works so the Rust dev has a clear spec to implement. You're not writing the Rust code — you're writing the document they code from.

Etchash stratum is nearly identical to Ethash stratum. Both ETC and pre-merge ETH mining software use the same protocol. This is extremely well documented because millions of GPUs used it.

---

## Stratum Protocol Overview

Stratum is a JSON-RPC protocol over TCP. The miner connects to the pool (or solo node), receives work, and submits solutions.

### Core Message Flow

```
Miner → Pool:  mining.subscribe       (register)
Pool → Miner:  mining.subscribe OK    (session ID, extra nonce)
Miner → Pool:  mining.authorize       (worker name, password)
Pool → Miner:  mining.authorize OK
Pool → Miner:  mining.notify          (new work: header hash, seed hash, target)
Miner → Pool:  mining.submit          (solution: nonce, header hash, mix digest)
Pool → Miner:  mining.submit OK/FAIL
Pool → Miner:  mining.set_difficulty   (adjust difficulty target)
```

### Key Messages

**mining.notify** — Pool sends work to miner:
```json
{
  "id": null,
  "method": "mining.notify",
  "params": [
    "job_id",
    "header_hash",      // 32 bytes, keccak256 of block header without nonce
    "seed_hash",        // 32 bytes, determines which DAG epoch
    "boundary_target",  // 32 bytes, difficulty target
    "clean_jobs"        // boolean, true = discard previous work
  ]
}
```

**mining.submit** — Miner submits solution:
```json
{
  "id": 4,
  "method": "mining.submit",
  "params": [
    "worker_name",
    "job_id",
    "nonce"             // 8 bytes, the nonce that produces valid PoW
  ]
}
```

### Differences from Bitcoin Stratum

- No `extranonce` manipulation (Ethash/Etchash nonce is miner-controlled entirely)
- Header hash is pre-computed by the pool/node and sent to miner
- Miner returns only the nonce (and optionally mixHash for verification)
- DAG epoch changes require miner to regenerate DAG (takes minutes on GPU)

---

## Reference Implementations to Study

### Pool Side (What the Node Needs to Implement)

1. **open-ethereum-pool** (Go) — Most widely used open-source Ethash pool
   - GitHub: `sammy007/open-ethereum-pool`
   - Stratum implementation in `proxy/stratum.go`
   - Well-commented, clear message handling

2. **ethpool-core** — Simpler reference implementation
   - Useful for understanding minimal viable stratum server

3. **ETC pool implementations** — Same protocol, adapted for Etchash
   - Search for open-source ETC mining pools on GitHub

### Miner Side (What Miners Already Run — No Work Needed)

Existing mining software that supports Etchash stratum:
- **lolMiner** — popular, multi-algorithm
- **T-Rex** — NVIDIA focused
- **TeamRedMiner** — AMD focused
- **ethminer** — original Ethash miner, supports Etchash
- **NBMiner** — multi-platform

All of these connect to any standard Ethash/Etchash stratum endpoint. If the node implements the protocol correctly, these miners work out of the box. No miner software modifications needed.

---

## Integration Challenge: Ergo Block Structure

The tricky part is not the stratum protocol itself — it's the interface between stratum and the Ergo-style block construction.

In a standard Etchash chain (ETC), the pool:
1. Gets a pending block from the node (geth-style RPC: `eth_getWork`)
2. Extracts header hash, seed hash, target
3. Sends to miner via stratum
4. Receives nonce from miner
5. Submits completed block to node (`eth_submitWork`)

In the new chain, the node uses Ergo-style APIs:
1. Node constructs block candidate including coinbase TX (emission, storage rent claims)
2. Block header has different fields than Ethereum (includes votes, extension root, etc.)
3. The header hash sent to miners must be the Etchash-compatible hash of the header
4. When miner returns nonce, the node inserts it into the header and validates

**The spec must define**:
- How to derive the Etchash-compatible header hash from the Ergo-style block header
- Where the nonce and mixHash live in the modified block header (Phase 2.3 of checklist)
- How `eth_getWork`-equivalent RPC methods map to the new node's API
- How `eth_submitWork`-equivalent maps back

---

## What to Document

### Spec Document Structure

```
1. Protocol Overview
   - TCP connection parameters
   - JSON-RPC format
   - Session lifecycle

2. Message Definitions
   - mining.subscribe (request/response)
   - mining.authorize (request/response)
   - mining.notify (all fields, types, encoding)
   - mining.submit (all fields, types, encoding)
   - mining.set_difficulty (format, when sent)

3. Block Header Mapping
   - Ergo-style header fields → Etchash header hash input
   - Nonce and mixHash field locations in modified header
   - Seed hash derivation (DAG epoch from block height)

4. Node API Requirements
   - Endpoint to get current mining work
   - Endpoint to submit solution
   - Endpoint to get current difficulty
   - Notification mechanism for new blocks

5. DAG Management
   - Epoch length (blocks per DAG epoch)
   - DAG size progression (starting at 4GB)
   - How miners detect epoch changes

6. Error Handling
   - Stale share handling
   - Invalid nonce handling
   - Connection timeout behavior

7. Solo Mining Mode
   - How a single miner connects directly to their node
   - Simplified flow without pool share accounting
```

---

## What to Deliver

1. **etchash_stratum_spec.md** — Complete protocol specification as described above
2. **reference_code_notes.md** — Annotated notes from reading open-ethereum-pool stratum code
3. **header_mapping.md** — Detailed mapping of Ergo header fields to Etchash input (requires collaboration with Rust dev on header format)
4. **miner_compatibility_matrix.md** — List of mining software confirmed to work with standard Etchash stratum, with version numbers
