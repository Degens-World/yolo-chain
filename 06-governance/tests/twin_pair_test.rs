//! twin_pair_test.rs — Tests that BOTH vault and reserve evaluate correctly
//! in the same transaction (dual-sided conservation).
//!
//! These tests build a full TX and evaluate both contracts independently,
//! verifying that both sides agree on validity.

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
fn load_tree(hex: &str) -> ErgoTree { ErgoTree::sigma_parse_bytes(&hex_to_bytes(hex)).unwrap() }

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

fn build_ctx(self_box: ErgoBox, others: Vec<ErgoBox>, outputs: Vec<ErgoBox>, h: u32) -> Context<'static> {
    let sr: &'static ErgoBox = Box::leak(Box::new(self_box));
    let os: &'static [ErgoBox] = Vec::leak(outputs);
    let mut refs: Vec<&'static ErgoBox> = vec![sr];
    for o in others { refs.push(Box::leak(Box::new(o))); }
    Context {
        height: h, self_box: sr, outputs: os, data_inputs: None,
        inputs: refs.try_into().unwrap(),
        pre_header: force_any_val::<PreHeader>(), headers: force_any_val::<[Header; 10]>(),
        extension: ContextExtension::empty(),
    }
}

fn eval(tree: &ErgoTree, ctx: &Context) -> bool {
    let p = TestProver { secrets: vec![] };
    let m = vec![0u8; 32];
    match p.prove(tree, ctx, &m, &HintsBag::empty()) {
        Ok(pr) => TestVerifier.verify(tree, ctx, pr.proof, &m).map(|v| v.result).unwrap_or(false),
        Err(_) => false,
    }
}

// ============================================================
// DUAL-SIDED TESTS
// ============================================================

#[test]
fn both_accept_valid_deposit() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);
    let amt = 25 * NANOCOIN;

    let vault_in = make_vault_box(&vt, 50 * NANOCOIN, 0);
    let reserve_in = make_reserve_box(&rt, 500_000 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, amt + NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 50 * NANOCOIN + amt, 1);
    let reserve_out = make_reserve_box(&rt, 500_000 * NANOCOIN - amt, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let outputs = vec![vault_out.clone(), reserve_out.clone(), user_out.clone()];

    // Evaluate vault (SELF = vault)
    let ctx_v = build_ctx(vault_in.clone(), vec![reserve_in.clone(), user_in.clone()], outputs.clone(), 1);
    assert!(eval(&vt, &ctx_v), "vault should accept deposit");

    // Evaluate reserve (SELF = reserve)
    let ctx_r = build_ctx(reserve_in, vec![vault_in, user_in], outputs, 1);
    assert!(eval(&rt, &ctx_r), "reserve should accept deposit");
}

#[test]
fn both_accept_valid_redeem() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);
    let amt = 15 * NANOCOIN;

    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let reserve_in = make_reserve_box(&rt, 900_000 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 100 * NANOCOIN - amt, 1);
    let reserve_out = make_reserve_box(&rt, 900_000 * NANOCOIN + amt, 1);
    let user_out = make_user_box(&ut, amt, 1);

    let outputs = vec![vault_out.clone(), reserve_out.clone(), user_out.clone()];

    let ctx_v = build_ctx(vault_in.clone(), vec![reserve_in.clone(), user_in.clone()], outputs.clone(), 1);
    assert!(eval(&vt, &ctx_v), "vault should accept redeem");

    let ctx_r = build_ctx(reserve_in, vec![vault_in, user_in], outputs, 1);
    assert!(eval(&rt, &ctx_r), "reserve should accept redeem");
}

#[test]
fn both_reject_conservation_violation() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);

    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, 20 * NANOCOIN, 0);

    // Vault gains 10, reserve only loses 5 — violation
    let vault_out = make_vault_box(&vt, 110 * NANOCOIN, 1);
    let reserve_out = make_reserve_box(&rt, 1_000_000 * NANOCOIN - 5 * NANOCOIN, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let outputs = vec![vault_out.clone(), reserve_out.clone(), user_out.clone()];

    let ctx_v = build_ctx(vault_in.clone(), vec![reserve_in.clone(), user_in.clone()], outputs.clone(), 1);
    assert!(!eval(&vt, &ctx_v), "vault should reject conservation violation");

    let ctx_r = build_ctx(reserve_in, vec![vault_in, user_in], outputs, 1);
    assert!(!eval(&rt, &ctx_r), "reserve should reject conservation violation");
}

#[test]
fn both_reject_noop() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);

    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let reserve_in = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, NANOCOIN, 0);

    let vault_out = make_vault_box(&vt, 100 * NANOCOIN, 1);
    let reserve_out = make_reserve_box(&rt, 1_000_000 * NANOCOIN, 1);
    let user_out = make_user_box(&ut, NANOCOIN, 1);

    let outputs = vec![vault_out.clone(), reserve_out.clone(), user_out.clone()];

    let ctx_v = build_ctx(vault_in.clone(), vec![reserve_in.clone(), user_in.clone()], outputs.clone(), 1);
    assert!(!eval(&vt, &ctx_v), "vault should reject no-op");

    let ctx_r = build_ctx(reserve_in, vec![vault_in, user_in], outputs, 1);
    assert!(!eval(&rt, &ctx_r), "reserve should reject no-op");
}

#[test]
fn reject_over_redeem_both_sides() {
    let vt = load_tree(VAULT_TREE_HEX);
    let rt = load_tree(RESERVE_TREE_HEX);
    let ut = load_tree(TRUE_TREE_HEX);

    // Try to redeem 200 from a vault holding only 100
    let vault_in = make_vault_box(&vt, 100 * NANOCOIN, 0);
    let reserve_in = make_reserve_box(&rt, 900_000 * NANOCOIN, 0);
    let user_in = make_user_box(&ut, NANOCOIN, 0);

    // vault_out would need negative value — BoxValue won't allow < MIN_BOX_VALUE
    // So the TX can't even be constructed. This validates the economic constraint.
    // Instead test a case where vault goes to MIN_BOX_VALUE but conservation fails.
    let vault_out = make_vault_box(&vt, MIN_BOX_VALUE, 1); // lost 99.99964 YOLO
    let actual_delta = 100 * NANOCOIN - MIN_BOX_VALUE; // ~99,999,640,000
    let reserve_out = make_reserve_box(&rt, 900_000 * NANOCOIN + 200 * NANOCOIN, 1); // claims +200
    let user_out = make_user_box(&ut, 200 * NANOCOIN, 1);

    let outputs = vec![vault_out.clone(), reserve_out.clone(), user_out.clone()];

    // Vault delta = MIN_BOX_VALUE - 100*NANOCOIN ≠ -(reserve delta = 200*NANOCOIN)
    let ctx_v = build_ctx(vault_in.clone(), vec![reserve_in.clone(), user_in.clone()], outputs.clone(), 1);
    assert!(!eval(&vt, &ctx_v), "vault should reject over-redeem (conservation)");

    let ctx_r = build_ctx(reserve_in, vec![vault_in, user_in], outputs, 1);
    assert!(!eval(&rt, &ctx_r), "reserve should reject over-redeem (conservation)");
}
