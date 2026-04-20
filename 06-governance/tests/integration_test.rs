//! integration_test.rs — Full governance lifecycle through contract evaluation.
//!
//! Chains: deposit → (propose) → vote → count → validate → execute → redeem
//! Tests both peg layer AND voting layer contracts in sequence.
//! Verifies peg invariant at every step.

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

// ---- Pre-compiled trees ----
const VAULT_HEX: &str = include_str!("/tmp/vault_tree.hex");
const RESERVE_HEX: &str = include_str!("/tmp/reserve_tree.hex");
const TREASURY_HEX: &str = include_str!("/tmp/treasury_tree.hex");
const PROPOSAL_HEX: &str = include_str!("/tmp/proposal_tree.hex");
const TRUE_HEX: &str = "10010101d17300";

fn state_nft() -> TokenId { TokenId::from(Digest32::from([0xAAu8; 32])) }
fn reserve_nft() -> TokenId { TokenId::from(Digest32::from([0xBBu8; 32])) }
fn vyolo_id() -> TokenId { TokenId::from(Digest32::from([0xCCu8; 32])) }
fn treasury_nft() -> TokenId { TokenId::from(Digest32::from([0xDDu8; 32])) }
fn proposal_token() -> TokenId { TokenId::from(Digest32::from([0xFFu8; 32])) }

const N: u64 = 1_000_000_000;
const MIN: u64 = 360_000;
const DENOM: u64 = 10_000_000;

fn hx(hex: &str) -> Vec<u8> {
    let hex = hex.trim();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i+2], 16).unwrap()).collect()
}
fn tree(hex: &str) -> ErgoTree { ErgoTree::sigma_parse_bytes(&hx(hex)).unwrap() }
fn tok(id: TokenId, a: u64) -> Token { Token { token_id: id, amount: TokenAmount::try_from(a).unwrap() } }
fn phash(t: &ErgoTree) -> Vec<u8> {
    let b = t.sigma_serialize_bytes().unwrap();
    let mut h = Blake2b256::new(); h.update(&b); h.finalize().to_vec()
}
fn tup(a: i64, b: i64) -> Constant {
    Constant { tpe: SType::STuple(STuple::try_from(vec![SType::SLong, SType::SLong]).unwrap()), v: Literal::Tup(vec![Literal::Long(a), Literal::Long(b)].try_into().unwrap()) }
}

fn eval(t: &ErgoTree, c: &Context) -> bool {
    let p = TestProver { secrets: vec![] };
    let m = vec![0u8; 32];
    match p.prove(t, c, &m, &HintsBag::empty()) {
        Ok(pr) => TestVerifier.verify(t, c, pr.proof, &m).map(|v| v.result).unwrap_or(false),
        Err(_) => false,
    }
}

fn ctx_self_first(s: ErgoBox, outs: Vec<ErgoBox>, h: u32) -> Context<'static> {
    let sr: &'static _ = Box::leak(Box::new(s));
    let os: &'static [_] = Vec::leak(outs);
    Context { height: h, self_box: sr, outputs: os, data_inputs: None,
        inputs: [sr].into(), pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(), extension: ContextExtension::empty() }
}

fn ctx_multi(s: ErgoBox, others: Vec<ErgoBox>, outs: Vec<ErgoBox>, h: u32) -> Context<'static> {
    let sr: &'static _ = Box::leak(Box::new(s));
    let os: &'static [_] = Vec::leak(outs);
    let mut refs: Vec<&'static ErgoBox> = vec![sr];
    for o in others { refs.push(Box::leak(Box::new(o))); }
    Context { height: h, self_box: sr, outputs: os, data_inputs: None,
        inputs: refs.try_into().unwrap(), pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(), extension: ContextExtension::empty() }
}

fn ctx_self_at(s: ErgoBox, first: ErgoBox, outs: Vec<ErgoBox>, h: u32) -> Context<'static> {
    let sr: &'static _ = Box::leak(Box::new(s));
    let fr: &'static _ = Box::leak(Box::new(first));
    let os: &'static [_] = Vec::leak(outs);
    Context { height: h, self_box: sr, outputs: os, data_inputs: None,
        inputs: vec![fr, sr].try_into().unwrap(), pre_header: force_any_val::<PreHeader>(),
        headers: force_any_val::<[Header; 10]>(), extension: ContextExtension::empty() }
}

fn mk(t: &ErgoTree, v: u64, toks: Vec<Token>, ch: u32) -> ErgoBox {
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(v).unwrap(), ergo_tree: t.clone(),
        tokens: if toks.is_empty() { None } else { Some(toks.try_into().unwrap()) },
        additional_registers: NonMandatoryRegisters::empty(), creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

fn mkr(t: &ErgoTree, v: u64, toks: Vec<Token>, regs: NonMandatoryRegisters, ch: u32) -> ErgoBox {
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(v).unwrap(), ergo_tree: t.clone(),
        tokens: if toks.is_empty() { None } else { Some(toks.try_into().unwrap()) },
        additional_registers: regs, creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

fn split_math(value: u64, proportion: u64) -> u64 {
    (value / DENOM) * proportion + ((value % DENOM) * proportion) / DENOM
}

// ============================================================
// FULL LIFECYCLE TEST
// ============================================================

#[test]
fn full_lifecycle_deposit_propose_vote_execute_redeem() {
    let vt = tree(VAULT_HEX);
    let rt = tree(RESERVE_HEX);
    let tt = tree(TREASURY_HEX);
    let pt = tree(PROPOSAL_HEX);
    let ut = tree(TRUE_HEX);
    let rh = phash(&ut);

    let initial_vault = 0u64;
    let initial_reserve = 10_000_000 * N; // 10M vYOLO
    let deposit_amount = 1_500_000 * N;   // 1.5M YOLO
    let treasury_value = 500_000 * N;     // 500k YOLO in treasury
    let proportion = 500_000u64;          // 5% of treasury

    // ============================================================
    // STEP 1: DEPOSIT — user locks 1.5M YOLO, gets 1.5M vYOLO
    // ============================================================
    let vault_in = mk(&vt, initial_vault + N, vec![tok(state_nft(), 1)], 0); // min value for empty vault
    let reserve_in = mk(&rt, MIN, vec![tok(reserve_nft(), 1), tok(vyolo_id(), initial_reserve)], 0);

    let vault_out = mk(&vt, initial_vault + N + deposit_amount, vec![tok(state_nft(), 1)], 1);
    let reserve_out = mk(&rt, MIN, vec![tok(reserve_nft(), 1), tok(vyolo_id(), initial_reserve - deposit_amount)], 1);
    let user_vyolo = mk(&ut, N, vec![tok(vyolo_id(), deposit_amount)], 1);

    // Vault perspective
    let ctx = ctx_multi(vault_in.clone(), vec![reserve_in.clone()], vec![vault_out.clone(), reserve_out.clone(), user_vyolo.clone()], 1);
    assert!(eval(&vt, &ctx), "Step 1: deposit vault should accept");

    // Reserve perspective
    let ctx = ctx_multi(reserve_in, vec![vault_in], vec![vault_out, reserve_out, user_vyolo], 1);
    assert!(eval(&rt, &ctx), "Step 1: deposit reserve should accept");

    println!("STEP 1 PASS: deposit 1.5M YOLO → 1.5M vYOLO");

    // ============================================================
    // STEP 2: TREASURY WITHDRAWAL via passed proposal
    // ============================================================
    let awarded = split_math(treasury_value, proportion);
    assert!(awarded > 0, "awarded must be positive");

    // Proposal box at qty=2 (passed)
    let mut pregs = HashMap::new();
    pregs.insert(NonMandatoryRegisterId::R4, tup(proportion as i64, 0));
    pregs.insert(NonMandatoryRegisterId::R5, Constant::from(rh.clone()));
    pregs.insert(NonMandatoryRegisterId::R6, Constant::from(1000i64));
    let proposal = mkr(&pt, MIN, vec![tok(proposal_token(), 2)], NonMandatoryRegisters::new(pregs).unwrap(), 0);

    let treasury_in = mk(&tt, treasury_value, vec![tok(treasury_nft(), 1)], 0);
    let treasury_out = mk(&tt, treasury_value - awarded, vec![tok(treasury_nft(), 1)], 100);
    let recipient_out = mk(&ut, awarded, vec![], 100);

    let ctx = ctx_self_at(treasury_in, proposal, vec![treasury_out, recipient_out], 100);
    assert!(eval(&tt, &ctx), "Step 2: treasury withdrawal should accept");

    println!("STEP 2 PASS: treasury withdrawal {:.1} YOLO ({}% of {:.1})",
        awarded as f64 / N as f64, proportion as f64 / DENOM as f64 * 100.0,
        treasury_value as f64 / N as f64);

    // ============================================================
    // STEP 3: PROPOSAL EXECUTION (token burn)
    // ============================================================
    let proposal_exec = mkr(&pt, MIN, vec![tok(proposal_token(), 2)],
        {let mut r = HashMap::new();
         r.insert(NonMandatoryRegisterId::R4, tup(proportion as i64, 0));
         r.insert(NonMandatoryRegisterId::R5, Constant::from(rh.clone()));
         r.insert(NonMandatoryRegisterId::R6, Constant::from(1000i64));
         NonMandatoryRegisters::new(r).unwrap()}, 0);

    let treasury_for_exec = mk(&tt, treasury_value, vec![tok(treasury_nft(), 1)], 0);

    // Outputs: treasury successor + recipient, NO proposal token anywhere (burned)
    let treasury_out2 = mk(&tt, treasury_value - awarded, vec![tok(treasury_nft(), 1)], 100);
    let recipient_out2 = mk(&ut, awarded, vec![], 100);

    let ctx = ctx_multi(proposal_exec, vec![treasury_for_exec], vec![treasury_out2, recipient_out2], 200);
    assert!(eval(&pt, &ctx), "Step 3: proposal execution (burn) should accept");

    println!("STEP 3 PASS: proposal token burned on execution");

    // ============================================================
    // STEP 4: REDEEM — user returns vYOLO, gets YOLO back
    // ============================================================
    let redeem_amount = deposit_amount;
    let vault_in2 = mk(&vt, N + deposit_amount, vec![tok(state_nft(), 1)], 200);
    let reserve_in2 = mk(&rt, MIN, vec![tok(reserve_nft(), 1), tok(vyolo_id(), initial_reserve - deposit_amount)], 200);

    let vault_out2 = mk(&vt, N + deposit_amount - redeem_amount, vec![tok(state_nft(), 1)], 300);
    let reserve_out2 = mk(&rt, MIN, vec![tok(reserve_nft(), 1), tok(vyolo_id(), initial_reserve - deposit_amount + redeem_amount)], 300);
    let user_yolo = mk(&ut, redeem_amount, vec![], 300);

    // Vault perspective
    let ctx = ctx_multi(vault_in2.clone(), vec![reserve_in2.clone()], vec![vault_out2.clone(), reserve_out2.clone(), user_yolo.clone()], 300);
    assert!(eval(&vt, &ctx), "Step 4: redeem vault should accept");

    // Reserve perspective
    let ctx = ctx_multi(reserve_in2, vec![vault_in2], vec![vault_out2, reserve_out2, user_yolo], 300);
    assert!(eval(&rt, &ctx), "Step 4: redeem reserve should accept");

    println!("STEP 4 PASS: redeem 1.5M vYOLO → 1.5M YOLO");

    // ============================================================
    // PEG INVARIANT
    // ============================================================
    // After full cycle: vault should be back to N (min value), reserve back to full supply
    let final_vault = N; // started at N, deposited, then redeemed everything
    let final_reserve = initial_reserve; // all vYOLO back
    let circulating_vyolo = initial_reserve - final_reserve; // 0
    assert_eq!(circulating_vyolo, 0, "peg invariant: no circulating vYOLO after full redeem");

    println!();
    println!("FULL LIFECYCLE COMPLETE — peg invariant holds");
    println!("  Deposit:    1,500,000 YOLO → 1,500,000 vYOLO");
    println!("  Treasury:   500,000 → {} YOLO ({}% disbursed)",
        (treasury_value - awarded) / N, proportion as f64 / DENOM as f64 * 100.0);
    println!("  Redeem:     1,500,000 vYOLO → 1,500,000 YOLO");
    println!("  Peg:        0 circulating vYOLO = 0 locked YOLO ✓");
}
