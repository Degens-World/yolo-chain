//! integration_test.rs — Coinbase integration tests proving emission, treasury,
//! and LP fund contracts work together in a single coinbase transaction.
//!
//! Uses pre-compiled ErgoTree bytes from the individual test suites:
//!   - emission.es v1.1         (from 01-emission-tests)
//!   - treasury_accumulation.es (from 02-treasury-tests)
//!   - lp_accumulation.es       (from 03-lp-fund-tests)
//!   - treasury_governance.es   (from 02-treasury-tests, for test 3b)
//!   - lp_fund.es               (from 03-lp-fund-tests, for test 4b)
//!
//! No node or compilation needed — all ErgoTree hex constants are pre-compiled.
//! Targets: ergo-lib 0.28, rustc 1.85+

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
const TREASURY_PCT: u64 = 10;
const LP_PCT: u64 = 5;

// ============================================================
// PRE-COMPILED ERGOTREE HEX CONSTANTS
// ============================================================

// Emission contract v1.1 — from 01-emission-tests (454 bytes)
const EMISSION_TREE_HEX: &str = "102004b0cec00104000580d0dbc3f40204020580e8eda1ba0104040580f4f6905d04060580babbc82e04080580dd9da417040a05c0ee8ed20b0580a8d6b9070580a8d6b9070580a8d6b90704000402040004040400040004000502051405c801050a05c801051405c801050a05c801d80ed601c1a7d6029da37300d6039590720273017302959372027303730495937202730573069593720273077308959372027309730a95937202730b730c730dd60495917203730e7203730fd605b2a5731000d606c17205d607c27205d608e4c6a7040ed609b2a5731100d60ac17209d60be4c6a7050ed60c8cb2db6308a773120001d60ddb63087205d60eb2a5731300d1ed91a38cc7a701ecededed9272017204edededededed91b1720d7314938cb2720d73150001720c938cb2720d731600027317937207c2a79372069972017204ed93e4c67205040e720893e4c67205050e720b938cc7720501a3ed93720a9d9c72047318731993cbc272097208ed93c1720e9d9c7204731a731b93cbc2720e720bededed8f72017204ed9372069d9c7201731c731d93cb72077208ed93720a9d9c7201731e731f93cbc27209720bafa5d9010f63afdb6308720fd901114d0e948c721101720c";

// Treasury accumulation contract v1.0 — from 02-treasury-tests (72 bytes)
// Embeds governance hash ecce6bc576975c9c17246f316caa34917705fc09416a83326c7af666f599180a
const TREASURY_ACCUM_TREE_HEX: &str = "100204000e20ecce6bc576975c9c17246f316caa34917705fc09416a83326c7af666f599180ad802d601b2a5730000d602c27201d1eced937202c2a792c17201c1a793cb72027301";

// LP fund accumulation contract v1.0 — from 03-lp-fund-tests (72 bytes)
// Embeds governance hash f7e52722204eab03cf13bea0772e0e00ee48f4a6f81396514d9a3b692d61b1e4
const LP_ACCUM_TREE_HEX: &str = "100204000e20f7e52722204eab03cf13bea0772e0e00ee48f4a6f81396514d9a3b692d61b1e4d802d601b2a5730000d602c27201d1eced937202c2a792c17201c1a793cb72027301";

// Treasury governance contract v1.1 — from 02-treasury-tests (642 bytes)
// Used in test 3b (accumulation → governance transfer)
const TREASURY_GOV_TREE_HEX: &str = "101a05000502040004000400040004000502050005c0ca010402040408cd034646ae5047316b4230d0086c8acec687f00b1cd9d1dc634f6cb358ac0a9a8fff08cd0288e2ddeb04657dbd0edadf9c1f98da3b3895faa1f00527934dd35d17542ffe9b08cd020305c75318f36537e1a5d0db4dfcdc94a9708a84a38f81ea7cfbe239252e01f20580897a0500050005000502050005040504040004000502d81bd601e4c6a70505d6029072017300d603e4c6a70805d604ef9372037301d605b2a5730200d60693c27205c2a7d607db63087205d60891b172077303d6098cb2db6308a773040001d60aeded7208938cb27207730500017209938cb27207730600027307d60bc17205d60cc1a7d60d92720b720cd60ee4c672050505d60f7ea305d61093720e720fd611e4c672050805d6129372117203d613e4c67205040ed614e4c67205060ed615e4c672050705d6169172017308d61791720f9a72017309d618b2a5730a00d619e4c6a7060ed61ae4c6a70705d61b937213e4c6a7040eea0298730b830308730c730d730ed1ececececececededededededed720272047206720a720d72107212937213cbb372147a7215ededededed721672047217ed93c27218721992c17218721aededed720692720b9999720c721a730f93720e73109372117311720aededededed72167206720d93720e73127212720aedededed72047206720dedededed721b93720e72019372147219937215721a7212720aededededed72047206720d9372117313ededed721b93720e72019372147219937215721a720aededededededed7202720493720373147206720d72109372117315720aeded93720373167217eded7208938cb27207731700017209938cb27207731800027319";

// LP fund governance contract v1.1 — from 03-lp-fund-tests
// Used in test 4b (accumulation → governance transfer)
const LP_GOV_TREE_HEX: &str = "10140502040004000400040004000502040408cd034646ae5047316b4230d0086c8acec687f00b1cd9d1dc634f6cb358ac0a9a8fff08cd0288e2ddeb04657dbd0edadf9c1f98da3b3895faa1f00527934dd35d17542ffe9b08cd020305c75318f36537e1a5d0db4dfcdc94a9708a84a38f81ea7cfbe239252e01f20402050205000504050405c0ca01040004000502d812d601e4c6a70505d602ef9372017300d603b2a5730100d60493c27203c2a7d605e4c67203041ad606e4c6a7041ad6079372057206d608e4c672030505d6099372087201d60ae4c672030605d60be4c6a70605d60c93720a720bd60ddb63087203d60e91b1720d7302d60f8cb2db6308a773030001d610eded720e938cb2720d73040001720f938cb2720d730500027306d61192c17203c1a7d6127ea305ea0298730783030873087309730ad1ecececececedededededed7202720472077209720cafb4a5730bb1a5d9011363ae7206d901150e937215cbc272137210ededededededed7202720472117209720c91b17205b17206af7206d901130eae7205d901150e93721572137210edededededed72027204721172077209720c7210ededededed7202720472117207937208730c7210ededededededed7202937201730d720472117207937208730e93720a72127210eded937201730f9172129a720b7310eded720e938cb2720d73110001720f938cb2720d731200027313";

// Dummy script: sigmaProp(HEIGHT > 0) — used for wrong-script rejection tests
const DUMMY_TREE_HEX: &str = "10010400d191a37300";

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

fn treasury_reward(reward: u64) -> u64 {
    reward * TREASURY_PCT / 100
}

fn lp_reward(reward: u64) -> u64 {
    reward * LP_PCT / 100
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

/// Build a valid normal-path coinbase scenario at any height using real
/// accumulation contract scripts. Returns (emission_tree, self_box, outputs, height).
fn coinbase_scenario(
    h: u32,
    self_val: u64,
    creation_height: u32,
) -> (ErgoTree, ErgoBox, Vec<ErgoBox>, u32) {
    let emission_tree = load_tree(EMISSION_TREE_HEX);
    let treasury_accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
    let lp_accum_tree = load_tree(LP_ACCUM_TREE_HEX);

    let t_hash = proposition_hash(&treasury_accum_tree);
    let l_hash = proposition_hash(&lp_accum_tree);

    let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, creation_height);
    let reward = block_reward(h);
    let t_amt = treasury_reward(reward);
    let l_amt = lp_reward(reward);

    let outputs = vec![
        make_next_emission_box(&self_box, self_val - reward, h),
        make_output_box(&treasury_accum_tree, t_amt, h),
        make_output_box(&lp_accum_tree, l_amt, h),
    ];

    (emission_tree, self_box, outputs, h)
}

// ============================================================
// PHASE 0: SETUP VERIFICATION
// ============================================================

#[cfg(test)]
mod phase0_setup {
    use super::*;

    #[test]
    fn all_trees_round_trip() {
        let trees = [
            ("emission", EMISSION_TREE_HEX),
            ("treasury_accum", TREASURY_ACCUM_TREE_HEX),
            ("lp_accum", LP_ACCUM_TREE_HEX),
            ("treasury_gov", TREASURY_GOV_TREE_HEX),
            ("lp_gov", LP_GOV_TREE_HEX),
        ];

        for (name, hex) in &trees {
            let tree = load_tree(hex);
            let bytes = tree.sigma_serialize_bytes().unwrap();
            let original = hex_to_bytes(hex);
            assert_eq!(bytes, original, "{} round-trip mismatch", name);
        }
    }

    #[test]
    fn all_propositions_parse() {
        let trees = [
            ("emission", EMISSION_TREE_HEX),
            ("treasury_accum", TREASURY_ACCUM_TREE_HEX),
            ("lp_accum", LP_ACCUM_TREE_HEX),
            ("treasury_gov", TREASURY_GOV_TREE_HEX),
            ("lp_gov", LP_GOV_TREE_HEX),
        ];

        for (name, hex) in &trees {
            let tree = load_tree(hex);
            tree.proposition().unwrap_or_else(|e| panic!("{} proposition failed: {:?}", name, e));
        }
    }

    #[test]
    fn accumulation_hashes_are_distinct() {
        let t_hash = proposition_hash(&load_tree(TREASURY_ACCUM_TREE_HEX));
        let l_hash = proposition_hash(&load_tree(LP_ACCUM_TREE_HEX));
        assert_ne!(t_hash, l_hash, "treasury and LP accumulation hashes must differ");
    }

    #[test]
    fn print_accumulation_hashes() {
        let t_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let l_tree = load_tree(LP_ACCUM_TREE_HEX);

        let t_hash = proposition_hash(&t_tree);
        let l_hash = proposition_hash(&l_tree);

        let t_hex: String = t_hash.iter().map(|b| format!("{:02x}", b)).collect();
        let l_hex: String = l_hash.iter().map(|b| format!("{:02x}", b)).collect();

        println!("treasury accumulation hash (R4): {}", t_hex);
        println!("treasury accumulation tree size: {} bytes", t_tree.sigma_serialize_bytes().unwrap().len());
        println!("LP accumulation hash (R5):       {}", l_hex);
        println!("LP accumulation tree size:       {} bytes", l_tree.sigma_serialize_bytes().unwrap().len());

        let e_tree = load_tree(EMISSION_TREE_HEX);
        println!("emission tree size:              {} bytes", e_tree.sigma_serialize_bytes().unwrap().len());
    }

    /// Verify the governance hashes embedded in the accumulation contracts match
    /// the actual compiled governance ErgoTree bytes.
    #[test]
    fn accumulation_governance_hashes_are_consistent() {
        // Treasury accumulation embeds ecce6bc5... as its governance hash
        let treasury_gov_tree = load_tree(TREASURY_GOV_TREE_HEX);
        let treasury_gov_hash = proposition_hash(&treasury_gov_tree);
        let expected_t: Vec<u8> = hex_to_bytes("ecce6bc576975c9c17246f316caa34917705fc09416a83326c7af666f599180a");
        assert_eq!(treasury_gov_hash, expected_t,
            "treasury governance hash doesn't match accumulation contract constant");

        // LP accumulation embeds f7e52722... as its governance hash
        let lp_gov_tree = load_tree(LP_GOV_TREE_HEX);
        let lp_gov_hash = proposition_hash(&lp_gov_tree);
        let expected_l: Vec<u8> = hex_to_bytes("f7e52722204eab03cf13bea0772e0e00ee48f4a6f81396514d9a3b692d61b1e4");
        assert_eq!(lp_gov_hash, expected_l,
            "LP governance hash doesn't match accumulation contract constant");
    }
}

// ============================================================
// TEST 1: HAPPY PATH AT HEIGHT 100
// ============================================================

#[cfg(test)]
mod test1_happy_path_h100 {
    use super::*;

    #[test]
    fn accept_coinbase_at_height_100() {
        let h = 100u32;
        let self_val = 200 * NANOCOIN;
        let (tree, self_box, outputs, h) = coinbase_scenario(h, self_val, h - 1);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx),
            "full coinbase TX at h=100 should accept with real accumulation scripts");
    }

    #[test]
    fn verify_amounts_at_height_100() {
        let reward = block_reward(100);
        assert_eq!(reward, 50 * NANOCOIN, "epoch 0 reward should be 50 coins");
        assert_eq!(treasury_reward(reward), 5 * NANOCOIN, "treasury 10% = 5 coins");
        assert_eq!(lp_reward(reward), 2_500_000_000, "LP 5% = 2.5 coins");
    }
}

// ============================================================
// TEST 2: HAPPY PATH AT HALVING BOUNDARY (h=1,577,880)
// ============================================================

#[cfg(test)]
mod test2_halving_boundary {
    use super::*;

    #[test]
    fn accept_coinbase_at_first_halving() {
        let h = BLOCKS_PER_HALVING; // 1,577,880
        let self_val = 200 * NANOCOIN;
        let (tree, self_box, outputs, h) = coinbase_scenario(h, self_val, h - 1);
        let ctx = build_context(self_box, outputs, h);
        assert!(evaluate(&tree, &ctx),
            "coinbase at halving boundary should accept with halved reward");
    }

    #[test]
    fn verify_amounts_at_halving() {
        let reward = block_reward(BLOCKS_PER_HALVING);
        assert_eq!(reward, 25 * NANOCOIN, "epoch 1 reward should be 25 coins");
        assert_eq!(treasury_reward(reward), 2_500_000_000, "treasury 10% of 25 = 2.5 coins");
        assert_eq!(lp_reward(reward), 1_250_000_000, "LP 5% of 25 = 1.25 coins");
    }
}

// ============================================================
// TEST 3: TREASURY ACCUMULATION IS INDEPENDENTLY SPENDABLE
// ============================================================

#[cfg(test)]
mod test3_treasury_accum_spendable {
    use super::*;

    /// 3a: Treasury accumulation box can consolidate with itself.
    #[test]
    fn accept_treasury_accum_consolidation() {
        let accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let h = 100u32;
        let value = 5 * NANOCOIN; // matches treasury reward at 50-coin block

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&accum_tree, value + 3 * NANOCOIN, h); // value grown
        let ctx = build_context(self_box, vec![out0], h);
        assert!(evaluate(&accum_tree, &ctx),
            "treasury accumulation consolidation should accept");
    }

    /// 3b: Treasury accumulation box can transfer to treasury governance.
    /// This proves the emission → accumulation → governance pipeline.
    #[test]
    fn accept_treasury_accum_transfer_to_governance() {
        let accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let gov_tree = load_tree(TREASURY_GOV_TREE_HEX);
        let h = 100u32;
        let value = 5 * NANOCOIN;

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&gov_tree, value, h);
        let ctx = build_context(self_box, vec![out0], h);
        assert!(evaluate(&accum_tree, &ctx),
            "treasury accumulation transfer to governance should accept");
    }

    /// 3c: Treasury accumulation rejects transfer to wrong destination.
    #[test]
    fn reject_treasury_accum_wrong_destination() {
        let accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let wrong_tree = load_tree(DUMMY_TREE_HEX);
        let h = 100u32;
        let value = 5 * NANOCOIN;

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&wrong_tree, value, h);
        let ctx = build_context(self_box, vec![out0], h);
        assert!(!evaluate(&accum_tree, &ctx),
            "treasury accumulation transfer to wrong script should reject");
    }
}

// ============================================================
// TEST 4: LP ACCUMULATION IS INDEPENDENTLY SPENDABLE
// ============================================================

#[cfg(test)]
mod test4_lp_accum_spendable {
    use super::*;

    /// 4a: LP accumulation box can consolidate with itself.
    #[test]
    fn accept_lp_accum_consolidation() {
        let accum_tree = load_tree(LP_ACCUM_TREE_HEX);
        let h = 100u32;
        let value = 2_500_000_000; // matches LP reward at 50-coin block

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&accum_tree, value + NANOCOIN, h); // value grown
        let ctx = build_context(self_box, vec![out0], h);
        assert!(evaluate(&accum_tree, &ctx),
            "LP accumulation consolidation should accept");
    }

    /// 4b: LP accumulation box can transfer to LP governance.
    /// This proves the emission → accumulation → governance pipeline.
    #[test]
    fn accept_lp_accum_transfer_to_governance() {
        let accum_tree = load_tree(LP_ACCUM_TREE_HEX);
        let gov_tree = load_tree(LP_GOV_TREE_HEX);
        let h = 100u32;
        let value = 2_500_000_000;

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&gov_tree, value, h);
        let ctx = build_context(self_box, vec![out0], h);
        assert!(evaluate(&accum_tree, &ctx),
            "LP accumulation transfer to governance should accept");
    }

    /// 4c: LP accumulation rejects transfer to wrong destination.
    #[test]
    fn reject_lp_accum_wrong_destination() {
        let accum_tree = load_tree(LP_ACCUM_TREE_HEX);
        let wrong_tree = load_tree(DUMMY_TREE_HEX);
        let h = 100u32;
        let value = 2_500_000_000;

        let self_box = make_output_box(&accum_tree, value, h - 1);
        let out0 = make_output_box(&wrong_tree, value, h);
        let ctx = build_context(self_box, vec![out0], h);
        assert!(!evaluate(&accum_tree, &ctx),
            "LP accumulation transfer to wrong script should reject");
    }
}

// ============================================================
// TEST 5: WRONG TREASURY SCRIPT → REJECT
// ============================================================

#[cfg(test)]
mod test5_wrong_treasury_script {
    use super::*;

    #[test]
    fn reject_wrong_treasury_script() {
        let h = 100u32;
        let self_val = 200 * NANOCOIN;

        let emission_tree = load_tree(EMISSION_TREE_HEX);
        let treasury_accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let lp_accum_tree = load_tree(LP_ACCUM_TREE_HEX);
        let wrong_tree = load_tree(DUMMY_TREE_HEX);

        let t_hash = proposition_hash(&treasury_accum_tree);
        let l_hash = proposition_hash(&lp_accum_tree);

        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, h - 1);
        let reward = block_reward(h);
        let t_amt = treasury_reward(reward);
        let l_amt = lp_reward(reward);

        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h),
            make_output_box(&wrong_tree, t_amt, h),       // WRONG: not treasury accum
            make_output_box(&lp_accum_tree, l_amt, h),
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx),
            "wrong treasury script should reject — blake2b256 hash mismatch with R4");
    }
}

// ============================================================
// TEST 6: WRONG LP SCRIPT → REJECT
// ============================================================

#[cfg(test)]
mod test6_wrong_lp_script {
    use super::*;

    #[test]
    fn reject_wrong_lp_script() {
        let h = 100u32;
        let self_val = 200 * NANOCOIN;

        let emission_tree = load_tree(EMISSION_TREE_HEX);
        let treasury_accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let lp_accum_tree = load_tree(LP_ACCUM_TREE_HEX);
        let wrong_tree = load_tree(DUMMY_TREE_HEX);

        let t_hash = proposition_hash(&treasury_accum_tree);
        let l_hash = proposition_hash(&lp_accum_tree);

        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, h - 1);
        let reward = block_reward(h);
        let t_amt = treasury_reward(reward);
        let l_amt = lp_reward(reward);

        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h),
            make_output_box(&treasury_accum_tree, t_amt, h),
            make_output_box(&wrong_tree, l_amt, h),        // WRONG: not LP accum
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx),
            "wrong LP script should reject — blake2b256 hash mismatch with R5");
    }
}

// ============================================================
// TEST 7: SWAPPED OUTPUT POSITIONS → REJECT
// ============================================================

#[cfg(test)]
mod test7_swapped_positions {
    use super::*;

    #[test]
    fn reject_swapped_treasury_lp_positions() {
        let h = 100u32;
        let self_val = 200 * NANOCOIN;

        let emission_tree = load_tree(EMISSION_TREE_HEX);
        let treasury_accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let lp_accum_tree = load_tree(LP_ACCUM_TREE_HEX);

        let t_hash = proposition_hash(&treasury_accum_tree);
        let l_hash = proposition_hash(&lp_accum_tree);

        let self_box = make_emission_box(&emission_tree, self_val, &t_hash, &l_hash, h - 1);
        let reward = block_reward(h);
        let t_amt = treasury_reward(reward);
        let l_amt = lp_reward(reward);

        // SWAPPED: LP accum at slot 1 (should be treasury), treasury accum at slot 2 (should be LP)
        let outputs = vec![
            make_next_emission_box(&self_box, self_val - reward, h),
            make_output_box(&lp_accum_tree, t_amt, h),       // slot 1: LP script, treasury value
            make_output_box(&treasury_accum_tree, l_amt, h),  // slot 2: treasury script, LP value
        ];
        let ctx = build_context(self_box, outputs, h);
        assert!(!evaluate(&emission_tree, &ctx),
            "swapped output positions should reject — hashes don't match respective registers");
    }
}

// ============================================================
// TEST 8: MULTI-BLOCK SEQUENCE ACROSS HALVING
// ============================================================

#[cfg(test)]
mod test8_multi_block_halving {
    use super::*;

    #[test]
    fn accept_three_block_chain_across_halving() {
        let emission_tree = load_tree(EMISSION_TREE_HEX);
        let treasury_accum_tree = load_tree(TREASURY_ACCUM_TREE_HEX);
        let lp_accum_tree = load_tree(LP_ACCUM_TREE_HEX);

        let t_hash = proposition_hash(&treasury_accum_tree);
        let l_hash = proposition_hash(&lp_accum_tree);

        let starting_value = 200 * NANOCOIN;

        // ---- Block 1,577,879 (epoch 0, reward = 50 coins) ----
        let h0 = BLOCKS_PER_HALVING - 1; // 1,577,879
        let reward0 = block_reward(h0);
        assert_eq!(reward0, 50 * NANOCOIN, "block before halving should have 50-coin reward");

        let emission_box_0 = make_emission_box(
            &emission_tree, starting_value, &t_hash, &l_hash, h0 - 1,
        );
        let val_after_0 = starting_value - reward0;
        let outputs_0 = vec![
            make_next_emission_box(&emission_box_0, val_after_0, h0),
            make_output_box(&treasury_accum_tree, treasury_reward(reward0), h0),
            make_output_box(&lp_accum_tree, lp_reward(reward0), h0),
        ];
        let ctx_0 = build_context(emission_box_0, outputs_0.clone(), h0);
        assert!(evaluate(&emission_tree, &ctx_0),
            "block 1,577,879 (epoch 0) should accept");

        // ---- Block 1,577,880 (epoch 1, reward = 25 coins) ----
        let h1 = BLOCKS_PER_HALVING; // 1,577,880
        let reward1 = block_reward(h1);
        assert_eq!(reward1, 25 * NANOCOIN, "first halving block should have 25-coin reward");

        // Chain: output emission box from block 0 becomes input for block 1.
        // Reconstruct with correct creation_height from previous output.
        let emission_box_1 = make_emission_box(
            &emission_tree, val_after_0, &t_hash, &l_hash, h0,
        );
        let val_after_1 = val_after_0 - reward1;
        let outputs_1 = vec![
            make_next_emission_box(&emission_box_1, val_after_1, h1),
            make_output_box(&treasury_accum_tree, treasury_reward(reward1), h1),
            make_output_box(&lp_accum_tree, lp_reward(reward1), h1),
        ];
        let ctx_1 = build_context(emission_box_1, outputs_1.clone(), h1);
        assert!(evaluate(&emission_tree, &ctx_1),
            "block 1,577,880 (epoch 1, halving boundary) should accept");

        // ---- Block 1,577,881 (epoch 1, reward = 25 coins) ----
        let h2 = BLOCKS_PER_HALVING + 1; // 1,577,881
        let reward2 = block_reward(h2);
        assert_eq!(reward2, 25 * NANOCOIN, "block after halving should still be 25-coin reward");

        let emission_box_2 = make_emission_box(
            &emission_tree, val_after_1, &t_hash, &l_hash, h1,
        );
        let val_after_2 = val_after_1 - reward2;
        let outputs_2 = vec![
            make_next_emission_box(&emission_box_2, val_after_2, h2),
            make_output_box(&treasury_accum_tree, treasury_reward(reward2), h2),
            make_output_box(&lp_accum_tree, lp_reward(reward2), h2),
        ];
        let ctx_2 = build_context(emission_box_2, outputs_2, h2);
        assert!(evaluate(&emission_tree, &ctx_2),
            "block 1,577,881 (epoch 1, post-halving) should accept");

        // ---- Verify value chain ----
        assert_eq!(val_after_2, starting_value - 50 * NANOCOIN - 25 * NANOCOIN - 25 * NANOCOIN,
            "remaining emission value should equal start minus 100 coins total");
        println!("3-block chain across halving: OK");
        println!("  block 1,577,879: reward=50, remaining={}", val_after_0);
        println!("  block 1,577,880: reward=25, remaining={}", val_after_1);
        println!("  block 1,577,881: reward=25, remaining={}", val_after_2);
    }
}
