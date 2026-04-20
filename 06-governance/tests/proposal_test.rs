//! proposal_test.rs — ErgoTree evaluation tests for proposal.es v1.1
//!
//! Paths: state advancement (1→2), execution (treasury + token burn).
//! ErgoTree compiled via Ergo node 6.x.

use std::collections::HashMap;
use blake2::{digest::consts::U32, Blake2b, Digest};
use ergo_lib::ergo_chain_types::{Digest32, Header, PreHeader};
use ergo_lib::ergotree_interpreter::{
    eval::context::Context,
    sigma_protocol::prover::{hint::HintsBag, ContextExtension, Prover, TestProver},
    sigma_protocol::verifier::{TestVerifier, Verifier},
};
use ergo_lib::ergotree_ir::{
    chain::{
        ergo_box::{box_value::BoxValue, ErgoBox, ErgoBoxCandidate, NonMandatoryRegisterId, NonMandatoryRegisters},
        token::{Token, TokenAmount, TokenId},
        tx_id::TxId,
    },
    ergo_tree::ErgoTree,
    mir::constant::{Constant, Literal},
    serialization::SigmaSerializable,
    types::stype::SType, types::stuple::STuple,
};
use sigma_test_util::force_any_val;

type Blake2b256 = Blake2b<U32>;

const PROPOSAL_TREE_HEX: &str = include_str!("/tmp/proposal_tree.hex");
const COUNTING_TREE_HEX: &str = include_str!("/tmp/counting_tree.hex");
const TREASURY_TREE_HEX: &str = include_str!("/tmp/treasury_tree.hex");
const TRUE_TREE_HEX: &str = "10010101d17300";

fn treasury_nft_id() -> TokenId { TokenId::from(Digest32::from([0xDDu8; 32])) }
fn proposal_token_id() -> TokenId { TokenId::from(Digest32::from([0xFFu8; 32])) }
// Use a distinct token for the counting box's "validation height" check
fn counting_token_id() -> TokenId { TokenId::from(Digest32::from([0xEEu8; 32])) }

const NANOCOIN: u64 = 1_000_000_000;
const MIN_BOX: u64 = 360_000;

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap()).collect()
}
fn load_tree(hex: &str) -> ErgoTree { ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).unwrap() }

fn make_tuple(a: i64, b: i64) -> Constant {
    Constant { tpe: SType::STuple(STuple::try_from(vec![SType::SLong, SType::SLong]).unwrap()), v: Literal::Tup(vec![Literal::Long(a), Literal::Long(b)].try_into().unwrap()) }
}
fn make_token(id: TokenId, amt: u64) -> Token {
    Token { token_id: id, amount: TokenAmount::try_from(amt).unwrap() }
}
fn prop_hash(tree: &ErgoTree) -> Vec<u8> {
    let bytes = tree.sigma_serialize_bytes().unwrap();
    let mut h = Blake2b256::new(); h.update(&bytes); h.finalize().to_vec()
}

fn make_proposal_box(proportion: i64, recipient: Vec<u8>, token_qty: u64, validation_height: i64, ch: u32) -> ErgoBox {
    let tree = load_tree(PROPOSAL_TREE_HEX);
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, make_tuple(proportion, 0));
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(recipient));
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(validation_height));
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(), ergo_tree: tree,
        tokens: Some(vec![make_token(proposal_token_id(), token_qty)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(), creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    let p = TestProver { secrets: vec![] };
    let m = vec![0u8; 32];
    match p.prove(tree, ctx, &m, &HintsBag::empty()) {
        Ok(pr) => TestVerifier.verify(tree, ctx, pr.proof, &m).map(|v| v.result).unwrap_or(false),
        Err(_) => false,
    }
}

fn build_ctx(self_box: ErgoBox, others: Vec<ErgoBox>, outputs: Vec<ErgoBox>, h: u32) -> Context<'static> {
    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outs: &'static [ErgoBox] = Vec::leak(outputs);
    let mut refs: Vec<&'static ErgoBox> = vec![sr];
    for o in others { refs.push(Box::leak(Box::new(o))); }
    Context {
        height: h, self_box: sr, outputs: outs, data_inputs: None,
        inputs: refs.try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

// ============================================================
// PHASE 0
// ============================================================

#[test]
fn proposal_tree_round_trips() {
    let t = load_tree(PROPOSAL_TREE_HEX);
    assert_eq!(t.sigma_serialize_bytes().unwrap(), hex_to_bytes(PROPOSAL_TREE_HEX));
}

#[test]
fn proposal_proposition_parses() {
    load_tree(PROPOSAL_TREE_HEX).proposition().expect("parse");
}

// ============================================================
// PATH 1: STATE ADVANCEMENT (1 → 2)
// ============================================================

#[test]
fn accept_advancement() {
    let proportion = 500_000i64;
    let recipient_hash = prop_hash(&load_tree(TRUE_TREE_HEX));
    let validation_height = 1000i64;

    // SELF = proposal box (qty=1, pending)
    let self_box = make_proposal_box(proportion, recipient_hash.clone(), 1, validation_height, 0);

    // INPUTS(0) = counting box with token qty matching validation_height
    let counting_tree = load_tree(COUNTING_TREE_HEX);
    let counting_box = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: counting_tree,
        tokens: Some(vec![make_token(counting_token_id(), validation_height as u64)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 0,
    }, TxId::zero(), 0).unwrap();

    // OUTPUTS(0) = counting successor (dummy)
    let dummy_out0 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 0).unwrap();

    // OUTPUTS(1) = proposal successor (qty=2, same script/R4/R5)
    let out1 = make_proposal_box(proportion, recipient_hash, 2, validation_height, 1);

    // Context: INPUTS(0)=counting, SELF=proposal at INPUTS(1)
    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counting_box));
    let outs: &'static [ErgoBox] = Vec::leak(vec![dummy_out0, out1]);

    let ctx = Context {
        height: 200, self_box: sr, outputs: outs, data_inputs: None,
        inputs: vec![cr, sr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(evaluate(&load_tree(PROPOSAL_TREE_HEX), &ctx), "advancement should accept");
}

#[test]
fn reject_advancement_wrong_token_qty() {
    let proportion = 500_000i64;
    let rh = prop_hash(&load_tree(TRUE_TREE_HEX));
    let vh = 1000i64;

    // SELF already at qty=2 — can't advance again
    let self_box = make_proposal_box(proportion, rh.clone(), 2, vh, 0);

    let counting_tree = load_tree(COUNTING_TREE_HEX);
    let counting_box = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: counting_tree,
        tokens: Some(vec![make_token(counting_token_id(), vh as u64)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 0,
    }, TxId::zero(), 0).unwrap();

    let dummy_out0 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 0).unwrap();
    let out1 = make_proposal_box(proportion, rh, 2, vh, 1); // trying 2→2

    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counting_box));
    let outs: &'static [ErgoBox] = Vec::leak(vec![dummy_out0, out1]);
    let ctx = Context {
        height: 200, self_box: sr, outputs: outs, data_inputs: None,
        inputs: vec![cr, sr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(!evaluate(&load_tree(PROPOSAL_TREE_HEX), &ctx), "already-advanced proposal should reject");
}

#[test]
fn reject_advancement_proportion_changed() {
    let proportion = 500_000i64;
    let rh = prop_hash(&load_tree(TRUE_TREE_HEX));
    let vh = 1000i64;

    let self_box = make_proposal_box(proportion, rh.clone(), 1, vh, 0);

    let counting_box = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: load_tree(COUNTING_TREE_HEX),
        tokens: Some(vec![make_token(counting_token_id(), vh as u64)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 0,
    }, TxId::zero(), 0).unwrap();

    let dummy_out0 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 0).unwrap();
    // ATTACK: proportion changed in successor
    let out1 = make_proposal_box(9_000_000, rh, 2, vh, 1);

    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counting_box));
    let outs: &'static [ErgoBox] = Vec::leak(vec![dummy_out0, out1]);
    let ctx = Context {
        height: 200, self_box: sr, outputs: outs, data_inputs: None,
        inputs: vec![cr, sr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(!evaluate(&load_tree(PROPOSAL_TREE_HEX), &ctx), "changed proportion should reject");
}

// ============================================================
// PATH 2: EXECUTION (treasury + token burn)
// ============================================================

#[test]
fn accept_execution_token_burned() {
    let proportion = 500_000i64;
    let rh = prop_hash(&load_tree(TRUE_TREE_HEX));
    let vh = 1000i64;

    // SELF = proposal at qty=2 (passed)
    let self_box = make_proposal_box(proportion, rh, 2, vh, 0);

    // INPUTS(1) = treasury box with treasury NFT
    let treasury_box = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(100 * NANOCOIN).unwrap(), ergo_tree: load_tree(TREASURY_TREE_HEX),
        tokens: Some(vec![make_token(treasury_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 0,
    }, TxId::zero(), 1).unwrap();

    // OUTPUTS: no proposal token anywhere (burned)
    let out0 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(95 * NANOCOIN).unwrap(), ergo_tree: load_tree(TREASURY_TREE_HEX),
        tokens: Some(vec![make_token(treasury_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 0).unwrap();
    let out1 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(5 * NANOCOIN).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 1).unwrap();

    let ctx = build_ctx(self_box, vec![treasury_box], vec![out0, out1], 200);
    assert!(evaluate(&load_tree(PROPOSAL_TREE_HEX), &ctx), "execution with burn should accept");
}

#[test]
fn reject_execution_token_not_burned() {
    let proportion = 500_000i64;
    let rh = prop_hash(&load_tree(TRUE_TREE_HEX));
    let vh = 1000i64;

    let self_box = make_proposal_box(proportion, rh, 2, vh, 0);

    let treasury_box = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(100 * NANOCOIN).unwrap(), ergo_tree: load_tree(TREASURY_TREE_HEX),
        tokens: Some(vec![make_token(treasury_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 0,
    }, TxId::zero(), 1).unwrap();

    // ATTACK: proposal token smuggled into output
    let out0 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(95 * NANOCOIN).unwrap(), ergo_tree: load_tree(TREASURY_TREE_HEX),
        tokens: Some(vec![make_token(treasury_nft_id(), 1), make_token(proposal_token_id(), 2)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 0).unwrap();
    let out1 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(5 * NANOCOIN).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 1).unwrap();

    let ctx = build_ctx(self_box, vec![treasury_box], vec![out0, out1], 200);
    assert!(!evaluate(&load_tree(PROPOSAL_TREE_HEX), &ctx), "token not burned should reject");
}

#[test]
fn reject_execution_not_passed() {
    let proportion = 500_000i64;
    let rh = prop_hash(&load_tree(TRUE_TREE_HEX));
    let vh = 1000i64;

    // SELF at qty=1 (NOT passed)
    let self_box = make_proposal_box(proportion, rh, 1, vh, 0);

    let treasury_box = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(100 * NANOCOIN).unwrap(), ergo_tree: load_tree(TREASURY_TREE_HEX),
        tokens: Some(vec![make_token(treasury_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 0,
    }, TxId::zero(), 1).unwrap();

    let out0 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(95 * NANOCOIN).unwrap(), ergo_tree: load_tree(TREASURY_TREE_HEX),
        tokens: Some(vec![make_token(treasury_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 0).unwrap();
    let out1 = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(5 * NANOCOIN).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 1).unwrap();

    let ctx = build_ctx(self_box, vec![treasury_box], vec![out0, out1], 200);
    assert!(!evaluate(&load_tree(PROPOSAL_TREE_HEX), &ctx), "not-passed proposal should reject execution");
}
