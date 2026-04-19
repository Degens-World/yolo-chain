//! vault_test.rs — ErgoTree evaluation tests for vault.es v1.0
//!
//! ErgoTree compiled via Ergo node 6.x `/script/p2sAddress` + `/script/addressToTree`
//! because sigma-rust 0.28 cannot parse typed-lambda `.filter { (b: Box) => ... }`.
//!
//! Compile-time constants baked in:
//!   STATE_NFT_ID   = 0xaa * 32 bytes
//!   RESERVE_NFT_ID = 0xbb * 32 bytes
//! vYOLO token ID   = 0xcc * 32 bytes (used in reserve boxes)

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
        ergo_box::{
            box_value::BoxValue, ErgoBox, ErgoBoxCandidate, NonMandatoryRegisters,
        },
        token::{Token, TokenAmount, TokenId},
        tx_id::TxId,
    },
    ergo_tree::ErgoTree,
    serialization::SigmaSerializable,
};
use sigma_test_util::force_any_val;

// ============================================================
// PRE-COMPILED ERGOTREE HEX (from Ergo node)
// ============================================================

const VAULT_TREE_HEX: &str = "101a0e20aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa04000e20bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb04000400040004000402040004000502040204000400050204020402040004020400040205000500040004000402d807d601db6308a7d6027300d603b2a5730100d6047302d605b5a4d9010563d801d607db63087205ed91b172077303938cb27207730400017204d606b5a5d9010663d801d608db63087206ed91b172087305938cb27208730600017204d60799c17203c1a7d1ededededededed93b172017307938cb27201730800017202938cb2720173090002730aededed93c27203c2a793b1db63087203730b938cb2db63087203730c00017202938cb2db63087203730d0002730eed93b17205730f93b172067310939a7207998cb2db6308b27206731100731200028cb2db6308b27205731300731400027315947207731693b1b5a4d9010863d801d60adb63087208ed91b1720a7317938cb2720a7318000172027319";

// Reserve tree — needed for building reserve boxes (not evaluated in vault tests)
const RESERVE_TREE_HEX: &str = "101d0e20bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb0e20cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc04020e20aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa0400040004000400040204020404040004000502040204040400040005020402040204020400040005000500040004000402d808d601db6308a7d6027300d6037301d604b2a5730200d6057303d606b5a4d9010663d801d608db63087206ed91b172087304938cb27208730500017205d607b5a5d9010763d801d609db63087207ed91b172097306938cb27209730700017205d608998cb2db63087204730800028cb2720173090002d1edededededededed93b17201730a938cb27201730b00017202938cb27201730c0002730d938cb27201730e00017203ededededed93c27204c2a793b1db63087204730f938cb2db63087204731000017202938cb2db63087204731100027312938cb2db6308720473130001720393c17204c1a7ed93b17206731493b172077315939a99c1b27207731600c1b2720673170072087318947208731993b1b5a4d9010963d801d60bdb63087209ed91b1720b731a938cb2720b731b00017202731c";

// sigmaProp(true) — compiled via Ergo node
const TRUE_TREE_HEX: &str = "10010101d17300";

// ============================================================
// TOKEN IDS
// ============================================================

fn state_nft_id() -> TokenId { TokenId::from(Digest32::from([0xAAu8; 32])) }
fn reserve_nft_id() -> TokenId { TokenId::from(Digest32::from([0xBBu8; 32])) }
fn vyolo_id() -> TokenId { TokenId::from(Digest32::from([0xCCu8; 32])) }

const NANOCOIN: u64 = 1_000_000_000;
const MIN_BOX_VALUE: u64 = 360_000;

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

fn make_vault_box(tree: &ErgoTree, yolo_value: u64, creation_height: u32) -> ErgoBox {
    let nft = Token {
        token_id: state_nft_id(),
        amount: TokenAmount::try_from(1u64).unwrap(),
    };
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(yolo_value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: Some(vec![nft].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}

fn make_reserve_box(tree: &ErgoTree, vyolo_amount: u64, creation_height: u32) -> ErgoBox {
    let nft = Token {
        token_id: reserve_nft_id(),
        amount: TokenAmount::try_from(1u64).unwrap(),
    };
    let vyolo = Token {
        token_id: vyolo_id(),
        amount: TokenAmount::try_from(vyolo_amount).unwrap(),
    };
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(MIN_BOX_VALUE).unwrap(),
        ergo_tree: tree.clone(),
        tokens: Some(vec![nft, vyolo].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 1).unwrap()
}

fn make_user_box(tree: &ErgoTree, value: u64, creation_height: u32) -> ErgoBox {
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: None,
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 2).unwrap()
}

fn make_user_box_with_vyolo(tree: &ErgoTree, value: u64, vyolo_amount: u64, creation_height: u32) -> ErgoBox {
    let vyolo = Token {
        token_id: vyolo_id(),
        amount: TokenAmount::try_from(vyolo_amount).unwrap(),
    };
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: Some(vec![vyolo].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 2).unwrap()
}

/// Build context with multiple inputs. SELF = inputs[0] (the vault box).
fn build_context(
    self_box: ErgoBox,
    other_inputs: Vec<ErgoBox>,
    outputs: Vec<ErgoBox>,
    height: u32,
) -> Context<'static> {
    let self_ref: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outputs_static: &'static [ErgoBox] = Vec::leak(outputs);

    let mut input_refs: Vec<&'static ErgoBox> = vec![self_ref];
    for inp in other_inputs {
        input_refs.push(Box::leak(Box::new(inp)));
    }
    let inputs = input_refs.try_into().expect("1-255 inputs");

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

fn evaluate(tree: &ErgoTree, ctx: &Context) -> bool {
    let prover = TestProver { secrets: vec![] };
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

// ============================================================
// PHASE 0: LOAD + SERIALIZE ROUND-TRIP
// ============================================================

#[test]
fn vault_tree_loads_and_round_trips() {
    let tree = load_tree(VAULT_TREE_HEX);
    let bytes = tree.sigma_serialize_bytes().unwrap();
    assert_eq!(bytes, hex_to_bytes(VAULT_TREE_HEX), "round-trip mismatch");
}

#[test]
fn vault_proposition_parses() {
    let tree = load_tree(VAULT_TREE_HEX);
    tree.proposition().expect("vault proposition should parse");
}

// ============================================================
// HAPPY PATH: DEPOSIT
// ============================================================

#[test]
fn accept_deposit_10_coins() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let reserve_tree = load_tree(RESERVE_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let deposit_amount = 10 * NANOCOIN;
    let initial_vault_yolo = 100 * NANOCOIN;
    let initial_reserve_vyolo = 1_000_000 * NANOCOIN;

    // INPUTS
    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let reserve_in = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 0);
    let user_in = make_user_box(&user_tree, deposit_amount + NANOCOIN, 0); // deposit + fee

    // OUTPUTS
    let vault_out = make_vault_box(&vault_tree, initial_vault_yolo + deposit_amount, 1);
    let reserve_out = make_reserve_box(&reserve_tree, initial_reserve_vyolo - deposit_amount, 1);
    let user_out = make_user_box_with_vyolo(&user_tree, NANOCOIN, deposit_amount, 1);

    let ctx = build_context(
        vault_in,
        vec![reserve_in, user_in],
        vec![vault_out, reserve_out, user_out],
        1,
    );
    assert!(evaluate(&vault_tree, &ctx), "deposit should accept");
}

// ============================================================
// HAPPY PATH: REDEEM
// ============================================================

#[test]
fn accept_redeem_5_coins() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let reserve_tree = load_tree(RESERVE_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let redeem_amount = 5 * NANOCOIN;
    let initial_vault_yolo = 100 * NANOCOIN;
    let initial_reserve_vyolo = 900_000 * NANOCOIN;

    // INPUTS
    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let reserve_in = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 0);
    let user_in = make_user_box_with_vyolo(&user_tree, NANOCOIN, redeem_amount, 0);

    // OUTPUTS: vault loses YOLO, reserve gains vYOLO
    let vault_out = make_vault_box(&vault_tree, initial_vault_yolo - redeem_amount, 1);
    let reserve_out = make_reserve_box(&reserve_tree, initial_reserve_vyolo + redeem_amount, 1);
    let user_out = make_user_box(&user_tree, redeem_amount, 1); // user gets YOLO back

    let ctx = build_context(
        vault_in,
        vec![reserve_in, user_in],
        vec![vault_out, reserve_out, user_out],
        1,
    );
    assert!(evaluate(&vault_tree, &ctx), "redeem should accept");
}

// ============================================================
// REJECTION: CONSERVATION VIOLATION
// ============================================================

#[test]
fn reject_conservation_violation_deposit() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let reserve_tree = load_tree(RESERVE_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let deposit_amount = 10 * NANOCOIN;
    let initial_vault_yolo = 100 * NANOCOIN;
    let initial_reserve_vyolo = 1_000_000 * NANOCOIN;

    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let reserve_in = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 0);
    let user_in = make_user_box(&user_tree, deposit_amount + NANOCOIN, 0);

    // VIOLATION: vault gains 10 but reserve only gives 5 vYOLO
    let vault_out = make_vault_box(&vault_tree, initial_vault_yolo + deposit_amount, 1);
    let reserve_out = make_reserve_box(&reserve_tree, initial_reserve_vyolo - (deposit_amount / 2), 1);
    let user_out = make_user_box_with_vyolo(&user_tree, NANOCOIN, deposit_amount / 2, 1);

    let ctx = build_context(
        vault_in,
        vec![reserve_in, user_in],
        vec![vault_out, reserve_out, user_out],
        1,
    );
    assert!(!evaluate(&vault_tree, &ctx), "conservation violation should reject");
}

// ============================================================
// REJECTION: NO-OP TRANSACTION
// ============================================================

#[test]
fn reject_noop_transaction() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let reserve_tree = load_tree(RESERVE_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let initial_vault_yolo = 100 * NANOCOIN;
    let initial_reserve_vyolo = 1_000_000 * NANOCOIN;

    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let reserve_in = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 0);
    let user_in = make_user_box(&user_tree, NANOCOIN, 0);

    // NO-OP: vault value unchanged
    let vault_out = make_vault_box(&vault_tree, initial_vault_yolo, 1);
    let reserve_out = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 1);
    let user_out = make_user_box(&user_tree, NANOCOIN, 1);

    let ctx = build_context(
        vault_in,
        vec![reserve_in, user_in],
        vec![vault_out, reserve_out, user_out],
        1,
    );
    assert!(!evaluate(&vault_tree, &ctx), "no-op transaction should reject");
}

// ============================================================
// REJECTION: SCRIPT REPLACEMENT
// ============================================================

#[test]
fn reject_vault_script_change() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let reserve_tree = load_tree(RESERVE_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let deposit_amount = 10 * NANOCOIN;
    let initial_vault_yolo = 100 * NANOCOIN;
    let initial_reserve_vyolo = 1_000_000 * NANOCOIN;

    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let reserve_in = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 0);
    let user_in = make_user_box(&user_tree, deposit_amount + NANOCOIN, 0);

    // ATTACK: vault output uses a different script (user_tree instead of vault_tree)
    let vault_out = make_vault_box(&user_tree, initial_vault_yolo + deposit_amount, 1);
    let reserve_out = make_reserve_box(&reserve_tree, initial_reserve_vyolo - deposit_amount, 1);
    let user_out = make_user_box_with_vyolo(&user_tree, NANOCOIN, deposit_amount, 1);

    let ctx = build_context(
        vault_in,
        vec![reserve_in, user_in],
        vec![vault_out, reserve_out, user_out],
        1,
    );
    assert!(!evaluate(&vault_tree, &ctx), "script replacement should reject");
}

// ============================================================
// REJECTION: EXTRA TOKENS IN VAULT
// ============================================================

#[test]
fn reject_extra_tokens_in_vault_output() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let reserve_tree = load_tree(RESERVE_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let deposit_amount = 10 * NANOCOIN;
    let initial_vault_yolo = 100 * NANOCOIN;
    let initial_reserve_vyolo = 1_000_000 * NANOCOIN;

    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let reserve_in = make_reserve_box(&reserve_tree, initial_reserve_vyolo, 0);
    let user_in = make_user_box(&user_tree, deposit_amount + NANOCOIN, 0);

    // ATTACK: vault output has 2 tokens (NFT + extra vYOLO smuggled in)
    let nft = Token { token_id: state_nft_id(), amount: TokenAmount::try_from(1u64).unwrap() };
    let extra = Token { token_id: vyolo_id(), amount: TokenAmount::try_from(1u64).unwrap() };
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(initial_vault_yolo + deposit_amount).unwrap(),
        ergo_tree: vault_tree.clone(),
        tokens: Some(vec![nft, extra].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height: 1,
    };
    let bad_vault_out = ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap();
    let reserve_out = make_reserve_box(&reserve_tree, initial_reserve_vyolo - deposit_amount, 1);
    let user_out = make_user_box_with_vyolo(&user_tree, NANOCOIN, deposit_amount, 1);

    let ctx = build_context(
        vault_in,
        vec![reserve_in, user_in],
        vec![bad_vault_out, reserve_out, user_out],
        1,
    );
    assert!(!evaluate(&vault_tree, &ctx), "extra tokens in vault should reject");
}

// ============================================================
// REJECTION: MISSING RESERVE PAIRING
// ============================================================

#[test]
fn reject_missing_reserve_in_transaction() {
    let vault_tree = load_tree(VAULT_TREE_HEX);
    let user_tree = load_tree(TRUE_TREE_HEX);

    let deposit_amount = 10 * NANOCOIN;
    let initial_vault_yolo = 100 * NANOCOIN;

    let vault_in = make_vault_box(&vault_tree, initial_vault_yolo, 0);
    let user_in = make_user_box(&user_tree, deposit_amount + NANOCOIN, 0);

    // NO RESERVE: vault output only, no reserve pairing
    let vault_out = make_vault_box(&vault_tree, initial_vault_yolo + deposit_amount, 1);
    let user_out = make_user_box(&user_tree, NANOCOIN, 1);

    let ctx = build_context(
        vault_in,
        vec![user_in],
        vec![vault_out, user_out],
        1,
    );
    assert!(!evaluate(&vault_tree, &ctx), "missing reserve pairing should reject");
}
