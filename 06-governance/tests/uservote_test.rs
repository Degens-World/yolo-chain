//! uservote_test.rs — ErgoTree evaluation tests for userVote.es v1.1
//!
//! Paths: cancel (voter sig + cooldown), submit (permissionless + NFT burn).
//! ErgoTree compiled via Ergo node 6.x.

use std::collections::HashMap;
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
    mir::constant::Constant,
    serialization::SigmaSerializable,
};
use sigma_test_util::force_any_val;

const USERVOTE_TREE_HEX: &str = include_str!("/tmp/userVote_tree.hex");
const COUNTING_TREE_HEX: &str = include_str!("/tmp/counting_tree.hex");
const TRUE_TREE_HEX: &str = "10010101d17300";

fn valid_vote_id() -> TokenId { TokenId::from(Digest32::from([0xFFu8; 32])) }
fn vyolo_id() -> TokenId { TokenId::from(Digest32::from([0xCCu8; 32])) }
fn counter_nft_id() -> TokenId { TokenId::from(Digest32::from([0xEEu8; 32])) }

const NANOCOIN: u64 = 1_000_000_000;
const MIN_BOX: u64 = 360_000;

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap()).collect()
}
fn load_tree(hex: &str) -> ErgoTree { ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).unwrap() }
fn make_token(id: TokenId, amt: u64) -> Token {
    Token { token_id: id, amount: TokenAmount::try_from(amt).unwrap() }
}

fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    // No secrets — submit path is permissionless
    let p = TestProver { secrets: vec![] };
    let m = vec![0u8; 32];
    match p.prove(tree, ctx, &m, &HintsBag::empty()) {
        Ok(pr) => TestVerifier.verify(tree, ctx, pr.proof, &m).map(|v| v.result).unwrap_or(false),
        Err(_) => false,
    }
}

/// Voter box: vote NFT + vYOLO + registers R4-R8
fn make_voter_box(
    vyolo_amount: u64, direction: i64, proposal_id: Vec<u8>,
    cancel_height: i64, submission_deadline: i64, ch: u32,
) -> ErgoBox {
    let tree = load_tree(USERVOTE_TREE_HEX);
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(direction));
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(proposal_id));
    // R6 = voter SigmaProp. For submit path (no sig needed), use trivial sigmaProp(true)
    // Actually R6 is SigmaProp type. Let's use proveDlog of a dummy key.
    // For the submit path tests, R6 doesn't matter (no sig check).
    // For cancel tests we'd need a real key — skip cancel for now (needs DlogProverInput).
    use ergo_lib::ergotree_interpreter::sigma_protocol::private_input::DlogProverInput;
    let dummy_key = DlogProverInput::from_bytes(&[0x01u8; 32]).unwrap();
    let sigma_prop = ergo_lib::ergotree_ir::sigma_protocol::sigma_boolean::SigmaProp::from(
        ergo_lib::ergotree_ir::sigma_protocol::sigma_boolean::ProveDlog::from(*dummy_key.public_image().h.clone())
    );
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(sigma_prop));
    regs.insert(NonMandatoryRegisterId::R7, Constant::from(cancel_height));
    regs.insert(NonMandatoryRegisterId::R8, Constant::from(submission_deadline));

    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(), ergo_tree: tree,
        tokens: Some(vec![make_token(valid_vote_id(), 1), make_token(vyolo_id(), vyolo_amount)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(), creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

fn make_counter_box(vote_deadline: i64, ch: u32) -> ErgoBox {
    let tree = load_tree(COUNTING_TREE_HEX);
    // Counter needs R4 for deadline (used by cancel path data input check)
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(vote_deadline));
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(NANOCOIN).unwrap(), ergo_tree: tree,
        tokens: Some(vec![make_token(counter_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(), creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

// ============================================================
// PHASE 0
// ============================================================

#[test]
fn uservote_tree_round_trips() {
    let t = load_tree(USERVOTE_TREE_HEX);
    assert_eq!(t.sigma_serialize_bytes().unwrap(), hex_to_bytes(USERVOTE_TREE_HEX));
}

#[test]
fn uservote_proposition_parses() {
    load_tree(USERVOTE_TREE_HEX).proposition().expect("parse");
}

// ============================================================
// PATH 2: SUBMIT (permissionless — counting bot)
// ============================================================

#[test]
fn accept_submit_vote_nft_burned() {
    let vote_amount = 10_000 * NANOCOIN;
    let deadline = 200i64;
    let h = 150u32; // within deadline

    let self_box = make_voter_box(vote_amount, 1, vec![0xABu8; 32], 100, deadline, h - 1);
    let counter = make_counter_box(deadline, 0);

    // OUTPUTS: vYOLO returned, vote NFT NOT present (burned)
    let vyolo_return = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: Some(vec![make_token(vyolo_id(), vote_amount)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: h,
    }, TxId::zero(), 0).unwrap();
    let counter_out = make_counter_box(deadline, h);

    // Context: SELF=voter, counter in INPUTS
    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counter));
    let outs: &'static [ErgoBox] = Vec::leak(vec![vyolo_return, counter_out]);
    // Data input for cancel path's eager CONTEXT.dataInputs(0) ValDef
    let di_counter = make_counter_box(deadline, 0);
    let di_ref: &'static ErgoBox = Box::leak(Box::new(di_counter));

    let ctx = Context {
        height: h, self_box: sr, outputs: outs,
        data_inputs: Some(vec![di_ref].try_into().unwrap()),
        inputs: vec![sr, cr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(evaluate(&load_tree(USERVOTE_TREE_HEX), &ctx), "submit with NFT burn should accept");
}

#[test]
fn reject_submit_nft_not_burned() {
    let vote_amount = 10_000 * NANOCOIN;
    let deadline = 200i64;
    let h = 150u32;

    let self_box = make_voter_box(vote_amount, 1, vec![0xABu8; 32], 100, deadline, h - 1);
    let counter = make_counter_box(deadline, 0);

    // ATTACK: vote NFT smuggled into output (at token slot 1, after vYOLO)
    let bad_out = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: Some(vec![make_token(vyolo_id(), vote_amount), make_token(valid_vote_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: h,
    }, TxId::zero(), 0).unwrap();
    let counter_out = make_counter_box(deadline, h);

    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counter));
    let outs: &'static [ErgoBox] = Vec::leak(vec![bad_out, counter_out]);
    let di_counter = make_counter_box(deadline, 0);
    let di_ref: &'static ErgoBox = Box::leak(Box::new(di_counter));

    let ctx = Context {
        height: h, self_box: sr, outputs: outs,
        data_inputs: Some(vec![di_ref].try_into().unwrap()),
        inputs: vec![sr, cr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(!evaluate(&load_tree(USERVOTE_TREE_HEX), &ctx), "NFT at slot 1 should be caught by nested forall");
}

#[test]
fn reject_submit_past_deadline() {
    let vote_amount = 10_000 * NANOCOIN;
    let deadline = 200i64;
    let h = 201u32; // PAST deadline

    let self_box = make_voter_box(vote_amount, 1, vec![0xABu8; 32], 100, deadline, h - 1);
    let counter = make_counter_box(deadline, 0);

    let vyolo_return = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: Some(vec![make_token(vyolo_id(), vote_amount)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: h,
    }, TxId::zero(), 0).unwrap();
    let counter_out = make_counter_box(deadline, h);

    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counter));
    let outs: &'static [ErgoBox] = Vec::leak(vec![vyolo_return, counter_out]);
    let di_counter = make_counter_box(deadline, 0);
    let di_ref: &'static ErgoBox = Box::leak(Box::new(di_counter));

    let ctx = Context {
        height: h, self_box: sr, outputs: outs,
        data_inputs: Some(vec![di_ref].try_into().unwrap()),
        inputs: vec![sr, cr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(!evaluate(&load_tree(USERVOTE_TREE_HEX), &ctx), "submit past deadline should reject");
}

#[test]
fn reject_submit_vyolo_stolen() {
    let vote_amount = 10_000 * NANOCOIN;
    let deadline = 200i64;
    let h = 150u32;

    let self_box = make_voter_box(vote_amount, 1, vec![0xABu8; 32], 100, deadline, h - 1);
    let counter = make_counter_box(deadline, 0);

    // ATTACK: vYOLO not returned (or less than locked amount)
    let partial_return = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(), ergo_tree: load_tree(TRUE_TREE_HEX),
        tokens: Some(vec![make_token(vyolo_id(), vote_amount / 2)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: h,
    }, TxId::zero(), 0).unwrap();
    let counter_out = make_counter_box(deadline, h);

    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let cr: &'static ErgoBox = Box::leak(Box::new(counter));
    let outs: &'static [ErgoBox] = Vec::leak(vec![partial_return, counter_out]);
    let di_counter = make_counter_box(deadline, 0);
    let di_ref: &'static ErgoBox = Box::leak(Box::new(di_counter));

    let ctx = Context {
        height: h, self_box: sr, outputs: outs,
        data_inputs: Some(vec![di_ref].try_into().unwrap()),
        inputs: vec![sr, cr].try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    };
    assert!(!evaluate(&load_tree(USERVOTE_TREE_HEX), &ctx), "stolen vYOLO should reject");
}
