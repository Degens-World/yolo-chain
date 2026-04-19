---
title: "Rust Test Harness for ErgoScript Contracts"
description: "Test ErgoScript contracts in Rust by compiling via the Ergo node API and evaluating with ergo-lib's prove/verify. No Scala toolchain required. Covers project setup, ErgoTree compilation, Context construction, prove/verify patterns, and known gotchas."
tags: [rust, sigma-rust, ergo-lib, testing, ergoscript, harness, prove, verify]
category: skill
difficulty: advanced
sdk: [sigma-rust]
updated: 2026-04-20
source: original
author: CannonQ
license: AGPL-3.0
---
# Testing ErgoScript Contracts in Rust

## Context

sigma-rust's `ergoscript-compiler` crate supports only a subset of ErgoScript syntax. Complex contracts — nested lambdas, typed `forall`, advanced tuple patterns — often fail to compile. But **sigma-rust's evaluator (ergotree-interpreter) supports the full opcode set.** The compilation and evaluation stages are independent.

The solution: compile ErgoScript via the Ergo node's API (which uses the full Scala sigmastate-interpreter), capture the serialized ErgoTree bytes, then load and evaluate them in Rust with `ergo-lib` 0.28+.

This pattern was proven on two YoloChain contracts:
- **Emission contract v1.1** (454-byte ErgoTree, nested `forall` with typed-tuple lambda, 19 tests covering normal path, halving boundaries, terminal drain, and NFT burn verification) — `sigmaProp(boolean)` only, no signatures needed.
- **Treasury governance contract v1.1** (642-byte ErgoTree, `atLeast(2, signers) && sigmaProp(boolean)`, 45 tests covering 7 spending paths: approval, execution, cancellation, consolidation, freeze, migration approval, migration execute) — requires `DlogProverInput` secrets for threshold signature proofs.

## Architecture

```
┌─────────────────┐     HTTP POST       ┌──────────────┐
│  emission.es    │ ───────────────────> │  Ergo Node   │
│  (ErgoScript)   │  /script/p2sAddress  │  6.x (Scala) │
└─────────────────┘     + treeVersion:0  └──────┬───────┘
                                                │
                         P2S address            │
                         ◄──────────────────────┘
                                                │
                         GET /script/           │
                         addressToTree/{addr}   │
                                                │
                         ErgoTree hex           │
                         ◄──────────────────────┘
                                                │
┌─────────────────┐                             │
│  Rust tests     │  ErgoTree::sigma_parse_bytes()
│  (cargo test)   │  ◄─────────────────────────┘
│  prove/verify   │
└─────────────────┘
```

Key insight: the `compiler` feature on `ergo-lib` is not needed. The node does the compilation; Rust only deserializes and evaluates.

## Step 1: Compile ErgoScript via Node API

The Ergo node's `/script/compile` endpoint is broken on some versions (6.1.2 returns `MethodRejection`). Use the two-step workaround:

```bash
# Step 1a: Compile to P2S address
# IMPORTANT: treeVersion is required — omitting it returns 400
curl -s -X POST "http://localhost:9053/script/p2sAddress" \
  -H "Content-Type: application/json" \
  -H "api_key: $API_KEY" \
  -d '{"source": "<ERGOSCRIPT_SOURCE>", "treeVersion": 0}'
# Returns: { "address": "3xwXZo..." }

# Step 1b: Convert address to raw ErgoTree hex
curl -s "http://localhost:9053/script/addressToTree/<ADDRESS>"
# Returns: { "tree": "10200480..." }
```

For multi-line contracts, use Python/jq to build the JSON payload from a file:

```bash
python3 -c "
import json, sys
with open('contract.es') as f:
    source = f.read()
print(json.dumps({'source': source, 'treeVersion': 0}))
" > /tmp/compile.json

ADDR=$(curl -s -X POST "http://localhost:9053/script/p2sAddress" \
  -H "Content-Type: application/json" \
  -H "api_key: $API_KEY" \
  -d @/tmp/compile.json | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])")

curl -s "http://localhost:9053/script/addressToTree/$ADDR" \
  | python3 -c "import sys,json; print(json.load(sys.stdin)['tree'])"
```

## Step 2: Project Setup

### Cargo.toml

```toml
[package]
name = "contract_tests"
version = "0.1.0"
edition = "2021"
rust-version = "1.85"

[dependencies]
ergo-lib = { version = "0.28", features = ["arbitrary"] }
blake2 = "0.10"          # only if contract uses blake2b256

[dev-dependencies]
sigma-test-util = { git = "https://github.com/ergoplatform/sigma-rust", tag = "ergo-lib-v0.28.0" }
```

Notes:
- **No `compiler` feature** — compilation is done by the node
- **`arbitrary` feature** is required for `force_any_val::<PreHeader>()` and `force_any_val::<[Header; 10]>()` which construct valid-shape Context fields without manually building them
- **`sigma-test-util`** provides `force_any_val` — pulled from the sigma-rust repo as a git dependency

### Key Imports

```rust
use ergo_lib::ergo_chain_types::{Digest32, Header, PreHeader};
use ergo_lib::ergotree_interpreter::{
    eval::context::Context,
    sigma_protocol::{
        prover::{hint::HintsBag, ContextExtension, Prover, TestProver},
        verifier::{TestVerifier, Verifier},
    },
};
use ergo_lib::ergotree_ir::{
    chain::{
        ergo_box::{
            box_value::BoxValue, ErgoBox, ErgoBoxCandidate, NonMandatoryRegisterId,
            NonMandatoryRegisters,
        },
        token::{Token, TokenAmount, TokenId},
        tx_id::TxId,
    },
    ergo_tree::ErgoTree,
    mir::constant::Constant,
    serialization::SigmaSerializable,
};
use sigma_test_util::force_any_val;
```

## Step 3: Load Pre-Compiled ErgoTree

```rust
const CONTRACT_TREE_HEX: &str = "10200480..."; // from Step 1

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn load_tree(hex: &str) -> ErgoTree {
    let bytes = hex_to_bytes(hex);
    ErgoTree::sigma_parse_bytes(&bytes).expect("valid ErgoTree")
}
```

**CRITICAL: Always verify the hex with a round-trip test:**

```rust
#[test]
fn round_trip_serialization_matches() {
    let tree = load_tree(CONTRACT_TREE_HEX);
    let bytes = tree.sigma_serialize_bytes().unwrap();
    let original = hex_to_bytes(CONTRACT_TREE_HEX);
    assert_eq!(bytes, original, "ErgoTree round-trip mismatch — hex is corrupt");
}

#[test]
fn proposition_parses() {
    let tree = load_tree(CONTRACT_TREE_HEX);
    tree.proposition().expect("proposition should parse");
}
```

The round-trip test catches hex truncation/corruption. The proposition test catches deserialization issues. Run these first before writing evaluation tests.

## Step 4: Build Context

```rust
fn build_context(self_box: ErgoBox, outputs: Vec<ErgoBox>, height: u32) -> Context<'static> {
    let self_ref: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outputs_static: &'static [ErgoBox] = Vec::leak(outputs);
    let inputs_arr: [&'static ErgoBox; 1] = [self_ref];
    let inputs = inputs_arr.into();

    // Contract doesn't read PreHeader/Headers — use arbitrary valid-shape values
    let pre_header = force_any_val::<PreHeader>();
    let headers = force_any_val::<[Header; 10]>();

    Context {
        height,
        self_box: self_ref,
        outputs: outputs_static,
        data_inputs: None,
        inputs,
        pre_header,
        headers,
        extension: ContextExtension::empty(),
    }
}
```

Notes:
- `Box::leak` / `Vec::leak` create `'static` references required by `Context`. Acceptable in tests — the OS reclaims memory when the test process exits.
- `inputs` must contain `self_box` as the first input (mirrors real transaction structure).
- If your contract reads `CONTEXT.dataInputs`, provide them via `data_inputs: Some(...)`.
- If your contract uses `getVar[T](n)`, populate `extension: ContextExtension { values: ... }`.
- `height` field directly controls the `HEIGHT` global in ErgoScript evaluation.

## Step 5: Prove / Verify Pattern

```rust
fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    let prover = TestProver { secrets: vec![] };
    let message = vec![0u8; 32];

    let proof = match prover.prove(tree, ctx, message.as_slice(), &HintsBag::empty()) {
        Ok(p) => p.proof,
        Err(_) => return false,
    };

    let verifier = TestVerifier;
    match verifier.verify(tree, ctx, proof, message.as_slice()) {
        Ok(v) => v.result,
        Err(_) => false,
    }
}
```

- `TestProver { secrets: vec![] }` — no private keys needed for `sigmaProp(boolean)` contracts. The prover reduces the tree to `TrivialProp(true)` or `TrivialProp(false)`.
- For contracts that require signatures (`proveDlog`, `proveDHTuple`), add the corresponding `DlogProverInput` / `DhTupleProverInput` to `secrets`.
- `message` is the transaction bytes being signed — `vec![0u8; 32]` is fine for logic-only tests.
- The 4-arg signatures (`prove(tree, ctx, msg, hints)`, `verify(tree, ctx, proof, msg)`) are specific to ergo-lib 0.28. Earlier versions may differ.

## Step 5b: Multisig / Threshold Contracts

For contracts using `atLeast(k, signers)`, the prover needs actual secret keys. Pure `sigmaProp(boolean)` contracts use `TestProver { secrets: vec![] }`, but threshold contracts need `DlogProverInput` secrets.

### Deterministic keypairs (offline, no chain needed)

`DlogProverInput` is pure elliptic curve math — no blockchain, no wallet, no node required. Generate deterministic keys from hardcoded byte arrays:

```rust
use ergo_lib::ergotree_interpreter::sigma_protocol::private_input::{DlogProverInput, PrivateInput};

// Test-only keys — NEVER use on mainnet
const SIGNER_SECRETS: [[u8; 32]; 3] = [
    [0x01, 0x23, 0x45, /* ... */ 0xEF],
    [0xFE, 0xDC, 0xBA, /* ... */ 0x10],
    [0xAA, 0xBB, 0xCC, /* ... */ 0x44],
];

fn signer_keys() -> Vec<DlogProverInput> {
    SIGNER_SECRETS.iter()
        .map(|b| DlogProverInput::from_bytes(b).expect("valid scalar"))
        .collect()
}
```

### Deriving P2PK addresses for ErgoScript `PK("...")` literals

The contract source needs the public key addresses. Derive them once, embed in the `.es` file, compile via node:

```rust
use ergo_lib::ergotree_ir::chain::address::{Address, NetworkAddress, NetworkPrefix};

let dlog_input = DlogProverInput::from_bytes(&secret_bytes).unwrap();
let address = Address::P2Pk(dlog_input.public_image());
let addr_str = NetworkAddress::new(NetworkPrefix::Mainnet, &address).to_base58();
// addr_str = "9gzkoMXa..." — use in PK("9gzkoMXa...") in ErgoScript
```

### Prover setup for threshold proofs

```rust
fn prover_2_of_3() -> TestProver {
    let keys = signer_keys();
    TestProver {
        secrets: vec![
            PrivateInput::DlogProverInput(keys[0].clone()),
            PrivateInput::DlogProverInput(keys[1].clone()),
        ],
    }
}

fn prover_1_of_3() -> TestProver {  // insufficient — for rejection tests
    let keys = signer_keys();
    TestProver {
        secrets: vec![PrivateInput::DlogProverInput(keys[0].clone())],
    }
}
```

The `evaluate()` function must accept a prover parameter:

```rust
fn evaluate(tree: &ErgoTree, ctx: &Context, prover: &TestProver) -> bool {
    let message = vec![0u8; 32];
    let proof = match prover.prove(tree, ctx, message.as_slice(), &HintsBag::empty()) {
        Ok(p) => p.proof,
        Err(_) => return false,
    };
    match TestVerifier.verify(tree, ctx, proof, message.as_slice()) {
        Ok(v) => v.result,
        Err(_) => false,
    }
}
```

### Compilation workflow for multisig contracts

1. Generate deterministic `DlogProverInput` from hardcoded bytes
2. Derive P2PK addresses (one-time helper test with `--nocapture`)
3. Substitute addresses into ErgoScript `PK("...")` literals
4. Compile via node API (one-time curl)
5. Store ErgoTree hex as constant — `cargo test` runs offline

This was proven on the treasury governance contract (3 signers, 2-of-3 threshold, 45 tests).

## Step 6: Build Boxes

### Basic output box

```rust
fn make_output_box(tree: &ErgoTree, value: u64, creation_height: u32) -> ErgoBox {
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: None,
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}
```

### Box with tokens

```rust
fn make_box_with_token(tree: &ErgoTree, value: u64, token_id: TokenId, amount: u64, creation_height: u32) -> ErgoBox {
    let token = Token {
        token_id,
        amount: TokenAmount::try_from(amount).unwrap(),
    };
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: Some(vec![token].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}
```

### Box with registers

```rust
fn make_box_with_registers(
    tree: &ErgoTree,
    value: u64,
    r4: Constant,
    r5: Constant,
    creation_height: u32,
) -> ErgoBox {
    let mut regs = std::collections::HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, r4);
    regs.insert(NonMandatoryRegisterId::R5, r5);
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: None,
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}
```

Common `Constant` constructors:
- `Constant::from(vec![u8])` → `Coll[Byte]`
- `Constant::from(42i32)` → `SInt`
- `Constant::from(100i64)` → `SLong`
- `Constant::from(true)` → `SBoolean`

### Successor boxes (copy from input)

When the contract checks `nextBox.propositionBytes == SELF.propositionBytes` or register equality, clone directly from the input box:

```rust
fn make_successor(prev: &ErgoBox, new_value: u64, creation_height: u32) -> ErgoBox {
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(new_value).unwrap(),
        ergo_tree: prev.ergo_tree.clone(),
        tokens: prev.tokens.clone(),
        additional_registers: prev.additional_registers.clone(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}
```

### Hashing propositionBytes (for blake2b256 script-hash checks)

If the contract stores `blake2b256(script.propositionBytes)` in a register and verifies output scripts against it:

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

`sigma_serialize_bytes()` produces the same bytes as `propositionBytes` in ErgoScript — this is the serialized ErgoTree including header and constants.

## Step 7: Write Tests

### Accept test (contract should evaluate to true)

```rust
#[test]
fn accept_normal_spend() {
    let tree = load_tree(CONTRACT_TREE_HEX);
    let self_box = make_input_box(&tree, ...);
    let outputs = vec![make_successor(&self_box, ...), ...];
    let ctx = build_context(self_box, outputs, height);
    assert!(evaluate(&tree, &ctx), "should accept");
}
```

### Reject test (contract should evaluate to false)

```rust
#[test]
fn reject_wrong_value() {
    let tree = load_tree(CONTRACT_TREE_HEX);
    let self_box = make_input_box(&tree, ...);
    let outputs = vec![make_successor(&self_box, WRONG_VALUE, ...), ...];
    let ctx = build_context(self_box, outputs, height);
    assert!(!evaluate(&tree, &ctx), "should reject");
}
```

## Issues Encountered and Solutions

### Issue 1: `/script/compile` Returns 500 on Node 6.1.2 (CRITICAL)

**Symptom:** `MethodRejection(HttpMethod(OPTIONS))` error from the compile endpoint.

**Cause:** Known issue in some Ergo node versions — the `/script/compile` route is broken.

**Solution:** Use the two-step workaround: `/script/p2sAddress` (returns a P2S address) then `/script/addressToTree/{address}` (returns raw ErgoTree hex). Both endpoints work reliably.

**Also:** `/script/p2sAddress` requires `"treeVersion": 0` in the request body — omitting it returns 400.

### Issue 2: Hex Corruption Causes Cryptic Parse Errors (CRITICAL)

**Symptom:** `SigmaParsingError(VlqEncode(Io("failed to fill whole buffer")))` or `RootParsingError(NonConsumedBytes)` when calling `tree.proposition()` or during `prover.prove()`.

**Cause:** The ErgoTree hex constant has missing or extra bytes. Even a single-byte corruption shifts all subsequent opcodes, producing cascading parse failures. Long hex strings are easy to truncate during copy-paste.

**Solution:** Always save the hex to a file (`> /tmp/tree.txt`) and read it back programmatically. Always include a round-trip serialization test:

```rust
#[test]
fn round_trip() {
    let tree = load_tree(HEX);
    let bytes = tree.sigma_serialize_bytes().unwrap();
    assert_eq!(bytes, hex_to_bytes(HEX));
}
```

This catches corruption immediately. If round-trip passes but `proposition()` fails, the issue is a genuine sigma-rust limitation (unlikely for standard opcodes).

### Issue 3: Rejection Tests Pass for the Wrong Reason

**Symptom:** All `assert!(!evaluate(...))` tests pass, but all `assert!(evaluate(...))` tests fail.

**Cause:** If the ErgoTree can't be parsed, `prover.prove()` returns `Err(...)` and `evaluate()` returns `false` — which looks like a correct rejection. The test asserts `!false == true` and "passes" without ever evaluating the contract.

**Solution:** Always include at least one acceptance test (`assert!(evaluate(...))`) alongside rejection tests. If acceptance tests fail with parsing errors, the rejection tests are unreliable. Fix the parsing issue first.

### Issue 4: Eager ValDef Evaluation Causes Index-Out-of-Bounds (CRITICAL)

**Symptom:** `ByIndex: index Int(N) out of bounds for collection size M` in a path that doesn't use `OUTPUTS(N)`.

**Cause:** The Scala compiler shares `ValDef` bindings across all paths. If the normal path defines `val nextBox = OUTPUTS(2)` and the terminal path only uses `OUTPUTS(0)` and `OUTPUTS(1)`, the `ValDef` for `OUTPUTS(2)` is still evaluated eagerly when the block is entered — before any path selection occurs.

**Example:** A contract with `normalPath || terminalPath` where normalPath accesses `OUTPUTS(0..2)` and terminalPath only accesses `OUTPUTS(0..1)`. Testing the terminal path with 2 outputs crashes even though terminal path logic doesn't need `OUTPUTS(2)`.

**Solution:** Always provide enough output boxes to satisfy ALL `ValDef` bindings, not just the path you're testing. Add dummy outputs for indices accessed by other paths:

```rust
// Terminal path only needs 2 outputs, but normal path ValDefs access OUTPUTS(2)
let outputs = vec![
    make_output_box(&treasury_tree, term_treasury, h),
    make_output_box(&lp_tree, term_lp, h),
    make_output_box(&lp_tree, NANOCOIN, h), // dummy for OUTPUTS(2) ValDef
];
```

### Issue 5: `propositionBytes` Hash Mismatch

**Symptom:** Contract evaluates to false when checking `blake2b256(OUTPUTS(n).propositionBytes) == storedHash`.

**Cause:** The hash in the register was computed from a different serialization of the ErgoTree than what `propositionBytes` returns at evaluation time.

**Solution:** Always compute hashes and build boxes using the SAME `ErgoTree` object:

```rust
let treasury_tree = load_tree(TREASURY_HEX);
let hash = proposition_hash(&treasury_tree); // hash these bytes
// ... store hash in R4 ...
let output = make_output_box(&treasury_tree, value, h); // same tree object
```

Never reconstruct trees or re-serialize between hashing and output construction.

### Issue 6: sigma-rust Compiler Limitations (Reference)

sigma-rust's `ergoscript-compiler` (the `compiler` feature) cannot parse:
- Nested typed lambdas: `OUTPUTS.forall { (o: Box) => o.tokens.forall { (t: (Coll[Byte], Long)) => ... } }`
- Some complex lambda syntax that the Scala compiler handles

**This is a COMPILER limitation, NOT an evaluator limitation.** The same contracts evaluate correctly when the ErgoTree bytes are compiled by the Ergo node and loaded via `sigma_parse_bytes()`. All standard opcodes — `ForAll`, `Exists`, `Map`, `Filter`, `FuncValue`, `SelectField`, `Tuple` types — are fully supported in sigma-rust's deserializer and evaluator.

### Issue 7: Eager ValDef Evaluation of Register Access (CRITICAL)

**Symptom:** Contract evaluates to false (or crashes) on a path that doesn't access certain registers, even though the output box exists.

**Cause:** This is a variant of Issue 4 (eager ValDef). When multiple paths read the same register from `OUTPUTS(0)` — e.g., `out0.R5[Long].get` appears in approval, execution, cancellation, and consolidation paths — the compiler hoists it to a shared `ValDef`. If the output box lacks that register (e.g., a migration output with a different script), `.get` fails before any path selection occurs.

**Example:** A governance contract with 7 paths all reading `out0.R5[Long].get`. Testing the migration path with an output box that has no registers crashes, even though migration doesn't check R5.

**Solution:** OUTPUTS(0) must always have the expected register structure (R4-R8 in the treasury case), even in tests for paths that don't check registers. Use a helper that creates boxes with the full register layout but a different script:

```rust
// Migration output: different script, but same register structure for shared ValDefs
let out0 = make_governance_box_other_script(
    &new_contract_tree,  // different script
    value,
    vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64,  // R4-R8 present
    creation_height,
);
```

This was discovered on the treasury governance contract where 6 paths share `out0.R5[Long].get` and `out0.R8[Long].get` ValDefs.

## Common Error → Fix Reference

| Error | Cause | Fix |
|---|---|---|
| `VlqEncode(Io("failed to fill whole buffer"))` | Hex is truncated/corrupt | Re-export hex from node, verify round-trip |
| `RootParsingError(NonConsumedBytes)` | Hex has extra bytes | Re-export hex from node, verify round-trip |
| `ByIndex: index N out of bounds` | Missing output boxes for shared ValDefs | Add dummy outputs for all OUTPUTS indices accessed by any path |
| `NotImplementedOpCode(X)` | Opcode not in sigma-rust 0.28 | Genuinely unsupported — check sigma-rust issues |
| `InvalidTypeCode(X)` | Unknown type code in serialized tree | Possible version mismatch between node and sigma-rust |
| `MethodRejection(HttpMethod(OPTIONS))` | `/script/compile` broken in node 6.1.2 | Use `/script/p2sAddress` + `/script/addressToTree` |
| `400: missing treeVersion` | `/script/p2sAddress` requires `treeVersion` | Add `"treeVersion": 0` to request JSON |
| All reject tests pass, all accept tests fail | Hex corruption → parse error masquerades as rejection | Fix hex first, add acceptance tests |
| Register `.get` fails on path that doesn't use it | Shared ValDef hoists register access across all OR paths | Ensure OUTPUTS(0) always has full register structure (Issue 7) |

## Minimal Working Example

A complete, self-contained test for a trivial contract:

```rust
use ergo_lib::ergo_chain_types::{Header, PreHeader};
use ergo_lib::ergotree_interpreter::{
    eval::context::Context,
    sigma_protocol::{
        prover::{hint::HintsBag, ContextExtension, Prover, TestProver},
        verifier::{TestVerifier, Verifier},
    },
};
use ergo_lib::ergotree_ir::{
    chain::ergo_box::{box_value::BoxValue, ErgoBox, ErgoBoxCandidate, NonMandatoryRegisters},
    chain::tx_id::TxId,
    ergo_tree::ErgoTree,
    serialization::SigmaSerializable,
};
use sigma_test_util::force_any_val;

// sigmaProp(HEIGHT > 0) — compiled via node API
const TREE_HEX: &str = "10010400d191a37300";

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn height_gt_zero_accepts_at_height_1() {
    let tree = ErgoTree::sigma_parse_bytes(&hex_to_bytes(TREE_HEX)).unwrap();

    let self_box = ErgoBox::from_box_candidate(
        &ErgoBoxCandidate {
            value: BoxValue::try_from(1_000_000_000u64).unwrap(),
            ergo_tree: tree.clone(),
            tokens: None,
            additional_registers: NonMandatoryRegisters::empty(),
            creation_height: 0,
        },
        TxId::zero(), 0,
    ).unwrap();

    let self_ref: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outputs: &'static [ErgoBox] = Vec::leak(vec![]);
    let inputs = [self_ref].into();

    let ctx = Context {
        height: 1,
        self_box: self_ref,
        outputs,
        data_inputs: None,
        inputs,
        pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };

    let prover = TestProver { secrets: vec![] };
    let msg = vec![0u8; 32];
    let proof = prover.prove(&tree, &ctx, &msg, &HintsBag::empty()).unwrap();
    let result = TestVerifier.verify(&tree, &ctx, proof.proof, &msg).unwrap();
    assert!(result.result);
}
```

## Comparison: Rust vs Scala Test Harness

| Dimension | Rust (this skill) | Scala (`scala-test-harness` skill) |
|---|---|---|
| Compilation | Node API (one-time curl) | In-process (sigmastate-interpreter) |
| Evaluation | In-process (sigma-rust) | Against live node |
| Toolchain | `cargo` only | `sbt` + Scala 2.12 + AppKit 5.0.4 |
| Build time | ~15s first build | ~2 min first build |
| Test speed | ~10ms per test | ~1-5s per test (node round-trip) |
| Setup | Zero — just `cargo test` | Node running + wallet configured |
| Consensus checks | No (logic only) | Yes (MIN_BOX_VALUE, block-value, fees) |
| Suitable for | Fast iteration, CI, logic validation | Final pre-deploy mainnet validation |

**Recommendation:** Use Rust for rapid development and CI. Use Scala for final pre-deploy validation against a real node. They complement each other.
