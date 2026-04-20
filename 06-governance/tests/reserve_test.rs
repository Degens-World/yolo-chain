//! reserve_test.rs — ErgoTree evaluation tests for reserve.es v1.0
//!
//! Mirror of vault_test.rs. SELF = reserve box (INPUTS(0) in this context,
//! but OUTPUTS(1) in canonical TX layout).
//!
//! Compile-time constants:
//!   RESERVE_NFT_ID = 0xbb * 32
//!   STATE_NFT_ID   = 0xaa * 32
//!   VYOLO_TOKEN_ID = 0xcc * 32

use std::convert::TryInto;

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
        ergo_box::{box_value::BoxValue, ErgoBox, ErgoBoxCandidate, NonMandatoryRegisters},
        token::{Token, TokenAmount, TokenId},
        tx_id::TxId,
    },
    ergo_tree::ErgoTree,
    serialization::SigmaSerializable,
};
use sigma_test_util::force_any_val;

const VAULT_TREE_HEX: &str = "101a0e20aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa04000e20bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb04000400040004000402040004000502040204000400050204020402040004020400040205000500040004000402d807d601db6308a7d6027300d603b2a5730100d6047302d605b5a4d9010563d801d607db63087205ed91b172077303938cb27207730400017204d606b5a5d9010663d801d608db63087206ed91b172087305938cb27208730600017204d60799c17203c1a7d1ededededededed93b172017307938cb27201730800017202938cb2720173090002730aededed93c27203c2a793b1db63087203730b938cb2db63087203730c00017202938cb2db63087203730d0002730eed93b17205730f93b172067310939a7207998cb2db6308b27206731100731200028cb2db6308b27205731300731400027315947207731693b1b5a4d9010863d801d60adb63087208ed91b1720a7317938cb2720a7318000172027319";
const RESERVE_TREE_HEX: &str = "101d0e20bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb0e20cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc04020e20aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0400040004000400040204020404040004000502040204040400040005020402040204020400040005000500040004000402d808d601db6308a7d6027300d6037301d604b2a5730200d6057303d606b5a4d9010663d801d608db63087206ed91b172087304938cb27208730500017205d607b5a5d9010763d801d609db63087207ed91b172097306938cb27209730700017205d608998cb2db63087204730800028cb2720173090002d1edededededededed93b17201730a938cb27201730b00017202938cb27201730c0002730d938cb27201730e00017203ededededed93c27204c2a793b1db63087204730f938cb2db63087204731000017202938cb2db63087204731100027312938cb2db6308720473130001720393c17204c1a7ed93b17206731493b172077315939a99c1b27207731600c1b2720673170072087318947208731993b1b5a4d9010963d801d60bdb63087209ed91b1720b731a938cb2720b731b00017202731c";
const TRUE_TREE_HEX: &str = "10010101d17300";

fn state_nft_id() -> TokenId { TokenId::from(Digest32::from([0xAAu8; 32])) }
fn reserve_nft_id() -> TokenId { TokenId::from(Digest32::from([0xBBu8; 32])) }
fn vyolo_id() -> TokenId { TokenId::from(Digest32::from([0xCCu8; 32])) }

const NANOCOIN: u64 = 1_000_000_000;
const MIN_BOX_VALUE: u64 = 360_000;

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len()).step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn load_tree(hex: &str) -> ErgoTree {
    ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).expect("valid ErgoTree")
}

fn make_vault_box(tree: &ErgoTree, yolo: u64, ch: u32) -> ErgoBox {
    let nft = Token { token_id: state_nft_id(), amount: TokenAmount::try_from(1u64).unwrap() };
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(yolo).unwrap(), ergo_tree: tree.clone(),
        tokens: Some(vec![nft].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: ch,
    }, TxId::zero(), 0).unwrap()
}

fn make_reserve_box(tree: &ErgoTree, vyolo: u64, ch: u32) -> ErgoBox {
    let nft = Token { token_id: reserve_nft_id(), amount: TokenAmount::try_from(1u64).unwrap() };
    let vt = Token { token_id: vyolo_id(), amount: TokenAmount::try_from(vyolo).unwrap() };
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX_VALUE).unwrap(), ergo_tree: tree.clone(),
        tokens: Some(vec![nft, vt].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: ch,
    }, TxId::zero(), 1).unwrap()
}

fn make_user_box(tree: &ErgoTree, v: u64, ch: u32) -> ErgoBox {
    ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(v).unwrap(), ergo_tree: tree.clone(),
        tokens: None, additional_registers: NonMandatoryRegisters::empty(), creation_height: ch,
    }, TxId::zero(), 2).unwrap()
}

/// Build context where SELF = reserve box (first input).
fn build_context(self_box: ErgoBox, others: Vec<ErgoBox>, outputs: Vec<ErgoBox>, h: u32) -> Context<'static> {
    let self_ref: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outs: &'static [ErgoBox] = Vec::leak(outputs);
    let mut refs: Vec<&'static ErgoBox> = vec![self_ref];
    for o in others { refs.push(Box::leak(Box::new(o))); }
    Context {
        height: h, self_box: self_ref, outputs: outs, data_inputs: None,
        inputs: refs.try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    let p = TestProver { secrets: vec![] };
    let m = vec![0u8; 32];
    match p.prove(tree, ctx, &m, &HintsBag::empty()) {
        Ok(pr) => match TestVerifier.verify(tree, ctx, pr.proof, &m) {
            Ok(v) => v.result, Err(_) => false,
        }, Err(_) => false,
    }
}

// ============================================================
// PHASE 0
// ============================================================

#[test]
fn reserve_tree_round_trips() {
    let t = load_tree(RESERVE_TREE_HEX);
    assert_eq!(t.sigma_serialize_bytes().unwrap(), hex_to_bytes(RESERVE_TREE_HEX));
}

#[test]
fn reserve_proposition_parses() {
    load_tree(RESERVE_TREE_HEX).proposition().expect("parse");
}

// ============================================================
// HAPPY PATH: DEPOSIT (reserve perspective — vYOLO leaves reserve)
// ============================================================

#[test]
fn accept_deposit_from_reserve_perspective() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);
    let amt = 10 * NANOCOIN;

    // SELF = reserve, other inputs = vault + user
    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, amt + NANOCOIN, 0);

    // OUTPUTS: vault at 0, reserve at 1 (canonical positions)
    let vault_out = make_vault_box(&vt, 100 * NANOCOIN + amt, 1);
    let reserve_out = make_reserve_box(&rt, 1_000_000 * NANOCOIN - amt, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let ctx = build_context(reserve_in, vec![vault_in, user_in], vec![vault_out, reserve_out, user_out], 1);
    assert!(evaluate(&rt, &ctx), "deposit from reserve perspective should accept");
}

// ============================================================
// HAPPY PATH: REDEEM (reserve perspective — vYOLO returns to reserve)
// ============================================================

#[test]
fn accept_redeem_from_reserve_perspective() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);
    let amt = 5 * NANOCOIN;

    let reserve_in = make_reserve_box(&rt, 900_000 * NANOCOIN, 0);
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 100 * NANOCOIN - amt, 1);
    let reserve_out = make_reserve_box(&rt, 900_000 * NANOCOIN + amt, 1);
    let user_out = make_user_box(&ut, amt, 1);

    let ctx = build_context(reserve_in, vec![vault_in, user_in], vec![vault_out, reserve_out, user_out], 1);
    assert!(evaluate(&rt, &ctx), "redeem from reserve perspective should accept");
}

// ============================================================
// REJECTIONS
// ============================================================

#[test]
fn reject_conservation_violation() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);

    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, 20 * NANOCOIN, 0);

    // Vault gains 10, reserve loses 5 — violation
    let vault_out = make_vault_box(&vt, 110 * NANOCOIN, 1);
    let reserve_out = make_reserve_box(&rt, 1_000_000 * NANOCOIN - 5 * NANOCOIN, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let ctx = build_context(reserve_in, vec![vault_in, user_in], vec![vault_out, reserve_out, user_out], 1);
    assert!(!evaluate(&rt, &ctx), "conservation violation should reject");
}

#[test]
fn reject_reserve_erg_drain() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);
    let amt = 10 * NANOCOIN;

    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, amt + NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 100 * NANOCOIN + amt, 1);
    // ATTACK: reserve output has lower ERG value than input
    let nft = Token { token_id: reserve_nft_id(), amount: TokenAmount::try_from(1u64).unwrap() };
    let vtok = Token { token_id: vyolo_id(), amount: TokenAmount::try_from(1_000_000 * NANOCOIN - amt).unwrap() };
    let bad_reserve_out = ErgoBox::from_box_candidate(&ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX_VALUE / 2).unwrap(), // drained!
        ergo_tree: rt.clone(),
        tokens: Some(vec![nft, vtok].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(), creation_height: 1,
    }, TxId::zero(), 1).unwrap();
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let ctx = build_context(reserve_in, vec![vault_in, user_in], vec![vault_out, bad_reserve_out, user_out], 1);
    assert!(!evaluate(&rt, &ctx), "ERG drain from reserve should reject");
}

#[test]
fn reject_reserve_script_change() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);
    let amt = 10 * NANOCOIN;

    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, amt + NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 100 * NANOCOIN + amt, 1);
    // ATTACK: reserve output uses different script
    let reserve_out = make_reserve_box(&ut, 1_000_000 * NANOCOIN - amt, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let ctx = build_context(reserve_in, vec![vault_in, user_in], vec![vault_out, reserve_out, user_out], 1);
    assert!(!evaluate(&rt, &ctx), "script change should reject");
}

#[test]
fn reject_missing_vault_pairing() {
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);

    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, NANOCOIN, 0);

    let reserve_out = make_reserve_box(&rt, 999_990 * NANOCOIN, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let ctx = build_context(reserve_in, vec![user_in], vec![reserve_out, user_out], 1);
    assert!(!evaluate(&rt, &ctx), "missing vault should reject");
}

#[test]
fn reject_noop() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);

    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 100 * NANOCOIN, 1);
    let reserve_out = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let ctx = build_context(reserve_in, vec![vault_in, user_in], vec![vault_out, reserve_out, user_out], 1);
    assert!(!evaluate(&rt, &ctx), "no-op should reject");
}
