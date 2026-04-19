//! lp_fund_test.rs — ErgoTree evaluation tests for the LP fund contracts v1.1.
//!
//! v1.1 audit fixes: R6 (migration height) preserved across all non-migration paths,
//! read at top level to avoid eager ValDef crash.
//! ErgoTree compiled via Ergo node 6.1.2 `/script/p2sAddress` + `/script/addressToTree`.
//! Tests use deterministic keypairs for the 2-of-3 multisig threshold proofs.
//!
//! Targets: ergo-lib 0.28 with "arbitrary" feature, rustc 1.85+

use std::collections::HashMap;
use blake2::{digest::consts::U32, Blake2b, Digest};
use ergo_lib::ergo_chain_types::{Digest32, Header, PreHeader};
use ergo_lib::ergotree_interpreter::{
    eval::context::Context,
    sigma_protocol::{
        prover::{hint::HintsBag, ContextExtension, Prover, TestProver},
        verifier::{TestVerifier, Verifier},
        private_input::{DlogProverInput, PrivateInput},
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

type Blake2b256 = Blake2b<U32>;

// ============================================================
// PRE-COMPILED ERGOTREE HEX CONSTANTS
// ============================================================

// LP Fund Governance contract v1.1 — compiled by Ergo node 6.1.2
const GOVERNANCE_TREE_HEX: &str = "10140502040004000400040004000502040408cd034646ae5047316b4230d0086c8acec687f00b1cd9d1dc634f6cb358ac0a9a8fff08cd0288e2ddeb04657dbd0edadf9c1f98da3b3895faa1f00527934dd35d17542ffe9b08cd020305c75318f36537e1a5d0db4dfcdc94a9708a84a38f81ea7cfbe239252e01f20402050205000504050405c0ca01040004000502d812d601e4c6a70505d602ef9372017300d603b2a5730100d60493c27203c2a7d605e4c67203041ad606e4c6a7041ad6079372057206d608e4c672030505d6099372087201d60ae4c672030605d60be4c6a70605d60c93720a720bd60ddb63087203d60e91b1720d7302d60f8cb2db6308a773030001d610eded720e938cb2720d73040001720f938cb2720d730500027306d61192c17203c1a7d6127ea305ea0298730783030873087309730ad1ecececececedededededed7202720472077209720cafb4a5730bb1a5d9011363ae7206d901150e937215cbc272137210ededededededed7202720472117209720c91b17205b17206af7206d901130eae7205d901150e93721572137210edededededed72027204721172077209720c7210ededededed7202720472117207937208730c7210ededededededed7202937201730d720472117207937208730e93720a72127210eded937201730f9172129a720b7310eded720e938cb2720d73110001720f938cb2720d731200027313";

// LP Fund Accumulation contract v1.0 — compiled by Ergo node 6.1.2
const ACCUMULATION_TREE_HEX: &str = "100204000e20f7e52722204eab03cf13bea0772e0e00ee48f4a6f81396514d9a3b692d61b1e4d802d601b2a5730000d602c27201d1eced937202c2a792c17201c1a793cb72027301";

// ============================================================
// DETERMINISTIC SIGNER KEYS (test-only, never on mainnet)
// Same keys as treasury — contracts share the same signers.
// ============================================================

const SIGNER_SECRET_BYTES: [[u8; 32]; 3] = [
    [
        0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
        0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF,
    ],
    [
        0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
        0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
        0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
        0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
    ],
    [
        0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44,
        0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44,
        0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44,
        0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x22, 0x33, 0x44,
    ],
];

const NANOCOIN: u64 = 1_000_000_000;
const TIMELOCK_BLOCKS: i64 = 12960;
const TX_FEE: u64 = 1_000_000;

// ============================================================
// HELPERS
// ============================================================

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn load_tree(hex: &str) -> ErgoTree {
    ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).expect("valid ErgoTree")
}

fn load_governance_tree() -> ErgoTree { load_tree(GOVERNANCE_TREE_HEX) }
fn load_accumulation_tree() -> ErgoTree { load_tree(ACCUMULATION_TREE_HEX) }

fn signer_keys() -> Vec<DlogProverInput> {
    SIGNER_SECRET_BYTES
        .iter()
        .map(|b| DlogProverInput::from_bytes(b).expect("valid scalar"))
        .collect()
}

fn prover_2_of_3() -> TestProver {
    let keys = signer_keys();
    TestProver {
        secrets: vec![
            PrivateInput::DlogProverInput(keys[0].clone()),
            PrivateInput::DlogProverInput(keys[1].clone()),
        ],
    }
}

fn prover_1_of_3() -> TestProver {
    let keys = signer_keys();
    TestProver {
        secrets: vec![PrivateInput::DlogProverInput(keys[0].clone())],
    }
}

fn build_context(self_box: ErgoBox, outputs: Vec<ErgoBox>, height: u32) -> Context<'static> {
    let self_ref: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outputs_static: &'static [ErgoBox] = Vec::leak(outputs);
    let inputs_arr: [&'static ErgoBox; 1] = [self_ref];
    let inputs = inputs_arr.into();

    Context {
        height,
        self_box: self_ref,
        outputs: outputs_static,
        data_inputs: None,
        inputs,
        pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

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

fn proposition_hash(tree: &ErgoTree) -> Vec<u8> {
    let bytes = tree.sigma_serialize_bytes().expect("serialize tree");
    let mut h = Blake2b256::new();
    h.update(&bytes);
    h.finalize().to_vec()
}

// ============================================================
// BOX CONSTRUCTION
// ============================================================

fn make_nft_token() -> Token {
    Token {
        token_id: TokenId::from(Digest32::from([0xBBu8; 32])),
        amount: TokenAmount::try_from(1u64).unwrap(),
    }
}

fn make_output_box(tree: &ErgoTree, value: u64, creation_height: u32) -> ErgoBox {
    ErgoBox::from_box_candidate(
        &ErgoBoxCandidate {
            value: BoxValue::try_from(value).unwrap(),
            ergo_tree: tree.clone(),
            tokens: None,
            additional_registers: NonMandatoryRegisters::empty(),
            creation_height,
        },
        TxId::zero(), 0,
    ).unwrap()
}

/// Build an LP fund governance box with NFT and all registers.
/// R4: Coll[Coll[Byte]] — whitelist
/// R5: Long — state flag (0=normal, 1=frozen, 2=migration approved)
/// R6: Long — migration height (0 = no migration)
fn make_governance_box(
    tree: &ErgoTree,
    value: u64,
    whitelist: Vec<Vec<u8>>,
    state_flag: i64,
    migration_height: i64,
    creation_height: u32,
) -> ErgoBox {
    let mut regs = HashMap::new();
    // R4: Coll[Coll[Byte]]
    let whitelist_const: Constant = whitelist.into();
    regs.insert(NonMandatoryRegisterId::R4, whitelist_const);
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(state_flag));
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(migration_height));

    ErgoBox::from_box_candidate(
        &ErgoBoxCandidate {
            value: BoxValue::try_from(value).unwrap(),
            ergo_tree: tree.clone(),
            tokens: Some(vec![make_nft_token()].try_into().unwrap()),
            additional_registers: NonMandatoryRegisters::new(regs).unwrap(),
            creation_height,
        },
        TxId::zero(), 0,
    ).unwrap()
}

/// Governance box with NFT but a different script (for migration output).
/// Includes R4/R5/R6 for shared ValDef compatibility.
fn make_governance_box_other_script(
    tree: &ErgoTree,
    value: u64,
    whitelist: Vec<Vec<u8>>,
    state_flag: i64,
    migration_height: i64,
    creation_height: u32,
) -> ErgoBox {
    let mut regs = HashMap::new();
    let whitelist_const: Constant = whitelist.into();
    regs.insert(NonMandatoryRegisterId::R4, whitelist_const);
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(state_flag));
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(migration_height));

    ErgoBox::from_box_candidate(
        &ErgoBoxCandidate {
            value: BoxValue::try_from(value).unwrap(),
            ergo_tree: tree.clone(),
            tokens: Some(vec![make_nft_token()].try_into().unwrap()),
            additional_registers: NonMandatoryRegisters::new(regs).unwrap(),
            creation_height,
        },
        TxId::zero(), 0,
    ).unwrap()
}

/// Shorthand: idle governance box (normal state, no migration)
fn make_idle_governance_box(tree: &ErgoTree, value: u64, whitelist: Vec<Vec<u8>>, creation_height: u32) -> ErgoBox {
    make_governance_box(tree, value, whitelist, 0i64, 0i64, creation_height)
}

/// Build a sample whitelist with one entry (the accumulation contract hash)
fn sample_whitelist() -> Vec<Vec<u8>> {
    vec![proposition_hash(&load_accumulation_tree())]
}

/// Build a whitelist with two entries
fn sample_whitelist_2() -> Vec<Vec<u8>> {
    let mut wl = sample_whitelist();
    wl.push(vec![0xFFu8; 32]); // dummy second entry
    wl
}

fn whitelisted_tree() -> ErgoTree {
    load_accumulation_tree()
}

// ============================================================
// TESTS
// ============================================================

#[cfg(test)]
mod phase0_load {
    use super::*;

    #[test]
    fn governance_ergotree_loads_and_has_valid_size() {
        let tree = load_governance_tree();
        let size = tree.sigma_serialize_bytes().unwrap().len();
        println!("LP fund governance v1.1 ErgoTree size: {} bytes", size);
        assert!(size > 100, "ErgoTree too small");
    }

    #[test]
    fn accumulation_ergotree_loads_and_has_valid_size() {
        let tree = load_accumulation_tree();
        let size = tree.sigma_serialize_bytes().unwrap().len();
        assert!(size > 10, "ErgoTree too small");
    }

    #[test]
    fn governance_round_trip_serialization() {
        let tree = load_governance_tree();
        let bytes = tree.sigma_serialize_bytes().unwrap();
        assert_eq!(bytes, hex_to_bytes(GOVERNANCE_TREE_HEX), "governance round-trip mismatch");
    }

    #[test]
    fn accumulation_round_trip_serialization() {
        let tree = load_accumulation_tree();
        let bytes = tree.sigma_serialize_bytes().unwrap();
        assert_eq!(bytes, hex_to_bytes(ACCUMULATION_TREE_HEX), "accumulation round-trip mismatch");
    }

    #[test]
    fn governance_proposition_parses() {
        load_governance_tree().proposition().expect("should parse");
    }

    #[test]
    fn accumulation_proposition_parses() {
        load_accumulation_tree().proposition().expect("should parse");
    }
}

#[cfg(test)]
mod phase1_threshold {
    use super::*;

    #[test]
    fn accept_2_of_3_threshold() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        // Valid spend to whitelisted destination
        let out0 = make_governance_box(&tree, value - NANOCOIN, wl.clone(), 0, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "2-of-3 threshold should accept");
    }

    #[test]
    fn reject_1_of_3_threshold() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value - NANOCOIN, wl.clone(), 0, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_1_of_3()), "1-of-3 should reject");
    }
}

#[cfg(test)]
mod phase2_spend {
    use super::*;

    #[test]
    fn accept_spend_to_whitelisted_destination() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value - spend_amount, wl.clone(), 0, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "whitelisted spend should accept");
    }

    #[test]
    fn reject_spend_to_non_whitelisted_destination() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        // Use a random tree that's NOT in the whitelist
        let wrong_tree = load_tree("10010400d191a37300"); // sigmaProp(HEIGHT > 0)

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value - spend_amount, wl.clone(), 0, 0, h);
        let out1 = make_output_box(&wrong_tree, spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "non-whitelisted spend should reject");
    }

    #[test]
    fn reject_spend_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, wl.clone(), 1, 0, h - 1); // frozen
        let out0 = make_governance_box(&tree, value - spend_amount, wl.clone(), 1, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "spend when frozen should reject");
    }

    #[test]
    fn reject_spend_whitelist_not_preserved() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let wl2 = sample_whitelist_2(); // different whitelist in output
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value - spend_amount, wl2, 0, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "whitelist change during spend should reject");
    }

    #[test]
    fn reject_spend_flag_not_preserved() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        // Change state flag to 2 during spend (sneaky migration bypass)
        let out0 = make_governance_box(&tree, value - spend_amount, wl.clone(), 2, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "flag change during spend should reject");
    }

    #[test]
    fn reject_spend_migration_height_not_preserved() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        // Change migration height during spend (audit fix test)
        let out0 = make_governance_box(&tree, value - spend_amount, wl.clone(), 0, 999, h);
        let out1 = make_output_box(&whitelisted_tree(), spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration height change during spend should reject");
    }

    #[test]
    fn reject_spend_script_not_preserved() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;
        let spend_amount = 10 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        // Change box goes to accumulation tree instead of governance
        let out0 = make_governance_box_other_script(&load_accumulation_tree(), value - spend_amount, wl.clone(), 0, 0, h);
        let out1 = make_output_box(&whitelisted_tree(), spend_amount, h);
        let ctx = build_context(self_box, vec![out0, out1], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "script change in change box should reject");
    }
}

#[cfg(test)]
mod phase3_whitelist_update {
    use super::*;

    #[test]
    fn accept_whitelist_add_entry() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let wl_new = sample_whitelist_2(); // superset with one more entry
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value, wl_new, 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "whitelist addition should accept");
    }

    #[test]
    fn reject_whitelist_remove_entry() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist_2(); // start with 2 entries
        let wl_shrunk = sample_whitelist(); // only 1 entry
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value, wl_shrunk, 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "whitelist removal should reject");
    }

    #[test]
    fn reject_whitelist_replace_entry() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        // New whitelist has a different entry (not superset)
        let wl_replaced = vec![vec![0xAAu8; 32], vec![0xCCu8; 32]];
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value, wl_replaced, 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "whitelist replacement should reject");
    }

    #[test]
    fn reject_whitelist_update_value_stolen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let wl_new = sample_whitelist_2();
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value - NANOCOIN, wl_new, 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "value decrease during whitelist update should reject");
    }

    #[test]
    fn reject_whitelist_update_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let wl_new = sample_whitelist_2();
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, wl.clone(), 1, 0, h - 1); // frozen
        let out0 = make_governance_box(&tree, value, wl_new, 1, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "whitelist update when frozen should reject");
    }
}

#[cfg(test)]
mod phase4_consolidation {
    use super::*;

    #[test]
    fn accept_consolidation_value_increases() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, wl.clone(), 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "consolidation should accept");
    }

    #[test]
    fn reject_consolidation_value_decreases() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value - NANOCOIN, wl.clone(), 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "value decrease in consolidation should reject");
    }

    #[test]
    fn reject_consolidation_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 50 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, wl.clone(), 1, 0, h - 1); // frozen
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, wl.clone(), 1, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "consolidation when frozen should reject");
    }

    #[test]
    fn reject_consolidation_whitelist_changed() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        // Same-size but different whitelist (not a superset, so isWhitelistUpdate also fails)
        let wl_different = vec![vec![0xDDu8; 32]];
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, wl_different, 0, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "whitelist change during consolidation should reject");
    }

    #[test]
    fn reject_consolidation_migration_height_changed() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        // Attempt to change migration height during consolidation (audit fix test)
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, wl.clone(), 0, 500, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration height change during consolidation should reject");
    }
}

#[cfg(test)]
mod phase5_freeze {
    use super::*;

    #[test]
    fn accept_freeze() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        let out0 = make_governance_box(&tree, value, wl.clone(), 1, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "freeze should accept");
    }

    #[test]
    fn reject_freeze_already_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, wl.clone(), 1, 0, h - 1);
        let out0 = make_governance_box(&tree, value, wl.clone(), 1, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "already frozen should reject");
    }
}

#[cfg(test)]
mod phase6_migration {
    use super::*;

    #[test]
    fn accept_migration_approval() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, wl.clone(), h - 1);
        // Output: R5=2 (migration flag), R6=HEIGHT
        let out0 = make_governance_box(&tree, value, wl.clone(), 2, h as i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "migration approval should accept");
    }

    #[test]
    fn reject_migration_approval_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, wl.clone(), 1, 0, h - 1);
        let out0 = make_governance_box(&tree, value, wl.clone(), 2, h as i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration approval when frozen should reject");
    }

    #[test]
    fn reject_migration_approval_not_normal_state() {
        let tree = load_governance_tree();
        let h = 200u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        // Already in migration state (R5=2)
        let self_box = make_governance_box(&tree, value, wl.clone(), 2, 50, h - 1);
        let out0 = make_governance_box(&tree, value, wl.clone(), 2, h as i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration approval from non-normal state should reject");
    }

    #[test]
    fn accept_migration_execute_after_timelock() {
        let tree = load_governance_tree();
        let new_contract = load_accumulation_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        // Self: R5=2 (migration approved), R6=approval_h
        let self_box = make_governance_box(&tree, value, wl.clone(), 2, approval_h, exec_h - 1);

        // Output: NFT goes to new contract. Needs registers for shared ValDefs.
        let out0 = make_governance_box_other_script(&new_contract, value - TX_FEE, wl.clone(), 0, 0, exec_h);
        let dummy = make_output_box(&tree, NANOCOIN, exec_h);
        let ctx = build_context(self_box, vec![out0, dummy], exec_h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "migration execute should accept");
    }

    #[test]
    fn reject_migration_execute_before_timelock() {
        let tree = load_governance_tree();
        let new_contract = load_accumulation_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS) as u32; // exact boundary, not past
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, wl.clone(), 2, approval_h, exec_h - 1);
        let out0 = make_governance_box_other_script(&new_contract, value - TX_FEE, wl.clone(), 0, 0, exec_h);
        let dummy = make_output_box(&tree, NANOCOIN, exec_h);
        let ctx = build_context(self_box, vec![out0, dummy], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration before timelock should reject");
    }

    #[test]
    fn reject_migration_execute_without_flag() {
        let tree = load_governance_tree();
        let new_contract = load_accumulation_tree();
        let exec_h = 20000u32;
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        // R5=0, not migration approved
        let self_box = make_governance_box(&tree, value, wl.clone(), 0, 0, exec_h - 1);
        let out0 = make_governance_box_other_script(&new_contract, value - TX_FEE, wl.clone(), 0, 0, exec_h);
        let dummy = make_output_box(&tree, NANOCOIN, exec_h);
        let ctx = build_context(self_box, vec![out0, dummy], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration without flag should reject");
    }

    #[test]
    fn full_migration_flow() {
        let tree = load_governance_tree();
        let wl = sample_whitelist();
        let value = 100 * NANOCOIN;

        // Step 1: Migration approval (R5: 0→2, R6=HEIGHT)
        let h1 = 100u32;
        let self1 = make_idle_governance_box(&tree, value, wl.clone(), h1 - 1);
        let out1 = make_governance_box(&tree, value, wl.clone(), 2, h1 as i64, h1);
        let dummy1 = make_output_box(&tree, NANOCOIN, h1);
        let ctx1 = build_context(self1, vec![out1, dummy1], h1);
        assert!(evaluate(&tree, &ctx1, &prover_2_of_3()), "migration approval step should accept");

        // Step 2: Migration execute after timelock
        let h2 = (h1 as i64 + TIMELOCK_BLOCKS + 1) as u32;
        let new_contract = load_accumulation_tree();
        let self2 = make_governance_box(&tree, value, wl.clone(), 2, h1 as i64, h1);
        let out2 = make_governance_box_other_script(&new_contract, value - TX_FEE, wl.clone(), 0, 0, h2);
        let dummy2 = make_output_box(&tree, NANOCOIN, h2);
        let ctx2 = build_context(self2, vec![out2, dummy2], h2);
        assert!(evaluate(&tree, &ctx2, &prover_2_of_3()), "migration execute step should accept");
    }
}

#[cfg(test)]
mod phase7_accumulation {
    use super::*;

    #[test]
    fn accept_accum_consolidation() {
        let tree = load_accumulation_tree();
        let h = 100u32;
        let value = 10 * NANOCOIN;

        let self_box = make_output_box(&tree, value, h - 1);
        let out0 = make_output_box(&tree, value + 5 * NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0], h);
        let prover = TestProver { secrets: vec![] };
        assert!(evaluate(&tree, &ctx, &prover), "accum consolidation should accept");
    }

    #[test]
    fn accept_accum_transfer_to_governance() {
        let accum_tree = load_accumulation_tree();
        let gov_tree = load_governance_tree();
        let h = 100u32;
        let value = 10 * NANOCOIN;

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&gov_tree, value, h);
        let ctx = build_context(self_box, vec![out0], h);
        let prover = TestProver { secrets: vec![] };
        assert!(evaluate(&accum_tree, &ctx, &prover), "transfer to governance should accept");
    }

    #[test]
    fn reject_accum_wrong_destination() {
        let accum_tree = load_accumulation_tree();
        let wrong_tree = load_tree("10010400d191a37300");
        let h = 100u32;
        let value = 10 * NANOCOIN;

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&wrong_tree, value, h);
        let ctx = build_context(self_box, vec![out0], h);
        let prover = TestProver { secrets: vec![] };
        assert!(!evaluate(&accum_tree, &ctx, &prover), "wrong destination should reject");
    }

    #[test]
    fn reject_accum_value_decrease() {
        let tree = load_accumulation_tree();
        let h = 100u32;
        let value = 10 * NANOCOIN;

        let self_box = make_output_box(&tree, value, h - 1);
        let out0 = make_output_box(&tree, value - NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0], h);
        let prover = TestProver { secrets: vec![] };
        assert!(!evaluate(&tree, &ctx, &prover), "value decrease should reject");
    }
}

#[cfg(test)]
mod phase8_audit_fixes {
    use super::*;

    /// Audit finding: R6 must be preserved during consolidation to prevent timelock bypass.
    /// This test verifies the v1.1 fix works.
    #[test]
    fn reject_consolidation_r6_tamper_during_migration() {
        let tree = load_governance_tree();
        let h = 200u32;
        let wl = sample_whitelist();
        let value = 50 * NANOCOIN;
        let approval_h = 100i64;

        // Box is in migration-approved state (R5=2, R6=100)
        let self_box = make_governance_box(&tree, value, wl.clone(), 2, approval_h, h - 1);
        // Consolidation attempt that resets R6 to 0 (would bypass timelock in v1.0)
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, wl.clone(), 2, 0, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "R6 tamper during migration should reject (v1.1 fix)");
    }

    /// Verify consolidation still works during migration-approved state when R6 is preserved.
    #[test]
    fn accept_consolidation_during_migration_r6_preserved() {
        let tree = load_governance_tree();
        let h = 200u32;
        let wl = sample_whitelist();
        let value = 50 * NANOCOIN;
        let approval_h = 100i64;

        let self_box = make_governance_box(&tree, value, wl.clone(), 2, approval_h, h - 1);
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, wl.clone(), 2, approval_h, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "consolidation during migration with R6 preserved should accept");
    }
}
