//! emission_test.rs — ErgoTree evaluation tests for the emission contract v1.1.
//!
//! ErgoTree compiled via Ergo node 6.1.2 `/script/p2sAddress` endpoint, then
//! decoded via `/script/addressToTree`. This bypasses sigma-rust's limited
//! ErgoScript compiler (which can't parse nested typed-lambdas in the terminal
//! path `nftBurned` check) while still running full prove/verify in Rust.
//!
//! Targets: ergo-lib 0.28 with "arbitrary" feature, rustc 1.85+

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
use blake2::{digest::consts::U32, Blake2b, Digest};
use sigma_test_util::force_any_val;

type Blake2b256 = Blake2b<U32>;

// ============================================================
// CONSTANTS
// ============================================================

const NANOCOIN: u64 = 1_000_000_000;
const BLOCKS_PER_HALVING: u32 = 1_577_880;
const INITIAL_REWARD: u64 = 50 * NANOCOIN;
const MIN_REWARD: u64 = 1 * NANOCOIN;
const MAX_HALVINGS: u32 = 20;
const TREASURY_PCT: u64 = 10;
const LP_PCT: u64 = 5;

// Pre-compiled ErgoTree hex — compiled by Ergo node 6.1.2
// Source: emission.es v1.1 (see contract file for full ErgoScript)
// Compiled via Ergo node 6.1.2 /script/p2sAddress + /script/addressToTree (454 bytes)
const EMISSION_TREE_HEX: &str = "102004b0cec00104000580d0dbc3f40204020580e8eda1ba0104040580f4f6905d04060580babbc82e04080580dd9da417040a05c0ee8ed20b0580a8d6b9070580a8d6b9070580a8d6b90704000402040004040400040004000502051405c801050a05c801051405c801050a05c801d80ed601c1a7d6029da37300d6039590720273017302959372027303730495937202730573069593720273077308959372027309730a95937202730b730c730dd60495917203730e7203730fd605b2a5731000d606c17205d607c27205d608e4c6a7040ed609b2a5731100d60ac17209d60be4c6a7050ed60c8cb2db6308a773120001d60ddb63087205d60eb2a5731300d1ed91a38cc7a701ecededed9272017204edededededed91b1720d7314938cb2720d73150001720c938cb2720d731600027317937207c2a79372069972017204ed93e4c67205040e720893e4c67205050e720b938cc7720501a3ed93720a9d9c72047318731993cbc272097208ed93c1720e9d9c7204731a731b93cbc2720e720bededed8f72017204ed9372069d9c7201731c731d93cb72077208ed93720a9d9c7201731e731f93cbc27209720bafa5d9010f63afdb6308720fd901114d0e948c721101720c";

// Treasury/LP test script: sigmaProp(HEIGHT > 0) — compiled by same node
const TREASURY_LP_TREE_HEX: &str = "10010400d191a37300";

// ============================================================
// MODEL FUNCTIONS
// ============================================================

fn block_reward(height: u32) -> u64 {
    let halvings = height / BLOCKS_PER_HALVING;
    let computed = if halvings == 0 {
        INITIAL_REWARD
    } else if halvings == 1 {
        INITIAL_REWARD / 2
    } else if halvings == 2 {
        INITIAL_REWARD / 4
    } else if halvings == 3 {
        INITIAL_REWARD / 8
    } else if halvings == 4 {
        INITIAL_REWARD / 16
    } else if halvings == 5 {
        INITIAL_REWARD / 32
    } else {
        MIN_REWARD
    };
    std::cmp::max(computed, MIN_REWARD)
}

fn split_reward(reward: u64) -> (u64, u64, u64) {
    let treasury = reward * TREASURY_PCT / 100;
    let lp = reward * LP_PCT / 100;
    let miner = reward - treasury - lp;
    (miner, treasury, lp)
}

fn genesis_box_value() -> u64 {
    let mut total: u64 = 0;
    for h in 0..MAX_HALVINGS {
        let reward = std::cmp::max(INITIAL_REWARD >> h, MIN_REWARD);
        total += reward * BLOCKS_PER_HALVING as u64;
    }
    total
}

// ============================================================
// ERGOTREE LOADING
// ============================================================

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

fn load_tree(hex: &str) -> ErgoTree {
    let bytes = hex_to_bytes(hex);
    ErgoTree::sigma_parse_bytes(&bytes).expect("valid ErgoTree")
}

fn load_emission_tree() -> ErgoTree {
    load_tree(EMISSION_TREE_HEX)
}

fn load_treasury_tree() -> ErgoTree {
    load_tree(TREASURY_LP_TREE_HEX)
}

fn load_lp_tree() -> ErgoTree {
    load_tree(TREASURY_LP_TREE_HEX)
}

// ============================================================
// HELPERS
// ============================================================

fn proposition_hash(tree: &ErgoTree) -> Vec<u8> {
    let bytes = tree.sigma_serialize_bytes().expect("serialize tree");
    let mut h = Blake2b256::new();
    h.update(&bytes);
    h.finalize().to_vec()
}

fn emission_nft_id() -> TokenId {
    TokenId::from(Digest32::from([0xEEu8; 32]))
}

fn make_emission_box(
    tree: &ErgoTree,
    value: u64,
    treasury_hash: &[u8],
    lp_hash: &[u8],
    creation_height: u32,
) -> ErgoBox {
    let nft = Token {
        token_id: emission_nft_id(),
        amount: TokenAmount::try_from(1u64).unwrap(),
    };
    let mut regs = std::collections::HashMap::new();
    regs.insert(NonMandatoryRegisterId::R4, Constant::from(treasury_hash.to_vec()));
    regs.insert(NonMandatoryRegisterId::R5, Constant::from(lp_hash.to_vec()));

    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: Some(vec![nft].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::new(regs).unwrap(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}

fn make_next_emission_box(prev: &ErgoBox, new_value: u64, creation_height: u32) -> ErgoBox {
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(new_value).unwrap(),
        ergo_tree: prev.ergo_tree.clone(),
        tokens: prev.tokens.clone(),
        additional_registers: prev.additional_registers.clone(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}

fn make_output_box(tree: &ErgoTree, value: u64, creation_height: u32) -> ErgoBox {
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: None,
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}

fn make_output_box_with_nft(tree: &ErgoTree, value: u64, creation_height: u32) -> ErgoBox {
    let nft = Token {
        token_id: emission_nft_id(),
        amount: TokenAmount::try_from(1u64).unwrap(),
    };
    let candidate = ErgoBoxCandidate {
        value: BoxValue::try_from(value).unwrap(),
        ergo_tree: tree.clone(),
        tokens: Some(vec![nft].try_into().unwrap()),
        additional_registers: NonMandatoryRegisters::empty(),
        creation_height,
    };
    ErgoBox::from_box_candidate(&candidate, TxId::zero(), 0).unwrap()
}

/// Build Context. PreHeader and Headers come from proptest Arbitrary impls
/// because the contract never reads them — we just need valid-shape fields.
fn build_context(self_box: ErgoBox, outputs: Vec<ErgoBox>, height: u32) -> Context<'static> {
    let self_ref: &'static ErgoBox = Box::leak(Box::new(self_box));
    let outputs_static: &'static [ErgoBox] = Vec::leak(outputs);
    let inputs_arr: [&'static ErgoBox; 1] = [self_ref];
    let inputs = inputs_arr.into();

    let pre_header = force_any_val::<PreHeader>();
    let headers = force_any_val::<[Header; 10]>();

    Context {
        height,
        self_box: self_ref,
        outputs: outputs_static,
        data_inputs: None,
        inputs,
        pre_header,
        headers,
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

    let verifier = TestVerifier;
    match verifier.verify(tree, ctx, proof, message.as_slice()) {
        Ok(v) => v.result,
        Err(_) => false,
    }
}

/// Build a valid normal-path scenario at any height.
/// Returns (emission_tree, self_box, outputs, height) ready for build_context.
fn normal_spend_scenario(h: u32, self_val: u64, creation_height: u32) -> (ErgoTree, ErgoBox, Vec<ErgoBox>, u32) {
    let emission_tree = load_emission_tree();
    let treasury_tree = load_treasury_tree();
    let lp_tree = load_lp_tree();
    let t_hash = proposition_hash(&treasury_tree);
    let l_hash = proposition_hash(&lp_tree);

    let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, creation_height);
    let reward = block_reward(h);
    let (_, t_amt, l_amt) = split_reward(reward);

    let outputs = vec![
        make_next_emission_box(&self_box, self_val - reward, h),
        make_output_box(&treasury_tree, t_amt, h),
        make_output_box(&lp_tree, l_amt, h),
    ];

    (emission_tree, self_box, outputs, h)
}

// ============================================================
// TESTS
// ============================================================

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn reward_at_epoch_boundaries() {
        assert_eq!(block_reward(0), 50 * NANOCOIN);
        assert_eq!(block_reward(BLOCKS_PER_HALVING), 25 * NANOCOIN);
        assert_eq!(block_reward(6 * BLOCKS_PER_HALVING), MIN_REWARD);
    }

    #[test]
    fn genesis_value_matches_spec() {
        assert_eq!(genesis_box_value(), 177_412_882_500_000_000);
    }
}

#[cfg(test)]
mod phase0_load {
    use super::*;

    #[test]
    fn emission_ergotree_loads_and_has_valid_size() {
        let tree = load_emission_tree();
        let size = tree.sigma_serialize_bytes().unwrap().len();
        println!("emission contract ErgoTree size: {} bytes", size);
        assert!(size > 100, "ErgoTree too small — likely corrupt");
    }

    #[test]
    fn treasury_lp_ergotree_loads() {
        let t = load_treasury_tree();
        let l = load_lp_tree();
        assert!(t.sigma_serialize_bytes().unwrap().len() > 0);
        assert!(l.sigma_serialize_bytes().unwrap().len() > 0);
    }

    #[test]
    fn round_trip_serialization_matches() {
        let tree = load_emission_tree();
        let bytes = tree.sigma_serialize_bytes().unwrap();
        let original = hex_to_bytes(EMISSION_TREE_HEX);
        assert_eq!(bytes, original, "ErgoTree round-trip serialization mismatch");
    }

    #[test]
    fn emission_tree_proposition_parses() {
        let tree = load_emission_tree();
        tree.proposition().expect("emission tree proposition should parse");
    }
}

#[cfg(test)]
mod phase2_normal_spend {
    use super::*;

    #[test]
    fn accept_normal_spend_at_height_1() {
        let (tree, self_box, outputs, h) = normal_spend_scenario(1, genesis_box_value(), 0);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx), "normal spend at h=1 should accept");
    }

    #[test]
    fn accept_normal_spend_at_height_100() {
        let h = 100u32;
        let val = genesis_box_value() - block_reward(0) * 99; // after 99 blocks
        let (tree, self_box, outputs, h) = normal_spend_scenario(h, val, h - 1);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx), "normal spend at h=100 should accept");
    }
}

#[cfg(test)]
mod phase3_halving_boundary {
    use super::*;

    #[test]
    fn accept_spend_at_first_halving() {
        let h = BLOCKS_PER_HALVING;
        // Value after all epoch-0 rewards are spent
        let val = genesis_box_value() - INITIAL_REWARD * BLOCKS_PER_HALVING as u64;
        let (tree, self_box, outputs, h) = normal_spend_scenario(h, val, h - 1);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx), "spend at first halving boundary should accept");
    }

    #[test]
    fn accept_spend_at_second_halving() {
        let h = 2 * BLOCKS_PER_HALVING;
        let val = genesis_box_value()
            - INITIAL_REWARD * BLOCKS_PER_HALVING as u64
            - (INITIAL_REWARD / 2) * BLOCKS_PER_HALVING as u64;
        let (tree, self_box, outputs, h) = normal_spend_scenario(h, val, h - 1);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx), "spend at second halving boundary should accept");
    }
}

#[cfg(test)]
mod phase4_underpay_treasury {
    use super::*;

    #[test]
    fn reject_treasury_underpaid() {
        let h = 1u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        let self_val = genesis_box_value();
        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, 0);
        let reward = block_reward(h);
        let (_, t_amt, l_amt) = split_reward(reward);

        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h),
            make_output_box(&treasury_tree, t_amt - 1, h), // underpay by 1 nanoERG
            make_output_box(&lp_tree, l_amt, h),
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx), "underpaid treasury should reject");
    }
}

#[cfg(test)]
mod phase5_overpay_treasury {
    use super::*;

    #[test]
    fn reject_treasury_overpaid() {
        let h = 1u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        let self_val = genesis_box_value();
        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, 0);
        let reward = block_reward(h);
        let (_, t_amt, l_amt) = split_reward(reward);

        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h),
            make_output_box(&treasury_tree, t_amt + 1, h), // overpay by 1 nanoERG
            make_output_box(&lp_tree, l_amt, h),
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx), "overpaid treasury should reject (strict ==)");
    }

    #[test]
    fn reject_lp_overpaid() {
        let h = 1u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        let self_val = genesis_box_value();
        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, 0);
        let reward = block_reward(h);
        let (_, t_amt, l_amt) = split_reward(reward);

        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h),
            make_output_box(&treasury_tree, t_amt, h),
            make_output_box(&lp_tree, l_amt + 1, h), // overpay LP
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx), "overpaid LP should reject (strict ==)");
    }
}

#[cfg(test)]
mod phase6_same_block_spend {
    use super::*;

    #[test]
    fn reject_same_block_spend() {
        let h = 5u32;
        // Self box created at height 5, spending at height 5 → heightIncreased fails
        let (tree, self_box, outputs, _) = normal_spend_scenario(h, genesis_box_value(), h);
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&tree, &ctx), "same-block spend should reject (heightIncreased)");
    }

    #[test]
    fn accept_next_block_spend() {
        let h = 6u32;
        // Self box created at height 5, spending at height 6 → heightIncreased passes
        let (tree, self_box, outputs, _) = normal_spend_scenario(h, genesis_box_value(), 5);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx), "next-block spend should accept");
    }
}

#[cfg(test)]
mod phase7_wrong_successor_height {
    use super::*;

    #[test]
    fn reject_wrong_successor_creation_height() {
        let h = 10u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        let self_val = genesis_box_value();
        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, h - 1);
        let reward = block_reward(h);
        let (_, t_amt, l_amt) = split_reward(reward);

        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h - 1), // wrong: should be h
            make_output_box(&treasury_tree, t_amt, h),
            make_output_box(&lp_tree, l_amt, h),
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx), "wrong successor creation_height should reject");
    }
}

#[cfg(test)]
mod phase8_terminal_drain {
    use super::*;

    #[test]
    fn accept_terminal_path_with_nft_burned() {
        let h = 1u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        // Self value < blockReward triggers terminal path
        let remaining = 10 * NANOCOIN; // 10 ERG, less than 50 ERG blockReward
        let self_box = make_emission_box(&emission_tree, remaining, &t_hash, &l_hash, 0);

        let term_treasury = remaining * 10 / 100; // 1 ERG
        let term_lp = remaining * 5 / 100;        // 0.5 ERG

        // Terminal outputs: treasury + LP + dummy (OUTPUTS(2) is accessed by shared
        // ValDef even though terminal path doesn't use it)
        let outputs = vec![
            make_output_box(&treasury_tree, term_treasury, h),
            make_output_box(&lp_tree, term_lp, h),
            make_output_box(&lp_tree, NANOCOIN, h), // dummy for OUTPUTS(2) ValDef
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&emission_tree, &ctx), "terminal path with NFT burned should accept");
    }
}

#[cfg(test)]
mod phase9_terminal_nft_not_burned {
    use super::*;

    #[test]
    fn reject_terminal_nft_smuggled_in_output() {
        let h = 1u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        let remaining = 10 * NANOCOIN;
        let self_box = make_emission_box(&emission_tree, remaining, &t_hash, &l_hash, 0);

        let term_treasury = remaining * 10 / 100;
        let term_lp = remaining * 5 / 100;

        // Third output smuggles the emission NFT — nftBurned should fail
        let outputs = vec![
            make_output_box(&treasury_tree, term_treasury, h),
            make_output_box(&lp_tree, term_lp, h),
            make_output_box_with_nft(&treasury_tree, NANOCOIN, h), // NFT not burned (also serves as OUTPUTS(2) for shared ValDef)
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx), "terminal path with NFT not burned should reject");
    }

    #[test]
    fn reject_terminal_nft_in_treasury_output() {
        let h = 1u32;
        let emission_tree = load_emission_tree();
        let treasury_tree = load_treasury_tree();
        let lp_tree = load_lp_tree();
        let t_hash = proposition_hash(&treasury_tree);
        let l_hash = proposition_hash(&lp_tree);

        let remaining = 10 * NANOCOIN;
        let self_box = make_emission_box(&emission_tree, remaining, &t_hash, &l_hash, 0);

        let term_treasury = remaining * 10 / 100;
        let term_lp = remaining * 5 / 100;

        // NFT smuggled into the treasury output itself
        let outputs = vec![
            make_output_box_with_nft(&treasury_tree, term_treasury, h),
            make_output_box(&lp_tree, term_lp, h),
            make_output_box(&lp_tree, NANOCOIN, h), // dummy for OUTPUTS(2) shared ValDef
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx), "NFT in treasury output should reject");
    }
}
