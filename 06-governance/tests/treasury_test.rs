//! treasury_test.rs — ErgoTree evaluation tests for governance treasury v1.1.
//!
//! Tests the governance-controlled treasury (dormant until migration from multisig).
//! Paths: deposit, withdrawal (split-math), new-treasury-mode.
//! No multisig — all spending gated by proposal state token (qty=2).
//!
//! ErgoTree compiled via Ergo node 6.x.
//! Targets: ergo-lib 0.28 with "arbitrary" feature, rustc 1.85+

use std::collections::HashMap;
use blake2::{digest::consts::U32, Blake2b, Digest};
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

type Blake2b256 = Blake2b<U32>;

// ============================================================
// PRE-COMPILED ERGOTREE HEX
// ============================================================

// Governance Treasury v1.1 (514 bytes)
const TREASURY_TREE_HEX: &str = include_str!("/tmp/treasury_tree.hex");

// Proposal contract (268 bytes) — for building proposal boxes
const PROPOSAL_TREE_HEX: &str = include_str!("/tmp/proposal_tree.hex");

// sigmaProp(true) — user/recipient script
const TRUE_TREE_HEX: &str = "10010101d17300";

// ============================================================
// TOKEN IDS (matching compile-time placeholders)
// ============================================================

fn treasury_nft_id() -> TokenId { TokenId::from(Digest32::from([0xDDu8; 32])) }
// proposal_token_id uses 0xFF (matches PROPOSAL_TOKEN_PLACEHOLDER = 'ff'*32)
fn proposal_token_id() -> TokenId { TokenId::from(Digest32::from([0xFFu8; 32])) }

const NANOCOIN: u64 = 1_000_000_000;
const MIN_BOX: u64 = 360_000;
const DENOM: u64 = 10_000_000;

// ============================================================
// HELPERS
// ============================================================

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn load_tree(hex: &str) -> ErgoTree {
    ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).expect("valid ErgoTree")
}

fn load_treasury_tree() -> ErgoTree { load_tree(TREASURY_TREE_HEX) }
fn load_proposal_tree() -> ErgoTree { load_tree(PROPOSAL_TREE_HEX) }
fn load_true_tree() -> ErgoTree { load_tree(TRUE_TREE_HEX) }

fn proposition_hash(tree: &ErgoTree) -> Vec<u8> {
    let bytes = tree.sigma_serialize_bytes().unwrap();
    let mut h = Blake2b256::new();
    h.update(&bytes);
    h.finalize().to_vec()
}

fn make_token(id: TokenId, amount: u64) -> Token {
    Token { token_id: id, amount: TokenAmount::try_from(amount).unwrap() }
}

fn make_box(tree: &ErgoTree, value: u64, tokens: Vec<Token>, creation_height: u32) -> ErgoBox {
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: if tokens.is_empty() { None } else { Some(tokens.try_into().unwrap()) },
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    }, TxId::zero(), 0).unwrap()
}

fn make_box_with_regs(
    tree: &ErgoTree, value: u64, tokens: Vec<Token>,
    regs: NonMandatoryRegisters, creation_height: u32,
) -> ErgoBox {
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: if tokens.is_empty() { None } else { Some(tokens.try_into().unwrap()) },
        additional_registers: regs,
        creation_height,
    }, TxId::zero(), 0).unwrap()
}

/// Treasury box: value + treasury NFT
fn make_treasury_box(value: u64, ch: u32) -> ErgoBox {
    make_box(&load_treasury_tree(), value, vec![make_token(treasury_nft_id(), 1)], ch)
}

/// Proposal box with state token at given qty, R4=(proportion,0), R5=recipient_hash
fn make_proposal_box(proportion: i64, recipient_hash: Vec<u8>, token_qty: u64, ch: u32) -> ErgoBox {
    let mut regs = HashMap::new();
    // R4 as tuple (Long, Long) — need to use Constant::Tup
    // Actually, from the 02/ pattern, individual Long registers work.
    // But our contract reads R4[(Long, Long)].get — a tuple.
    // sigma-rust Constant doesn't support (i64,i64) directly from From.
    // We need to construct the tuple Constant manually.
    use ergo_lib::ergotree_ir::mir::constant::Literal;
    use ergo_lib::ergotree_ir::types::stype::SType;
    use ergo_lib::ergotree_ir::types::stuple::STuple;

    let tuple_val = Literal::Tup(
        vec![
            Literal::Long(proportion),
            Literal::Long(0i64),
        ].try_into().unwrap()
    );
    let tuple_type = SType::STuple(STuple::try_from(vec![SType::SLong, SType::SLong]).unwrap());
    let tuple_const = Constant { tpe: tuple_type, v: tuple_val };

    regs.insert(NonMandatoryRegisterId::R4, tuple_const);
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(recipient_hash));

    make_box_with_regs(
        &load_proposal_tree(), MIN_BOX,
        vec![make_token(proposal_token_id(), token_qty)],
        NonMandatoryRegisters::new(regs).unwrap(), ch,
    )
}

fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    let prover = TestProver { secrets: vec![] };
    let message = vec![0u8; 32];
    match prover.prove(tree, ctx, message.as_slice(), &HintsBag::empty()) {
        Ok(p) => match TestVerifier.verify(tree, ctx, p.proof, message.as_slice()) {
            Ok(v) => v.result,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Build context where SELF = treasury at given position in inputs.
/// For deposit: treasury is INPUTS(0) (SELF).
/// For withdrawal: proposal is INPUTS(0), treasury is INPUTS(1+).
fn build_context_deposit(treasury: ErgoBox, outputs: Vec<ErgoBox>, height: u32) -> Context<'static> {
    let self_ref: &'static ErgoBox = Box::leak(Box::new(treasury));
    let outs: &'static [ErgoBox] = Vec::leak(outputs);
    let inputs: [&'static ErgoBox; 1] = [self_ref];

    Context {
        height,
        self_box: self_ref,
        outputs: outs,
        data_inputs: None,
        inputs: inputs.into(),
        pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

fn build_context_withdrawal(
    proposal: ErgoBox, treasury: ErgoBox, outputs: Vec<ErgoBox>, height: u32,
) -> Context<'static> {
    let treasury_ref: &'static ErgoBox = Box::leak(Box::new(treasury));
    let proposal_ref: &'static ErgoBox = Box::leak(Box::new(proposal));
    let outs: &'static [ErgoBox] = Vec::leak(outputs);
    let inputs = vec![proposal_ref, treasury_ref];

    Context {
        height,
        self_box: treasury_ref,
        outputs: outs,
        data_inputs: None,
        inputs: inputs.try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

/// Split-math in Rust (mirrors ErgoScript)
fn split_math_awarded(value: u64, proportion: u64) -> u64 {
    let whole = (value / DENOM) * proportion;
    let remainder = ((value % DENOM) * proportion) / DENOM;
    whole + remainder
}

// ============================================================
// PHASE 0: LOAD + ROUND-TRIP
// ============================================================

#[test]
fn treasury_tree_round_trips() {
    let tree = load_treasury_tree();
    let bytes = tree.sigma_serialize_bytes().unwrap();
    assert_eq!(bytes, hex_to_bytes(TREASURY_TREE_HEX));
}

#[test]
fn treasury_proposition_parses() {
    load_treasury_tree().proposition().expect("parse");
}

// ============================================================
// PATH 1: DEPOSIT
// ============================================================

#[test]
fn accept_deposit() {
    let treasury_in = make_treasury_box(100 * NANOCOIN, 0);
    let treasury_out = make_treasury_box(150 * NANOCOIN, 1);
    let ctx = build_context_deposit(treasury_in, vec![treasury_out], 1);
    assert!(evaluate(&load_treasury_tree(), &ctx), "deposit should accept");
}

#[test]
fn reject_deposit_value_decrease() {
    let treasury_in = make_treasury_box(100 * NANOCOIN, 0);
    let treasury_out = make_treasury_box(50 * NANOCOIN, 1);
    let ctx = build_context_deposit(treasury_in, vec![treasury_out], 1);
    assert!(!evaluate(&load_treasury_tree(), &ctx), "deposit with value decrease should reject");
}

#[test]
fn reject_deposit_nft_stolen() {
    let tree = load_treasury_tree();
    let treasury_in = make_treasury_box(100 * NANOCOIN, 0);
    // Output: no NFT
    let bad_out = make_box(&tree, 150 * NANOCOIN, vec![], 1);
    let ctx = build_context_deposit(treasury_in, vec![bad_out], 1);
    assert!(!evaluate(&tree, &ctx), "deposit without NFT should reject");
}

#[test]
fn reject_deposit_script_changed() {
    let treasury_in = make_treasury_box(100 * NANOCOIN, 0);
    // Output: different script
    let bad_out = make_box(
        &load_true_tree(), 150 * NANOCOIN,
        vec![make_token(treasury_nft_id(), 1)], 1,
    );
    let ctx = build_context_deposit(treasury_in, vec![bad_out], 1);
    assert!(!evaluate(&load_treasury_tree(), &ctx), "deposit with script change should reject");
}

// ============================================================
// PATH 2: WITHDRAWAL (split-math)
// ============================================================

#[test]
fn accept_withdrawal_5_percent() {
    let tree = load_treasury_tree();
    let recipient_tree = load_true_tree();
    let rh = proposition_hash(&recipient_tree);

    let treasury_value = 1_000_000 * NANOCOIN;
    let proportion = 500_000u64; // 5% of 10M
    let awarded = split_math_awarded(treasury_value, proportion);

    let proposal = make_proposal_box(proportion as i64, rh, 2, 0); // qty=2 = passed
    let treasury_in = make_treasury_box(treasury_value, 0);
    let treasury_out = make_treasury_box(treasury_value - awarded, 1);
    let recipient_out = make_box(&recipient_tree, awarded, vec![], 1);

    let ctx = build_context_withdrawal(proposal, treasury_in, vec![treasury_out, recipient_out], 1);
    assert!(evaluate(&tree, &ctx), "5% withdrawal should accept");
}

#[test]
fn reject_withdrawal_overclaim() {
    let tree = load_treasury_tree();
    let recipient_tree = load_true_tree();
    let rh = proposition_hash(&recipient_tree);

    let treasury_value = 1_000_000 * NANOCOIN;
    let proportion = 500_000u64;
    let awarded = split_math_awarded(treasury_value, proportion);

    let proposal = make_proposal_box(proportion as i64, rh, 2, 0);
    let treasury_in = make_treasury_box(treasury_value, 0);
    let treasury_out = make_treasury_box(treasury_value - awarded - NANOCOIN, 1);
    let recipient_out = make_box(&recipient_tree, awarded + NANOCOIN, vec![], 1);

    let ctx = build_context_withdrawal(proposal, treasury_in, vec![treasury_out, recipient_out], 1);
    assert!(!evaluate(&tree, &ctx), "overclaim should reject");
}

#[test]
fn reject_withdrawal_proposal_not_passed() {
    let tree = load_treasury_tree();
    let recipient_tree = load_true_tree();
    let rh = proposition_hash(&recipient_tree);

    let treasury_value = 1_000_000 * NANOCOIN;
    let proportion = 500_000u64;
    let awarded = split_math_awarded(treasury_value, proportion);

    let proposal = make_proposal_box(proportion as i64, rh, 1, 0); // qty=1 = NOT passed
    let treasury_in = make_treasury_box(treasury_value, 0);
    let treasury_out = make_treasury_box(treasury_value - awarded, 1);
    let recipient_out = make_box(&recipient_tree, awarded, vec![], 1);

    let ctx = build_context_withdrawal(proposal, treasury_in, vec![treasury_out, recipient_out], 1);
    assert!(!evaluate(&tree, &ctx), "proposal not passed (qty=1) should reject");
}

#[test]
fn reject_withdrawal_wrong_recipient() {
    let tree = load_treasury_tree();
    let recipient_tree = load_true_tree();
    let rh = proposition_hash(&recipient_tree);

    let treasury_value = 1_000_000 * NANOCOIN;
    let proportion = 500_000u64;
    let awarded = split_math_awarded(treasury_value, proportion);

    let proposal = make_proposal_box(proportion as i64, rh, 2, 0);
    let treasury_in = make_treasury_box(treasury_value, 0);
    let treasury_out = make_treasury_box(treasury_value - awarded, 1);
    // Wrong recipient — send to treasury script instead
    let wrong_out = make_box(&tree, awarded, vec![], 1);

    let ctx = build_context_withdrawal(proposal, treasury_in, vec![treasury_out, wrong_out], 1);
    assert!(!evaluate(&tree, &ctx), "wrong recipient should reject");
}

// ============================================================
// PATH 3: NEW-TREASURY-MODE
// ============================================================

#[test]
fn accept_new_treasury_mode() {
    let tree = load_treasury_tree();
    let new_tree = load_true_tree();
    let rh = proposition_hash(&new_tree);

    let treasury_value = 500_000 * NANOCOIN;
    let proportion = 10_000_000i64; // 100% = new-treasury-mode

    let proposal = make_proposal_box(proportion, rh, 2, 0);
    let treasury_in = make_treasury_box(treasury_value, 0);
    let new_treasury = make_box(&new_tree, treasury_value, vec![make_token(treasury_nft_id(), 1)], 1);
    // Dummy OUTPUTS(1) for eager ValDef from withdrawal path (reads OUTPUTS(1).propositionBytes)
    let dummy = make_box(&load_true_tree(), MIN_BOX, vec![], 1);

    let ctx = build_context_withdrawal(proposal, treasury_in, vec![new_treasury, dummy], 1);
    assert!(evaluate(&tree, &ctx), "new-treasury-mode should accept");
}

#[test]
fn reject_new_treasury_value_stolen() {
    let tree = load_treasury_tree();
    let new_tree = load_true_tree();
    let rh = proposition_hash(&new_tree);

    let treasury_value = 500_000 * NANOCOIN;
    let proportion = 10_000_000i64;

    let proposal = make_proposal_box(proportion, rh, 2, 0);
    let treasury_in = make_treasury_box(treasury_value, 0);
    // New treasury gets less than full value
    let new_treasury = make_box(&new_tree, treasury_value - NANOCOIN, vec![make_token(treasury_nft_id(), 1)], 1);
    let dummy = make_box(&load_true_tree(), MIN_BOX, vec![], 1);

    let ctx = build_context_withdrawal(proposal, treasury_in, vec![new_treasury, dummy], 1);
    assert!(!evaluate(&tree, &ctx), "new-treasury with stolen value should reject");
}
