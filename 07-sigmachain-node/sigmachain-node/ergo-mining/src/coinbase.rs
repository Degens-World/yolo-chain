//! Coinbase transaction assembly: pre-EIP-27 emission tx and fee tx.
//!
//! Port of `CandidateGenerator.collectRewards` from
//! `src/main/scala/org/ergoplatform/mining/CandidateGenerator.scala:713-820`,
//! pre-EIP-27 path only. The EIP-27 activation + post-activation paths
//! live in `crate::reemission`.
//!
//! Two transactions, in this order in the block:
//!
//! 1. **Emission tx**: consumes the current emission box, emits an
//!    updated emission box (value reduced by `miners_reward_at_height`)
//!    plus a single miner reward box. One input (the emission box,
//!    empty proof), two outputs.
//!
//! 2. **Fee tx** (optional, skipped when total fees == 0): consumes
//!    all fee-locked outputs from selected user transactions (boxes
//!    whose `ergo_tree_bytes == MAINNET_FEE_PROPOSITION_BYTES` and
//!    that aren't spent within the block), aggregates their values
//!    and tokens into a single miner reward box.

#[cfg(test)]
use ergo_primitives::digest::Digest32;
use ergo_primitives::digest::{blake2b256, ModifierId};
use ergo_ser::ergo_box::{ErgoBox, ErgoBoxCandidate};
use ergo_ser::input::{ContextExtension, Input, SpendingProof};
use ergo_ser::register::AdditionalRegisters;
use ergo_ser::token::Token;
use ergo_ser::transaction::{bytes_to_sign, Transaction};

use crate::emission_rules::{
    miners_reward_at_height, yolo_emission_at_height, yolo_lp_reward_at_height,
    yolo_miners_reward_at_height, yolo_treasury_reward_at_height, MonetarySettings,
    YoloEmissionParams,
};
use crate::error::MiningError;
use crate::reward_script::reward_output_script;

/// Maximum tokens per ErgoBox per Scala
/// `sdk.wallet.Constants.MaxAssetsPerBox`.
pub const MAX_ASSETS_PER_BOX: usize = 255;

/// Mainnet fee-output proposition bytes, mirrored from
/// `ergo-mempool/src/validator.rs:35-43` so this crate can detect
/// fee boxes without depending on `ergo-mempool`. Identity is
/// `Scala::MonetarySettings.feePropositionBytes` =
/// `ErgoTreePredef.feeProposition(720).bytes`.
const MAINNET_FEE_PROPOSITION_BYTES: &[u8] = &[
    0x10, 0x05, 0x04, 0x00, 0x04, 0x00, 0x0e, 0x36, 0x10, 0x02, 0x04, 0xa0, 0x0b, 0x08, 0xcd, 0x02,
    0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac, 0x55, 0xa0, 0x62, 0x95, 0xce, 0x87, 0x0b, 0x07,
    0x02, 0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2, 0x81, 0x5b, 0x16, 0xf8, 0x17, 0x98,
    0xea, 0x02, 0xd1, 0x92, 0xa3, 0x9a, 0x8c, 0xc7, 0xa7, 0x01, 0x73, 0x00, 0x73, 0x01, 0x10, 0x01,
    0x02, 0x04, 0x02, 0xd1, 0x96, 0x83, 0x03, 0x01, 0x93, 0xa3, 0x8c, 0xc7, 0xb2, 0xa5, 0x73, 0x00,
    0x00, 0x01, 0x93, 0xc2, 0xb2, 0xa5, 0x73, 0x01, 0x00, 0x74, 0x73, 0x02, 0x73, 0x03, 0x83, 0x01,
    0x08, 0xcd, 0xee, 0xac, 0x93, 0xb1, 0xa5, 0x73, 0x04,
];

/// Build the pre-EIP-27 emission transaction for a candidate at
/// `next_height`. Mirrors the pre-activation branch of
/// `CandidateGenerator.collectRewards` (`CandidateGenerator.scala:736-800`).
///
/// `input_emission_box` is the current emission box in state.
/// `miner_pk` is the 33-byte compressed secp256k1 reward pubkey.
///
/// The emission box's `value` decreases by
/// `miners_reward_at_height(next_height, &mainnet)`; its tokens are
/// preserved verbatim (pre-EIP-27 the emission box has no tokens on
/// mainnet, but tests / testnet may differ). The miner reward box
/// carries `miners_reward_at_height` nanoERG, no tokens, no registers,
/// and uses the `reward_output_script(miner_pk)` lock.
pub fn build_pre_eip27_emission_tx(
    input_emission_box: &ErgoBox,
    miner_pk: &[u8; 33],
    next_height: u32,
    settings: &MonetarySettings,
) -> Result<Transaction, MiningError> {
    let emission_amount = miners_reward_at_height(next_height, settings);
    let input_value = input_emission_box.candidate.value;
    if emission_amount > input_value {
        return Err(MiningError::EmissionInvariant {
            op: "build_pre_eip27_emission_tx",
            reason: format!(
                "emission box value {input_value} < per-block emission {emission_amount} \
                 at h={next_height}: emission curve is exhausted or input is wrong",
            ),
        });
    }

    // Updated emission box: same script, same tokens (pre-EIP-27),
    // value reduced, height bumped.
    let new_emission_box = ErgoBoxCandidate::from_trusted_raw_parts(
        input_value - emission_amount,
        input_emission_box.candidate.ergo_tree().clone(),
        input_emission_box.candidate.ergo_tree_bytes().to_vec(),
        next_height,
        input_emission_box.candidate.tokens.clone(),
        AdditionalRegisters::empty(),
        Vec::new(), // empty registers serialize to a single `0x00` count byte; we let new() re-serialize below
    );
    // Re-derive register bytes through new() to be safe (we used trusted_raw_parts above for the tree bytes).
    let new_emission_box = ErgoBoxCandidate::new(
        new_emission_box.value,
        new_emission_box.ergo_tree().clone(),
        next_height,
        new_emission_box.tokens.clone(),
        AdditionalRegisters::empty(),
    )
    .map_err(|e| MiningError::IdComputation {
        op: "new_emission_box",
        reason: format!("{e:?}"),
    })?;

    // Miner reward box: 54-byte reward script, no tokens, no registers.
    let reward_script_bytes = reward_output_script(miner_pk).to_vec();
    let reward_tree = parse_ergo_tree(&reward_script_bytes)?;
    let miner_box = ErgoBoxCandidate::from_trusted_raw_parts(
        emission_amount,
        reward_tree,
        reward_script_bytes,
        next_height,
        Vec::new(),
        AdditionalRegisters::empty(),
        vec![0x00], // empty-register block serializes as a single 0x00 count byte
    );

    let input = Input {
        box_id: input_emission_box
            .box_id()
            .map_err(|e| MiningError::IdComputation {
                op: "emission_box_id",
                reason: format!("{e:?}"),
            })?,
        spending_proof: SpendingProof::new(Vec::new(), ContextExtension::empty()).map_err(|e| {
            MiningError::IdComputation {
                op: "empty_spending_proof",
                reason: format!("{e:?}"),
            }
        })?,
    };

    Ok(Transaction {
        inputs: vec![input],
        data_inputs: Vec::new(),
        output_candidates: vec![new_emission_box, miner_box],
    })
}

/// Build the SigmaChain (YOLO) emission transaction for a candidate
/// at `next_height`. Four outputs in fixed order so emission.es can
/// reference them positionally:
///
/// - `OUTPUTS(0)` — new emission box, value reduced by the full
///   `block_reward(next_height)`, with the input's tokens, ergo_tree,
///   R4 (treasury hash), and R5 (LP hash) preserved verbatim and
///   `creationHeight` bumped to `next_height`.
/// - `OUTPUTS(1)` — treasury accumulation box, value =
///   `treasury_reward = block_reward * treasury_share_bps / 10_000`,
///   locked by `treasury_script_bytes` (must hash to the input's R4).
/// - `OUTPUTS(2)` — LP accumulation box, value = `lp_reward = block_reward
///   * lp_share_bps / 10_000`, locked by `lp_script_bytes` (must hash
///   to the input's R5).
/// - `OUTPUTS(3)` — miner reward box, value = `block_reward -
///   treasury_reward - lp_reward` (≈ 85%), locked by
///   `reward_output_script(miner_pk)`.
///
/// The miner share is computed as the remainder rather than as a fixed
/// 85% so no nanoYOLO is lost to integer division across the three
/// non-emission outputs (matches `YoloEmissionParams::miner_reward_at_height`).
///
/// `treasury_script_bytes` and `lp_script_bytes` are the canonical
/// ErgoTree wire bytes for `treasury_accumulation.es` and
/// `lp_accumulation.es` — same constants that live in
/// `ergo-chain-spec::yolo_genesis_scripts`. The caller is responsible
/// for verifying blake2b256(treasury_script_bytes) ==
/// input_emission_box.R4 (similarly for LP/R5) BEFORE calling this
/// builder; emission.es enforces the same invariant on-chain, but a
/// well-behaved miner avoids producing a block the validator will
/// reject by checking up front.
pub fn build_yolo_emission_tx(
    input_emission_box: &ErgoBox,
    miner_pk: &[u8; 33],
    next_height: u32,
    yolo: &YoloEmissionParams,
    treasury_script_bytes: &[u8],
    lp_script_bytes: &[u8],
) -> Result<Transaction, MiningError> {
    let block_reward = yolo_emission_at_height(next_height, yolo);
    let treasury_reward = yolo_treasury_reward_at_height(next_height, yolo);
    let lp_reward = yolo_lp_reward_at_height(next_height, yolo);
    let miner_reward = yolo_miners_reward_at_height(next_height, yolo);
    debug_assert_eq!(
        miner_reward + treasury_reward + lp_reward,
        block_reward,
        "YoloEmissionParams split invariant violated at h={next_height}",
    );

    let input_value = input_emission_box.candidate.value;
    if block_reward > input_value {
        return Err(MiningError::EmissionInvariant {
            op: "build_yolo_emission_tx",
            reason: format!(
                "emission box value {input_value} < block reward {block_reward} \
                 at h={next_height}: SigmaChain emission curve is exhausted or input is wrong",
            ),
        });
    }

    // Output 0: updated emission box. Script, tokens, and R4/R5
    // registers are preserved verbatim — emission.es checks all four.
    let new_emission_box = ErgoBoxCandidate::new(
        input_value - block_reward,
        input_emission_box.candidate.ergo_tree().clone(),
        next_height,
        input_emission_box.candidate.tokens.clone(),
        input_emission_box.candidate.additional_registers.clone(),
    )
    .map_err(|e| MiningError::IdComputation {
        op: "yolo_new_emission_box",
        reason: format!("{e:?}"),
    })?;

    // Output 1: treasury accumulation box. emission.es line 102 checks
    // blake2b256(propositionBytes) == R4; the script bytes must hash
    // to the value in input R4 or this block will be rejected.
    let treasury_tree = parse_ergo_tree(treasury_script_bytes)?;
    let treasury_box = ErgoBoxCandidate::from_trusted_raw_parts(
        treasury_reward,
        treasury_tree,
        treasury_script_bytes.to_vec(),
        next_height,
        Vec::new(),
        AdditionalRegisters::empty(),
        vec![0x00],
    );

    // Output 2: LP accumulation box. emission.es line 108 mirrors the
    // treasury check with R5.
    let lp_tree = parse_ergo_tree(lp_script_bytes)?;
    let lp_box = ErgoBoxCandidate::from_trusted_raw_parts(
        lp_reward,
        lp_tree,
        lp_script_bytes.to_vec(),
        next_height,
        Vec::new(),
        AdditionalRegisters::empty(),
        vec![0x00],
    );

    // Output 3: miner reward box. Same shape as the Ergo coinbase's
    // miner box — 54-byte reward script gated by HEIGHT.
    let reward_script_bytes = reward_output_script(miner_pk).to_vec();
    let reward_tree = parse_ergo_tree(&reward_script_bytes)?;
    let miner_box = ErgoBoxCandidate::from_trusted_raw_parts(
        miner_reward,
        reward_tree,
        reward_script_bytes,
        next_height,
        Vec::new(),
        AdditionalRegisters::empty(),
        vec![0x00],
    );

    let input = Input {
        box_id: input_emission_box
            .box_id()
            .map_err(|e| MiningError::IdComputation {
                op: "yolo_emission_box_id",
                reason: format!("{e:?}"),
            })?,
        spending_proof: SpendingProof::new(Vec::new(), ContextExtension::empty()).map_err(|e| {
            MiningError::IdComputation {
                op: "yolo_empty_spending_proof",
                reason: format!("{e:?}"),
            }
        })?,
    };

    Ok(Transaction {
        inputs: vec![input],
        data_inputs: Vec::new(),
        output_candidates: vec![new_emission_box, treasury_box, lp_box, miner_box],
    })
}

/// Build the fee transaction. Returns `None` when the selected
/// transactions produce no fee-locked outputs (or all such outputs
/// are spent within the same block).
///
/// Mirrors `CandidateGenerator.collectRewards` lines 803-820.
pub fn build_fee_tx(
    selected_user_txs: &[Transaction],
    miner_pk: &[u8; 33],
    next_height: u32,
) -> Result<Option<Transaction>, MiningError> {
    let fee_boxes = find_unspent_fee_boxes(selected_user_txs)?;
    if fee_boxes.is_empty() {
        return Ok(None);
    }

    let total_value: u64 = fee_boxes.iter().map(|b| b.candidate.value).sum();
    // Aggregate tokens, capped at MAX_ASSETS_PER_BOX.
    let mut combined_tokens: Vec<Token> = Vec::new();
    for b in &fee_boxes {
        for t in &b.candidate.tokens {
            if combined_tokens.len() >= MAX_ASSETS_PER_BOX {
                break;
            }
            combined_tokens.push(t.clone());
        }
    }

    let reward_script_bytes = reward_output_script(miner_pk).to_vec();
    let reward_tree = parse_ergo_tree(&reward_script_bytes)?;
    let miner_box = ErgoBoxCandidate::from_trusted_raw_parts(
        total_value,
        reward_tree,
        reward_script_bytes,
        next_height,
        combined_tokens,
        AdditionalRegisters::empty(),
        vec![0x00],
    );

    let inputs: Vec<Input> = fee_boxes
        .iter()
        .map(|b| {
            b.box_id().map(|box_id| Input {
                box_id,
                spending_proof: SpendingProof::new(Vec::new(), ContextExtension::empty())
                    .expect("empty spending proof always builds"),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| MiningError::IdComputation {
            op: "fee_input_box_id",
            reason: format!("{e:?}"),
        })?;

    Ok(Some(Transaction {
        inputs,
        data_inputs: Vec::new(),
        output_candidates: vec![miner_box],
    }))
}

/// Walk `txs` and return every output box whose script bytes match
/// the mainnet fee proposition AND is not spent within `txs` itself.
/// Mirrors the `feeBoxes` computation at `CandidateGenerator.scala:803-806`.
fn find_unspent_fee_boxes(txs: &[Transaction]) -> Result<Vec<ErgoBox>, MiningError> {
    // Collect every input.box_id from the batch, so we can filter
    // out fee outputs that the batch immediately re-spends.
    let mut spent_in_batch: std::collections::HashSet<[u8; 32]> = Default::default();
    for tx in txs {
        for inp in &tx.inputs {
            spent_in_batch.insert(*inp.box_id.as_bytes());
        }
    }

    let mut out = Vec::new();
    for tx in txs {
        // Compute tx_id = blake2b256(bytes_to_sign(tx)).
        let bts = bytes_to_sign(tx).map_err(|e| MiningError::IdComputation {
            op: "bytes_to_sign",
            reason: format!("{e:?}"),
        })?;
        let tx_id: ModifierId = ModifierId::from_bytes(*blake2b256(&bts).as_bytes());

        for (i, candidate) in tx.output_candidates.iter().enumerate() {
            if candidate.ergo_tree_bytes() != MAINNET_FEE_PROPOSITION_BYTES {
                continue;
            }
            let ergo_box = ErgoBox {
                candidate: candidate.clone(),
                transaction_id: tx_id,
                index: i as u16,
            };
            let box_id = ergo_box.box_id().map_err(|e| MiningError::IdComputation {
                op: "fee_output_box_id",
                reason: format!("{e:?}"),
            })?;
            if spent_in_batch.contains(box_id.as_bytes()) {
                continue;
            }
            out.push(ergo_box);
        }
    }
    Ok(out)
}

/// Parse 54-or-more bytes of canonical ErgoTree wire-form into an
/// `ergo_ser::ergo_tree::ErgoTree`.
fn parse_ergo_tree(bytes: &[u8]) -> Result<ergo_ser::ergo_tree::ErgoTree, MiningError> {
    use ergo_primitives::reader::VlqReader;
    let mut r = VlqReader::new(bytes);
    ergo_ser::ergo_tree::read_ergo_tree(&mut r).map_err(|e| MiningError::Decode {
        op: "reward_ergo_tree",
        reason: format!("{e:?}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    // ----- helpers -----

    #[derive(Deserialize)]
    struct EmissionTxFixture {
        #[allow(dead_code)]
        height: u32,
        #[allow(dead_code)]
        header_id: String,
        emission_tx: TxJson,
    }
    #[derive(Deserialize)]
    struct TxJson {
        id: String,
        inputs: Vec<InputJson>,
        #[allow(dead_code)]
        #[serde(rename = "dataInputs")]
        data_inputs: Vec<serde_json::Value>,
        outputs: Vec<OutputJson>,
    }
    #[derive(Deserialize)]
    struct InputJson {
        #[serde(rename = "boxId")]
        box_id: String,
        #[allow(dead_code)]
        #[serde(rename = "spendingProof")]
        spending_proof: serde_json::Value,
    }

    // The `box_id` field IS read by the parity test (mainnet input check).
    impl InputJson {
        #[allow(dead_code)]
        fn _read(&self) -> &str {
            &self.box_id
        }
    }
    #[derive(Deserialize)]
    struct OutputJson {
        #[serde(rename = "boxId")]
        #[allow(dead_code)]
        box_id: String,
        value: u64,
        #[serde(rename = "ergoTree")]
        ergo_tree: String,
        #[serde(rename = "creationHeight")]
        creation_height: u32,
        #[serde(default)]
        assets: Vec<AssetJson>,
    }
    #[derive(Deserialize)]
    struct AssetJson {
        #[serde(rename = "tokenId")]
        token_id: String,
        amount: u64,
    }

    fn load(h: u32) -> EmissionTxFixture {
        let path = format!(
            "{}/../test-vectors/mining/emission_txs/{}.json",
            env!("CARGO_MANIFEST_DIR"),
            h
        );
        let bytes = std::fs::read(&path).expect("read");
        serde_json::from_slice(&bytes).expect("parse")
    }

    fn ergo_box_from_output(o: &OutputJson, tx_id_hex: &str, index: u16) -> ErgoBox {
        let tree_bytes = hex::decode(&o.ergo_tree).expect("tree hex");
        let tree = parse_ergo_tree(&tree_bytes).expect("parse tree");
        let tokens: Vec<Token> = o
            .assets
            .iter()
            .map(|a| {
                let mut id = [0u8; 32];
                hex::decode_to_slice(&a.token_id, &mut id).expect("token hex");
                Token {
                    token_id: Digest32::from_bytes(id),
                    amount: a.amount,
                }
            })
            .collect();
        let candidate = ErgoBoxCandidate::from_trusted_raw_parts(
            o.value,
            tree,
            tree_bytes,
            o.creation_height,
            tokens,
            AdditionalRegisters::empty(),
            vec![0x00],
        );
        let mut tx_id_arr = [0u8; 32];
        hex::decode_to_slice(tx_id_hex, &mut tx_id_arr).expect("tx_id hex");
        ErgoBox {
            candidate,
            transaction_id: ModifierId::from_bytes(tx_id_arr),
            index,
        }
    }

    /// Recover the miner pubkey from a real reward box's ergoTree at
    /// the canonical pk offset (7..40 of the 54-byte script).
    fn miner_pk_from_reward_tree_hex(tree_hex: &str) -> [u8; 33] {
        let raw = hex::decode(tree_hex).expect("tree hex");
        let mut pk = [0u8; 33];
        pk.copy_from_slice(&raw[7..40]);
        pk
    }

    // ----- happy path -----

    #[test]
    fn pre_eip27_emission_tx_matches_mainnet_at_700000() {
        // Parent emission box: output[0] of h=699999's emission tx.
        let parent = load(699_999);
        let input_box =
            ergo_box_from_output(&parent.emission_tx.outputs[0], &parent.emission_tx.id, 0);

        // Target: h=700000's emission tx.
        let target = load(700_000);
        let miner_pk = miner_pk_from_reward_tree_hex(&target.emission_tx.outputs[1].ergo_tree);

        let settings = MonetarySettings::mainnet();
        let built = build_pre_eip27_emission_tx(&input_box, &miner_pk, 700_000, &settings)
            .expect("build emission tx");

        // Inputs: single input pointing at parent emission box.
        assert_eq!(built.inputs.len(), 1);
        assert_eq!(built.data_inputs.len(), 0);
        assert_eq!(built.output_candidates.len(), 2);
        let captured_input_box_id = hex::decode(&target.emission_tx.inputs[0].box_id).expect("hex");
        assert_eq!(
            built.inputs[0].box_id.as_bytes()[..],
            captured_input_box_id[..],
            "emission input box_id mismatch (input == h=699999.tx.outputs[0].box_id)"
        );
        // Empty spending proof.
        assert!(built.inputs[0].spending_proof.proof.is_empty());
        assert!(built.inputs[0].spending_proof.extension.is_empty());

        // Output[0] = new emission box.
        let new_em = &built.output_candidates[0];
        assert_eq!(new_em.value, target.emission_tx.outputs[0].value);
        assert_eq!(new_em.creation_height, 700_000);
        let captured_tree = hex::decode(&target.emission_tx.outputs[0].ergo_tree).expect("hex");
        assert_eq!(
            new_em.ergo_tree_bytes(),
            &captured_tree[..],
            "new emission box ergo_tree must equal input emission box's ergo_tree"
        );
        assert!(
            new_em.tokens.is_empty(),
            "pre-EIP-27 emission has no tokens"
        );

        // Output[1] = miner reward box.
        let miner = &built.output_candidates[1];
        assert_eq!(miner.value, target.emission_tx.outputs[1].value);
        assert_eq!(miner.creation_height, 700_000);
        let captured_reward_tree =
            hex::decode(&target.emission_tx.outputs[1].ergo_tree).expect("hex");
        assert_eq!(miner.ergo_tree_bytes(), &captured_reward_tree[..]);
        assert!(miner.tokens.is_empty(), "pre-EIP-27 miner gets no tokens");
    }

    #[test]
    fn pre_eip27_emission_tx_matches_mainnet_at_700001() {
        let parent = load(700_000);
        let input_box =
            ergo_box_from_output(&parent.emission_tx.outputs[0], &parent.emission_tx.id, 0);
        let target = load(700_001);
        let miner_pk = miner_pk_from_reward_tree_hex(&target.emission_tx.outputs[1].ergo_tree);
        let built = build_pre_eip27_emission_tx(
            &input_box,
            &miner_pk,
            700_001,
            &MonetarySettings::mainnet(),
        )
        .expect("build");

        assert_eq!(
            built.output_candidates[0].value,
            target.emission_tx.outputs[0].value
        );
        assert_eq!(
            built.output_candidates[1].value,
            target.emission_tx.outputs[1].value
        );
        assert_eq!(built.output_candidates[1].creation_height, 700_001);
    }

    // ----- fee tx happy path -----

    #[test]
    fn build_fee_tx_returns_none_for_empty_batch() {
        let miner_pk = [0x02u8; 33];
        let fee = build_fee_tx(&[], &miner_pk, 1_000_000).expect("ok");
        assert!(fee.is_none(), "empty user-tx batch must produce no fee tx");
    }

    // ----- error paths -----

    #[test]
    fn pre_eip27_emission_tx_rejects_exhausted_emission_box() {
        // Take a real emission box from the captured corpus and
        // overwrite its value with 1 nanoERG so the per-height
        // emission exceeds the box's value. This exercises the
        // "emission curve exhausted" guard without forging a tree.
        let parent = load(699_999);
        let real_input =
            ergo_box_from_output(&parent.emission_tx.outputs[0], &parent.emission_tx.id, 0);
        // Rebuild with value=1.
        let exhausted_candidate = ErgoBoxCandidate::from_trusted_raw_parts(
            1,
            real_input.candidate.ergo_tree().clone(),
            real_input.candidate.ergo_tree_bytes().to_vec(),
            real_input.candidate.creation_height,
            real_input.candidate.tokens.clone(),
            AdditionalRegisters::empty(),
            vec![0x00],
        );
        let input_box = ErgoBox {
            candidate: exhausted_candidate,
            transaction_id: real_input.transaction_id,
            index: 0,
        };
        let err = build_pre_eip27_emission_tx(
            &input_box,
            &[0x02u8; 33],
            700_000,
            &MonetarySettings::mainnet(),
        )
        .expect_err("must reject");
        match err {
            MiningError::EmissionInvariant { op, reason } => {
                assert_eq!(op, "build_pre_eip27_emission_tx");
                assert!(
                    reason.contains("emission box value") && reason.contains("exhausted"),
                    "{reason}"
                );
            }
            other => panic!("expected EmissionInvariant, got {other:?}"),
        }
    }

    // ----- SigmaChain YOLO emission tx (Phase 4.4) -----

    /// Build a synthetic input emission box at the SigmaChain genesis
    /// state for use in the YOLO coinbase tests. Uses a real Ergo
    /// emission tree as a stand-in for the SigmaChain emission script
    /// — the builder doesn't validate the input's script, only
    /// preserves it on the output side.
    fn yolo_input_box_at(value: u64) -> ErgoBox {
        let parent = load(699_999);
        let real = ergo_box_from_output(&parent.emission_tx.outputs[0], &parent.emission_tx.id, 0);
        let candidate = ErgoBoxCandidate::from_trusted_raw_parts(
            value,
            real.candidate.ergo_tree().clone(),
            real.candidate.ergo_tree_bytes().to_vec(),
            real.candidate.creation_height,
            real.candidate.tokens.clone(),
            AdditionalRegisters::empty(),
            vec![0x00],
        );
        ErgoBox {
            candidate,
            transaction_id: real.transaction_id,
            index: 0,
        }
    }

    fn yolo_treasury_script() -> Vec<u8> {
        ergo_chain_spec::yolo_genesis_scripts::treasury_accumulation_ergo_tree_bytes()
    }

    fn yolo_lp_script() -> Vec<u8> {
        ergo_chain_spec::yolo_genesis_scripts::lp_accumulation_ergo_tree_bytes()
    }

    #[test]
    fn yolo_emission_tx_has_four_outputs_at_genesis() {
        // h = 0 → block reward 50 YOLO, split 42.5 / 5 / 2.5.
        let yolo = YoloEmissionParams::sigmachain_testnet();
        let value = 176_525_325_000_000_000u64;
        let input_box = yolo_input_box_at(value);
        // For the smoke test we use the actual treasury / LP bytes
        // from the chain-spec module so the builder exercises the same
        // parse path that production will hit.
        let treasury_bytes = ergo_chain_spec::yolo_genesis_scripts::treasury_accumulation_ergo_tree_bytes();
        let lp_bytes = ergo_chain_spec::yolo_genesis_scripts::lp_accumulation_ergo_tree_bytes();
        let tx = build_yolo_emission_tx(
            &input_box,
            &[0x02u8; 33],
            1,
            &yolo,
            &treasury_bytes,
            &lp_bytes,
        )
        .expect("ok");
        assert_eq!(tx.inputs.len(), 1, "one input (the emission box)");
        assert_eq!(tx.output_candidates.len(), 4, "four outputs");
        // Output values match the YoloEmissionParams split for h=1
        // (still in the first halving epoch).
        let new_emission = &tx.output_candidates[0];
        let treasury = &tx.output_candidates[1];
        let lp = &tx.output_candidates[2];
        let miner = &tx.output_candidates[3];
        assert_eq!(new_emission.value, value - 50_000_000_000);
        assert_eq!(treasury.value, 5_000_000_000);
        assert_eq!(lp.value, 2_500_000_000);
        assert_eq!(miner.value, 42_500_000_000);
        // Conservation: every nanoYOLO from the input is accounted for.
        assert_eq!(
            new_emission.value + treasury.value + lp.value + miner.value,
            value,
        );
    }

    #[test]
    fn yolo_emission_tx_preserves_input_emission_box_identity() {
        // Per emission.es: the new emission box (OUTPUTS(0)) must keep
        // the same script, the same tokens, and the same R4/R5
        // registers as the input. Confirm the builder preserves all
        // three.
        let yolo = YoloEmissionParams::sigmachain_testnet();
        let input_box = yolo_input_box_at(yolo.initial_reward * 1000);
        let tx = build_yolo_emission_tx(
            &input_box,
            &[0x02u8; 33],
            10,
            &yolo,
            &yolo_treasury_script(),
            &yolo_lp_script(),
        )
        .expect("ok");
        let new_emission = &tx.output_candidates[0];
        assert_eq!(
            new_emission.ergo_tree_bytes(),
            input_box.candidate.ergo_tree_bytes(),
        );
        assert_eq!(new_emission.tokens, input_box.candidate.tokens);
        assert_eq!(
            new_emission.additional_registers,
            input_box.candidate.additional_registers,
        );
        assert_eq!(new_emission.creation_height, 10);
    }

    #[test]
    fn yolo_emission_tx_split_holds_at_tail_emission() {
        // h = 5 * blocks_per_halving → tail rate (1 YOLO/block).
        // Treasury = 100M nano, LP = 50M nano, miner = 850M nano,
        // sum = 1e9 = MIN_REWARD.
        let yolo = YoloEmissionParams::sigmachain_testnet();
        let value = 10_000_000_000u64;
        let input_box = yolo_input_box_at(value);
        let h = 5 * yolo.blocks_per_halving + 17; // anywhere past the floor
        let tx = build_yolo_emission_tx(
            &input_box,
            &[0x02u8; 33],
            h,
            &yolo,
            &yolo_treasury_script(),
            &yolo_lp_script(),
        )
        .expect("ok");
        assert_eq!(tx.output_candidates[0].value, value - 1_000_000_000);
        assert_eq!(tx.output_candidates[1].value, 100_000_000);
        assert_eq!(tx.output_candidates[2].value, 50_000_000);
        assert_eq!(tx.output_candidates[3].value, 850_000_000);
    }

    #[test]
    fn yolo_emission_tx_rejects_exhausted_box() {
        let yolo = YoloEmissionParams::sigmachain_testnet();
        let input_box = yolo_input_box_at(1);
        let err = build_yolo_emission_tx(
            &input_box,
            &[0x02u8; 33],
            1,
            &yolo,
            &yolo_treasury_script(),
            &yolo_lp_script(),
        )
        .expect_err("must reject");
        match err {
            MiningError::EmissionInvariant { op, reason } => {
                assert_eq!(op, "build_yolo_emission_tx");
                assert!(reason.contains("block reward"), "{reason}");
            }
            other => panic!("expected EmissionInvariant, got {other:?}"),
        }
    }
}
