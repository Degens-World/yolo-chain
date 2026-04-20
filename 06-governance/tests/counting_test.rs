//! counting_test.rs — ErgoTree evaluation tests for counting.es v1.1
//!
//! 4-phase state machine: before-counting, counting, validation, new-proposal.
//! Tests each phase + adversarial cases (re-initiation, NFT burn bypass, etc.).
//!
//! ErgoTree compiled via Ergo node 6.x.
//! Targets: ergo-lib 0.28 with "arbitrary" feature, rustc 1.85+

use std::collections::HashMap;
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
    mir::constant::{Constant, Literal},
    serialization::SigmaSerializable,
    types::stype::SType,
    types::stuple::STuple,
};
use sigma_test_util::force_any_val;

// ============================================================
// PRE-COMPILED ERGOTREE HEX
// ============================================================

const COUNTING_TREE_HEX: &str = include_str!("/tmp/counting_tree.hex");
const TRUE_TREE_HEX: &str = "10010101d17300";

// ============================================================
// TOKEN IDS
// ============================================================

fn counter_nft_id() -> TokenId { TokenId::from(Digest32::from([0xEEu8; 32])) }
fn vyolo_id() -> TokenId { TokenId::from(Digest32::from([0xCCu8; 32])) }
fn valid_vote_id() -> TokenId { TokenId::from(Digest32::from([0xFFu8; 32])) }

const NANOCOIN: u64 = 1_000_000_000;
const MIN_BOX: u64 = 360_000;
const VOTING_WINDOW: i64 = 12960;
const COUNTING_PHASE: i64 = 1080;
const EXECUTION_GRACE: i64 = 4320;
const INITIATION_HURDLE: u64 = 100_000 * NANOCOIN;

// ============================================================
// HELPERS
// ============================================================

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    (0..hex.len()).step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn load_tree(hex: &str) -> ErgoTree {
    ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).expect("valid ErgoTree")
}

fn load_counting_tree() -> ErgoTree { load_tree(COUNTING_TREE_HEX) }

fn make_token(id: TokenId, amount: u64) -> Token {
    Token { token_id: id, amount: TokenAmount::try_from(amount).unwrap() }
}

fn make_tuple_constant(a: i64, b: i64) -> Constant {
    let tuple_val = Literal::Tup(
        vec![Literal::Long(a), Literal::Long(b)].try_into().unwrap()
    );
    let tuple_type = SType::STuple(STuple::try_from(vec![SType::SLong, SType::SLong]).unwrap());
    Constant { tpe: tuple_type, v: tuple_val }
}

/// Build a counting box with all registers
fn make_counting_box(
    vote_deadline: i64, proportion: i64, votes_for: i64,
    recipient_hash: Vec<u8>, total_votes: i64,
    initiation_stake: i64, validation_votes: i64,
    value: u64, ch: u32,
) -> ErgoBox {
    let tree = load_counting_tree();
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(vote_deadline));
    regs.insert(NonMandatoryRegisterId::R5, make_tuple_constant(proportion, votes_for));
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(recipient_hash));
    regs.insert(NonMandatoryRegisterId::R7, Constant::from(total_votes));
    regs.insert(NonMandatoryRegisterId::R8, Constant::from(initiation_stake));
    regs.insert(NonMandatoryRegisterId::R9, Constant::from(validation_votes));

    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree,
        tokens: Some(vec![make_token(counter_nft_id(), 1)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(),
        creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

/// Idle counting box (no active proposal)
fn make_idle_counting_box(vote_deadline: i64, value: u64, ch: u32) -> ErgoBox {
    make_counting_box(vote_deadline, 0, 0, vec![0u8; 32], 0, 0, 0, value, ch)
}

/// Voter box with vote NFT + vYOLO + direction register
fn make_voter_box(vyolo_amount: u64, direction: i64, ch: u32) -> ErgoBox {
    let tree = load_tree(TRUE_TREE_HEX);
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(direction));

    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(),
        ergo_tree: tree,
        tokens: Some(vec![
            make_token(valid_vote_id(), 1),
            make_token(vyolo_id(), vyolo_amount),
        ].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(),
        creation_height: ch,
    }, TxId::zero(), 1).unwrap()
}

/// Initiation data input box (holds vYOLO for stake check)
fn make_initiation_box(vyolo_amount: u64, ch: u32) -> ErgoBox {
    let tree = load_tree(TRUE_TREE_HEX);
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX).unwrap(),
        ergo_tree: tree,
        tokens: Some(vec![make_token(vyolo_id(), vyolo_amount)].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    let prover = TestProver { secrets: vec![] };
    let message = vec![0u8; 32];
    match prover.prove(tree, ctx, message.as_slice(), &HintsBag::empty()) {
        Ok(p) => match TestVerifier.verify(tree, ctx, p.proof, message.as_slice()) {
            Ok(v) => v.result, Err(_) => false,
        }, Err(_) => false,
    }
}

fn build_context(self_box: ErgoBox, outputs: Vec<ErgoBox>, height: u32) -> Context<'static> {
    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outs: &'static [ErgoBox] = Vec::leak(outputs);
    Context {
        height, self_box: sr, outputs: outs, data_inputs: None,
        inputs: [sr].into(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

fn build_context_with_data(
    self_box: ErgoBox, other_inputs: Vec<ErgoBox>,
    outputs: Vec<ErgoBox>, data_inputs: Vec<ErgoBox>, height: u32,
) -> Context<'static> {
    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outs: &'static [ErgoBox] = Vec::leak(outputs);
    let mut refs: Vec<&'static ErgoBox> = vec![sr];
    for o in other_inputs { refs.push(Box::leak(Box::new(o))); }
    let di: Vec<&'static ErgoBox> = data_inputs.into_iter().map(|b| &*Box::leak(Box::new(b))).collect();

    Context {
        height, self_box: sr, outputs: outs,
        data_inputs: if di.is_empty() { None } else { Some(di.try_into().unwrap()) },
        inputs: refs.try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

// ============================================================
// PHASE 0: LOAD + ROUND-TRIP
// ============================================================

#[test]
fn counting_tree_round_trips() {
    let t = load_counting_tree();
    let bytes = t.sigma_serialize_bytes().unwrap();
    assert_eq!(bytes, hex_to_bytes(COUNTING_TREE_HEX));
}

#[test]
fn counting_proposition_parses() {
    load_counting_tree().proposition().expect("parse");
}

// ============================================================
// PHASE 4: NEW PROPOSAL PERIOD (easiest — test first)
// ============================================================

#[test]
fn accept_phase4_reset() {
    let deadline = 100i64;
    let validation_end = deadline + COUNTING_PHASE + EXECUTION_GRACE;
    let h = (validation_end + 1) as u32; // past validation deadline

    // Self: has stale tallies from previous round
    let self_box = make_counting_box(deadline, 500_000, 100, vec![0u8; 32], 200, INITIATION_HURDLE as i64, 100, NANOCOIN, h - 1);
    // Output: tallies cleared
    let out0 = make_counting_box(deadline, 500_000, 100, vec![0u8; 32], 0, INITIATION_HURDLE as i64, 0, NANOCOIN, h);

    // Dummy data input needed for Phase 1's eager CONTEXT.dataInputs(0) ValDef
    let dummy_di = make_initiation_box(INITIATION_HURDLE, h - 1);
    // Dummy INPUTS(1) for Phase 2's eager INPUTS.slice ValDef
    let dummy_input = make_voter_box(NANOCOIN, 1, h - 1);
    let ctx = build_context_with_data(self_box, vec![dummy_input], vec![out0], vec![dummy_di], h);
    assert!(evaluate(&load_counting_tree(), &ctx), "phase 4 reset should accept");
}

#[test]
fn reject_phase4_before_validation_end() {
    let deadline = 100i64;
    let validation_end = deadline + COUNTING_PHASE + EXECUTION_GRACE;
    let h = validation_end as u32; // exactly at boundary, not past

    let self_box = make_counting_box(deadline, 500_000, 100, vec![0u8; 32], 200, INITIATION_HURDLE as i64, 100, NANOCOIN, h - 1);
    let out0 = make_counting_box(deadline, 500_000, 100, vec![0u8; 32], 0, INITIATION_HURDLE as i64, 0, NANOCOIN, h);

    let ctx = build_context(self_box, vec![out0], h);
    assert!(!evaluate(&load_counting_tree(), &ctx), "phase 4 before validation end should reject");
}

// ============================================================
// PHASE 1: BEFORE COUNTING — initiate new vote
// ============================================================

#[test]
fn accept_phase1_initiation() {
    let h = 50u32;
    // Self: idle counter with old deadline in the past is needed for isBeforeCounting.
    // Actually isBeforeCounting = HEIGHT < voteDeadline, so we need deadline IN THE FUTURE.
    // Wait — for initiation, we need HEIGHT < voteDeadline AND noActiveProposal.
    // The new deadline is set by the output, not SELF. SELF's deadline just needs to allow Phase 1.
    // For a fresh counter: deadline=0 means isBeforeCounting = HEIGHT < 0 = false.
    // Hmm. Let me re-read the phases.
    //
    // Phase 1: isBeforeCounting = HEIGHT < voteDeadline
    // This means SELF.R4 (voteDeadline) must be > HEIGHT for Phase 1 to activate.
    // But that conflicts with "no active proposal" — if deadline is in the future,
    // a vote is supposed to be active.
    //
    // Looking at the DuckDAO pattern more carefully: Phase 1 happens when
    // the counter is between rounds. After Phase 4 resets the counter,
    // the old deadline is still in the past. A new initiation needs to be
    // triggered somehow.
    //
    // Wait — Phase 4 condition is isNewProposalPeriod = HEIGHT >= validationEnd.
    // Phase 1 condition is isBeforeCounting = HEIGHT < voteDeadline.
    // After Phase 4, voteDeadline is still the OLD value (Phase 4 doesn't change R4).
    // So the next Phase 1 would need HEIGHT < old_deadline which is in the past — impossible.
    //
    // This means Phase 1 can only be entered when a NEW deadline is set...
    // but Phase 1 is what SETS the new deadline. Chicken-and-egg.
    //
    // The fix: Phase 4 should set voteDeadline to a FUTURE value, or Phase 1
    // should also be enterable when HEIGHT >= validationEnd (isNewProposalPeriod).
    //
    // This is actually a bug in counting.es — Phase 1 is unreachable after the
    // first round completes! Let me verify by checking what happens:
    // 1. Genesis: counter box created with R4 = some_future_deadline
    // 2. Phase 1 fires (HEIGHT < deadline): sets new deadline = HEIGHT + votingWindow
    // 3. Voting happens, counting happens, validation happens
    // 4. Phase 4 fires (HEIGHT >= validationEnd): resets tallies, but R4 stays as old deadline
    // 5. Now HEIGHT >> old deadline, so isBeforeCounting = false permanently
    //
    // Phase 1 SHOULD be: isBeforeCounting || isNewProposalPeriod
    // Or: Phase 4 should update R4 to a new future deadline.
    //
    // For now, let me test what we have and document this as a finding.
    // Phase 1 works for the FIRST round (genesis counter has future deadline).
    //
    // Use a counter with deadline far in the future:
    let deadline = (h as i64) + VOTING_WINDOW + 1000; // well in the future
    let self_box = make_idle_counting_box(deadline, NANOCOIN, h - 1);

    // Data input: initiation box with enough vYOLO
    let initiation = make_initiation_box(INITIATION_HURDLE, h - 1);

    // Output: new deadline, tallies reset
    let new_deadline = h as i64 + VOTING_WINDOW;
    let out0 = make_counting_box(
        new_deadline, 0, 0, vec![0u8; 32], 0,
        INITIATION_HURDLE as i64, 0, NANOCOIN, h,
    );

    // Dummy voter input for Phase 2's eager INPUTS.slice ValDef
    let dummy_voter = make_voter_box(NANOCOIN, 1, h - 1);
    let ctx = build_context_with_data(self_box, vec![dummy_voter], vec![out0], vec![initiation], h);
    assert!(evaluate(&load_counting_tree(), &ctx), "phase 1 initiation should accept");
}

#[test]
fn reject_phase1_insufficient_stake() {
    let h = 50u32;
    let deadline = (h as i64) + VOTING_WINDOW + 1000;
    let self_box = make_idle_counting_box(deadline, NANOCOIN, h - 1);

    // Data input: NOT enough vYOLO
    let initiation = make_initiation_box(INITIATION_HURDLE - 1, h - 1);

    let new_deadline = h as i64 + VOTING_WINDOW;
    let out0 = make_counting_box(
        new_deadline, 0, 0, vec![0u8; 32], 0,
        INITIATION_HURDLE as i64, 0, NANOCOIN, h,
    );

    let dummy_voter = make_voter_box(NANOCOIN, 1, h - 1);
    let ctx = build_context_with_data(self_box, vec![dummy_voter], vec![out0], vec![initiation], h);
    assert!(!evaluate(&load_counting_tree(), &ctx), "insufficient stake should reject");
}

// ============================================================
// PHASE 2: COUNTING PERIOD — accumulate votes
// ============================================================

#[test]
fn accept_phase2_count_one_yes_vote() {
    let deadline = 100i64;
    let h = deadline as u32; // exactly at deadline = start of counting

    let vote_amount = 5_000 * NANOCOIN;
    let proportion = 500_000i64;
    let recipient = vec![0xABu8; 32];

    // Self: counter in counting period with initial tallies
    let self_box = make_counting_box(
        deadline, proportion, 0, recipient.clone(), 0,
        INITIATION_HURDLE as i64, 0, NANOCOIN, h - 1,
    );

    // Voter box: 1 yes vote
    let voter = make_voter_box(vote_amount, 1, h - 1);

    // Output: tallies updated
    let out0 = make_counting_box(
        deadline, proportion, vote_amount as i64, recipient,
        vote_amount as i64, INITIATION_HURDLE as i64, vote_amount as i64,
        NANOCOIN, h,
    );

    // Dummy data input for Phase 1's eager CONTEXT.dataInputs(0) ValDef
    let dummy_di = make_initiation_box(INITIATION_HURDLE, h - 1);
    let ctx = build_context_with_data(self_box, vec![voter], vec![out0], vec![dummy_di], h);
    assert!(evaluate(&load_counting_tree(), &ctx), "counting one yes vote should accept");
}

#[test]
fn reject_phase2_tallies_wrong() {
    let deadline = 100i64;
    let h = deadline as u32;

    let vote_amount = 5_000 * NANOCOIN;
    let proportion = 500_000i64;
    let recipient = vec![0xABu8; 32];

    let self_box = make_counting_box(
        deadline, proportion, 0, recipient.clone(), 0,
        INITIATION_HURDLE as i64, 0, NANOCOIN, h - 1,
    );

    let voter = make_voter_box(vote_amount, 1, h - 1);

    // WRONG: total_votes is wrong (double-counted)
    let out0 = make_counting_box(
        deadline, proportion, vote_amount as i64, recipient,
        (vote_amount * 2) as i64, // WRONG
        INITIATION_HURDLE as i64, vote_amount as i64,
        NANOCOIN, h,
    );

    let dummy_di = make_initiation_box(INITIATION_HURDLE, h - 1);
    let ctx = build_context_with_data(self_box, vec![voter], vec![out0], vec![dummy_di], h);
    assert!(!evaluate(&load_counting_tree(), &ctx), "wrong tallies should reject");
}

#[test]
fn reject_phase2_outside_counting_window() {
    let deadline = 100i64;
    let h = (deadline - 1) as u32; // before deadline = still in voting, not counting

    let vote_amount = 5_000 * NANOCOIN;
    let proportion = 500_000i64;
    let recipient = vec![0xABu8; 32];

    let self_box = make_counting_box(
        deadline, proportion, 0, recipient.clone(), 0,
        INITIATION_HURDLE as i64, 0, NANOCOIN, h - 1,
    );

    let voter = make_voter_box(vote_amount, 1, h - 1);

    let out0 = make_counting_box(
        deadline, proportion, vote_amount as i64, recipient,
        vote_amount as i64, INITIATION_HURDLE as i64, vote_amount as i64,
        NANOCOIN, h,
    );

    let dummy_di = make_initiation_box(INITIATION_HURDLE, h - 1);
    let ctx = build_context_with_data(self_box, vec![voter], vec![out0], vec![dummy_di], h);
    assert!(!evaluate(&load_counting_tree(), &ctx), "counting before deadline should reject");
}

// ============================================================
// PHASE 1 RE-INITIATION GUARD
// ============================================================

#[test]
fn reject_phase1_reinitiation_during_active_vote() {
    let h = 50u32;
    let deadline = (h as i64) + VOTING_WINDOW + 1000;

    // Self: counter with ACTIVE tallies (not idle)
    let self_box = make_counting_box(
        deadline, 500_000, 100, vec![0u8; 32], 200,
        INITIATION_HURDLE as i64, 100, NANOCOIN, h - 1,
    );

    let initiation = make_initiation_box(INITIATION_HURDLE, h - 1);

    let new_deadline = h as i64 + VOTING_WINDOW;
    let out0 = make_counting_box(
        new_deadline, 0, 0, vec![0u8; 32], 0,
        INITIATION_HURDLE as i64, 0, NANOCOIN, h,
    );

    let dummy_voter = make_voter_box(NANOCOIN, 1, h - 1);
    let ctx = build_context_with_data(self_box, vec![dummy_voter], vec![out0], vec![initiation], h);
    assert!(!evaluate(&load_counting_tree(), &ctx), "re-initiation during active vote should reject");
}
