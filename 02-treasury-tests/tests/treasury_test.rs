//! treasury_test.rs — ErgoTree evaluation tests for the treasury contracts v1.1.
//!
//! v1.1 audit fixes: singleton NFT, migration approval path, proposal hash constraint.
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

// Treasury Governance contract v1.1 — compiled by Ergo node 6.1.2 (642 bytes)
const GOVERNANCE_TREE_HEX: &str = "101a05000502040004000400040004000502050005c0ca010402040408cd034646ae5047316b4230d0086c8acec687f00b1cd9d1dc634f6cb358ac0a9a8fff08cd0288e2ddeb04657dbd0edadf9c1f98da3b3895faa1f00527934dd35d17542ffe9b08cd020305c75318f36537e1a5d0db4dfcdc94a9708a84a38f81ea7cfbe239252e01f20580897a0500050005000502050005040504040004000502d81bd601e4c6a70505d6029072017300d603e4c6a70805d604ef9372037301d605b2a5730200d60693c27205c2a7d607db63087205d60891b172077303d6098cb2db6308a773040001d60aeded7208938cb27207730500017209938cb27207730600027307d60bc17205d60cc1a7d60d92720b720cd60ee4c672050505d60f7ea305d61093720e720fd611e4c672050805d6129372117203d613e4c67205040ed614e4c67205060ed615e4c672050705d6169172017308d61791720f9a72017309d618b2a5730a00d619e4c6a7060ed61ae4c6a70705d61b937213e4c6a7040eea0298730b830308730c730d730ed1ececececececededededededed720272047206720a720d72107212937213cbb372147a7215ededededed721672047217ed93c27218721992c17218721aededed720692720b9999720c721a730f93720e73109372117311720aededededed72167206720d93720e73127212720aedededed72047206720dedededed721b93720e72019372147219937215721a7212720aededededed72047206720d9372117313ededed721b93720e72019372147219937215721a720aededededededed7202720493720373147206720d72109372117315720aeded93720373167217eded7208938cb27207731700017209938cb27207731800027319";

// Treasury Accumulation contract v1.1 — compiled by Ergo node 6.1.2 (72 bytes)
const ACCUMULATION_TREE_HEX: &str = "100204000e20ecce6bc576975c9c17246f316caa34917705fc09416a83326c7af666f599180ad802d601b2a5730000d602c27201d1eced937202c2a792c17201c1a793cb72027301";

// ============================================================
// DETERMINISTIC SIGNER KEYS (test-only, never on mainnet)
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

/// Compute proposal hash: blake2b256(recipient_bytes ++ longToByteArray(amount))
fn compute_proposal_hash(recipient_bytes: &[u8], amount: i64) -> Vec<u8> {
    let mut hasher = Blake2b256::new();
    hasher.update(recipient_bytes);
    hasher.update(&amount.to_be_bytes());
    hasher.finalize().to_vec()
}

// ============================================================
// BOX CONSTRUCTION
// ============================================================

fn make_nft_token() -> Token {
    Token {
        token_id: TokenId::from(Digest32::from([0xAAu8; 32])),
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

/// Build a governance box with NFT and all registers.
fn make_governance_box(
    tree: &ErgoTree,
    value: u64,
    proposal_hash: Vec<u8>,
    approval_height: i64,
    recipient: Vec<u8>,
    amount: i64,
    state_flag: i64,
    creation_height: u32,
) -> ErgoBox {
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(proposal_hash));
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(approval_height));
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(recipient));
    regs.insert(NonMandatoryRegisterId::R7, Constant::from(amount));
    regs.insert(NonMandatoryRegisterId::R8, Constant::from(state_flag));

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

/// Governance box with NFT but using a different script (for migration output)
fn make_governance_box_other_script(
    tree: &ErgoTree,
    value: u64,
    proposal_hash: Vec<u8>,
    approval_height: i64,
    recipient: Vec<u8>,
    amount: i64,
    state_flag: i64,
    creation_height: u32,
) -> ErgoBox {
    let mut regs = HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(proposal_hash));
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(approval_height));
    regs.insert(NonMandatoryRegisterId::R6, Constant::from(recipient));
    regs.insert(NonMandatoryRegisterId::R7, Constant::from(amount));
    regs.insert(NonMandatoryRegisterId::R8, Constant::from(state_flag));

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

/// Shorthand: idle governance box (no proposal, normal state)
fn make_idle_governance_box(tree: &ErgoTree, value: u64, creation_height: u32) -> ErgoBox {
    make_governance_box(tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, creation_height)
}

/// Shorthand: governance box with an active proposal
fn make_proposal_governance_box(
    tree: &ErgoTree, value: u64, recipient_bytes: Vec<u8>,
    amount: i64, approval_height: i64, state_flag: i64, creation_height: u32,
) -> ErgoBox {
    let proposal_hash = compute_proposal_hash(&recipient_bytes, amount);
    make_governance_box(tree, value, proposal_hash, approval_height, recipient_bytes, amount, state_flag, creation_height)
}

fn recipient_bytes() -> Vec<u8> {
    load_accumulation_tree().sigma_serialize_bytes().unwrap()
}

fn recipient_tree() -> ErgoTree {
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
        println!("governance contract v1.1 ErgoTree size: {} bytes", size);
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
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "2-of-3 threshold should accept");
    }

    #[test]
    fn reject_1_of_3_threshold() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_1_of_3()), "1-of-3 should reject");
    }
}

#[cfg(test)]
mod phase2_approval {
    use super::*;

    #[test]
    fn accept_approval_valid_proposal() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "valid approval should accept");
    }

    #[test]
    fn reject_approval_inconsistent_hash() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        // Wrong hash — doesn't match R6/R7
        let bad_hash = vec![0xFFu8; 32];

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, bad_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "inconsistent proposal hash should reject");
    }

    #[test]
    fn reject_approval_when_proposal_active() {
        let tree = load_governance_tree();
        let h = 200u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, 50, 0, h - 1);
        let proposal_hash = compute_proposal_hash(&rb, amount);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "approval with active proposal should reject");
    }

    #[test]
    fn reject_approval_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_governance_box(&tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 1i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "approval when frozen should reject");
    }

    #[test]
    fn reject_approval_value_not_preserved() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value - NANOCOIN, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "approval with reduced value should reject");
    }

    #[test]
    fn reject_approval_insufficient_signers() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_1_of_3()), "1-of-3 should reject");
    }
}

#[cfg(test)]
mod phase3_execution {
    use super::*;

    #[test]
    fn accept_execution_after_timelock() {
        let tree = load_governance_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "execution after timelock should accept");
    }

    #[test]
    fn reject_execution_before_timelock() {
        let tree = load_governance_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS) as u32; // exact boundary, not past
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "execution before timelock should reject");
    }

    #[test]
    fn reject_execution_when_frozen() {
        let tree = load_governance_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 1, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "execution when frozen should reject");
    }

    #[test]
    fn reject_execution_wrong_recipient() {
        let tree = load_governance_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&tree, amount as u64, exec_h); // wrong recipient
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "wrong recipient should reject");
    }

    #[test]
    fn reject_execution_wrong_amount() {
        let tree = load_governance_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64 - 1, exec_h); // short by 1
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "wrong amount should reject");
    }

    #[test]
    fn reject_execution_no_active_proposal() {
        let tree = load_governance_tree();
        let exec_h = 20000u32;
        let value = 100 * NANOCOIN;
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_idle_governance_box(&tree, value, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "no proposal should reject");
    }

    #[test]
    fn reject_execution_change_not_to_self() {
        let tree = load_governance_tree();
        let accum_tree = load_accumulation_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, exec_h - 1);
        let out0 = make_output_box(&accum_tree, value - amount as u64 - TX_FEE, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "change to wrong script should reject");
    }

    #[test]
    fn reject_execution_proposal_not_cleared() {
        let tree = load_governance_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, exec_h - 1);
        // R5 not cleared
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], approval_h, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "proposal not cleared should reject");
    }
}

#[cfg(test)]
mod phase4_cancellation {
    use super::*;

    #[test]
    fn accept_cancellation_active_proposal() {
        let tree = load_governance_tree();
        let h = 200u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, 50, 0, h - 1);
        let out0 = make_governance_box(&tree, value, vec![0u8; 32], 0i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "cancellation should accept");
    }

    #[test]
    fn reject_cancellation_value_stolen() {
        let tree = load_governance_tree();
        let h = 200u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, 50, 0, h - 1);
        let out0 = make_governance_box(&tree, value - NANOCOIN, vec![0u8; 32], 0i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "cancel with stolen value should reject");
    }
}

#[cfg(test)]
mod phase5_consolidation {
    use super::*;

    #[test]
    fn accept_consolidation_value_increases() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_idle_governance_box(&tree, value + 10 * NANOCOIN, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "consolidation should accept");
    }

    #[test]
    fn reject_consolidation_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 50 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h - 1);
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "consolidation when frozen should reject");
    }

    #[test]
    fn reject_consolidation_registers_changed() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        // Change R7 — no path allows this during consolidation
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, vec![0u8; 32], 0i64, vec![0u8; 36], 999i64, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "changed registers should reject");
    }

    #[test]
    fn reject_consolidation_value_decreases() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 50 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_idle_governance_box(&tree, value - NANOCOIN, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "decreased value should reject");
    }
}

#[cfg(test)]
mod phase6_freeze {
    use super::*;

    #[test]
    fn accept_freeze_threshold_sig() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "freeze should accept");
    }

    #[test]
    fn reject_freeze_already_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h - 1);
        let out0 = make_governance_box(&tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "already frozen should reject");
    }
}

#[cfg(test)]
mod phase7_migration {
    use super::*;

    #[test]
    fn accept_migration_approval() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        // Output: R8=2 (migration flag), R5=HEIGHT
        let out0 = make_governance_box(&tree, value, vec![0u8; 32], h as i64, vec![0u8; 36], 0i64, 2i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "migration approval should accept");
    }

    #[test]
    fn reject_migration_approval_when_frozen() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 1i64, h - 1);
        let out0 = make_governance_box(&tree, value, vec![0u8; 32], h as i64, vec![0u8; 36], 0i64, 2i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration approval when frozen should reject");
    }

    #[test]
    fn reject_migration_approval_when_proposal_active() {
        let tree = load_governance_tree();
        let h = 200u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_proposal_governance_box(&tree, value, rb, amount, 50, 0, h - 1);
        let out0 = make_governance_box(&tree, value, vec![0u8; 32], h as i64, vec![0u8; 36], 0i64, 2i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration approval with active proposal should reject");
    }

    #[test]
    fn accept_migration_execute_after_timelock() {
        let tree = load_governance_tree();
        let new_contract = load_accumulation_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;

        // Self: R8=2 (migration approved), R5=approval_h
        let self_box = make_governance_box(&tree, value, vec![0u8; 32], approval_h, vec![0u8; 36], 0i64, 2i64, exec_h - 1);

        // Output: NFT goes to new contract (different script). Needs registers for shared ValDefs.
        let out0 = make_governance_box_other_script(&new_contract, value - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let dummy = make_output_box(&tree, NANOCOIN, exec_h);
        let ctx = build_context(self_box, vec![out0, dummy], exec_h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "migration execute should accept");
    }

    #[test]
    fn reject_migration_execute_without_flag() {
        let tree = load_governance_tree();
        let new_contract = load_accumulation_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS + 1) as u32;
        let value = 100 * NANOCOIN;

        // R8=0, not migration approved
        let self_box = make_governance_box(&tree, value, vec![0u8; 32], approval_h, vec![0u8; 36], 0i64, 0i64, exec_h - 1);
        let out0 = make_governance_box_other_script(&new_contract, value - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let dummy = make_output_box(&tree, NANOCOIN, exec_h);
        let ctx = build_context(self_box, vec![out0, dummy], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration without flag should reject");
    }

    #[test]
    fn reject_migration_execute_before_timelock() {
        let tree = load_governance_tree();
        let new_contract = load_accumulation_tree();
        let approval_h = 100i64;
        let exec_h = (approval_h + TIMELOCK_BLOCKS) as u32; // exact boundary
        let value = 100 * NANOCOIN;

        let self_box = make_governance_box(&tree, value, vec![0u8; 32], approval_h, vec![0u8; 36], 0i64, 2i64, exec_h - 1);
        let out0 = make_governance_box_other_script(&new_contract, value - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let dummy = make_output_box(&tree, NANOCOIN, exec_h);
        let ctx = build_context(self_box, vec![out0, dummy], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "migration before timelock should reject");
    }
}

#[cfg(test)]
mod phase8_accumulation {
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
mod phase9_integration {
    use super::*;

    #[test]
    fn reject_double_execution() {
        let tree = load_governance_tree();
        let exec_h = 20000u32;
        let value = 80 * NANOCOIN;
        let amount = 10i64 * NANOCOIN as i64;

        let self_box = make_idle_governance_box(&tree, value, exec_h - 1);
        let out0 = make_governance_box(&tree, value - amount as u64 - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, exec_h);
        let out1 = make_output_box(&recipient_tree(), amount as u64, exec_h);
        let ctx = build_context(self_box, vec![out0, out1], exec_h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "double execution should reject");
    }

    #[test]
    fn accept_approval_after_cancellation() {
        let tree = load_governance_tree();
        let h = 300u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 5i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_idle_governance_box(&tree, value, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "approval after cancel should accept");
    }

    #[test]
    fn reject_proposal_replacement_without_cancel() {
        let tree = load_governance_tree();
        let h = 200u32;
        let value = 100 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let proposal_hash = compute_proposal_hash(&rb, amount);

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, 50, 0, h - 1);
        let out0 = make_governance_box(&tree, value, proposal_hash, h as i64, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(!evaluate(&tree, &ctx, &prover_2_of_3()), "replacement without cancel should reject");
    }

    #[test]
    fn consolidation_preserves_active_proposal() {
        let tree = load_governance_tree();
        let h = 100u32;
        let value = 50 * NANOCOIN;
        let rb = recipient_bytes();
        let amount = 10i64 * NANOCOIN as i64;
        let approval_h = 50i64;

        let self_box = make_proposal_governance_box(&tree, value, rb.clone(), amount, approval_h, 0, h - 1);

        let proposal_hash = compute_proposal_hash(&rb, amount);
        let out0 = make_governance_box(&tree, value + 10 * NANOCOIN, proposal_hash, approval_h, rb, amount, 0i64, h);
        let dummy = make_output_box(&tree, NANOCOIN, h);
        let ctx = build_context(self_box, vec![out0, dummy], h);
        assert!(evaluate(&tree, &ctx, &prover_2_of_3()), "consolidation with proposal should accept");
    }

    #[test]
    fn full_migration_flow() {
        // Step 1: Migration approval (R8: 0→2, R5=HEIGHT)
        let tree = load_governance_tree();
        let h1 = 100u32;
        let value = 100 * NANOCOIN;

        let self1 = make_idle_governance_box(&tree, value, h1 - 1);
        let out1 = make_governance_box(&tree, value, vec![0u8; 32], h1 as i64, vec![0u8; 36], 0i64, 2i64, h1);
        let dummy1 = make_output_box(&tree, NANOCOIN, h1);
        let ctx1 = build_context(self1, vec![out1, dummy1], h1);
        assert!(evaluate(&tree, &ctx1, &prover_2_of_3()), "migration approval step should accept");

        // Step 2: Migration execute after timelock
        let h2 = (h1 as i64 + TIMELOCK_BLOCKS + 1) as u32;
        let new_contract = load_accumulation_tree();

        let self2 = make_governance_box(&tree, value, vec![0u8; 32], h1 as i64, vec![0u8; 36], 0i64, 2i64, h1);
        let out2 = make_governance_box_other_script(&new_contract, value - TX_FEE, vec![0u8; 32], 0i64, vec![0u8; 36], 0i64, 0i64, h2);
        let dummy2 = make_output_box(&tree, NANOCOIN, h2);
        let ctx2 = build_context(self2, vec![out2, dummy2], h2);
        assert!(evaluate(&tree, &ctx2, &prover_2_of_3()), "migration execute step should accept");
    }
}
