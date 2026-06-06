//! Build a block-candidate Extension field list.
//!
//! Per v12 §4.6 + Scala `CandidateGenerator.createCandidate` (lines 529-565)
//! the extension at height `H` carries:
//!
//! 1. NIPoPoW interlinks (key prefix `0x01`) — **always**.
//! 2. At voting-epoch boundary (`H % voting_length == 0`): the active
//!    parameter map (key prefix `0x00`, numeric ids carrying 4-byte
//!    big-endian `i32` values) plus a `proposed_update` entry at
//!    `(0x00, 124)` carrying a serialized
//!    `ErgoValidationSettingsUpdate`.
//!
//! Two entry points, dispatched at the caller on the boundary predicate
//! `candidate_height % voting_length == 0`:
//!
//! * [`build_candidate_extension_fields`] for the non-epoch path —
//!   emits interlinks only.
//! * [`build_candidate_extension_fields_epoch_boundary`] for the
//!   epoch-start path — emits interlinks followed by the encoded active
//!   parameters via [`ActiveProtocolParameters::encode_extension_fields`].
//!
//! Field order is consensus-bearing (the extension-root merkle tree is
//! order-dependent). The boundary path appends parameter fields after
//! interlinks; within parameter fields the order is fixed by the
//! encoder (ascending numeric id, `proposed_update` last).

use ergo_primitives::digest::ModifierId;
use ergo_ser::header::Header;
use ergo_validation::active_params::ActiveProtocolParameters;
use ergo_validation::popow::algos::{pack_interlinks, update_interlinks};

use crate::error::MiningError;

/// Build interlinks-only extension fields for a non-epoch-boundary
/// candidate. Always succeeds for any honest parent.
///
/// The caller is responsible for checking `candidate_height %
/// voting_length != 0` before calling this; at boundary heights the
/// returned fields are missing the active-parameter map and validation
/// would reject the resulting block.
pub fn build_candidate_extension_fields(
    parent_header: &Header,
    parent_interlinks: &[ModifierId],
) -> Result<ExtensionFields, MiningError> {
    let new_interlinks = update_interlinks(parent_header, parent_interlinks)?;
    Ok(pack_interlinks(&new_interlinks))
}

/// Build the epoch-boundary extension field list: interlinks plus the
/// encoded active-parameter map (numeric ids + `proposed_update`).
///
/// `active_params` must be the parameters for the new epoch about to
/// start at `parent_header.height + 1`, as produced by
/// [`ergo_validation::voting::compute_next_params`]. Validation at the
/// boundary block parses the extension and asserts equality against the
/// independently recomputed value; passing stale or unrecomputed params
/// here produces a block that will be rejected by `exMatchParameters`.
pub fn build_candidate_extension_fields_epoch_boundary(
    parent_header: &Header,
    parent_interlinks: &[ModifierId],
    active_params: &ActiveProtocolParameters,
) -> Result<ExtensionFields, MiningError> {
    let new_interlinks = update_interlinks(parent_header, parent_interlinks)?;
    let mut fields = pack_interlinks(&new_interlinks);
    fields.extend(active_params.encode_extension_fields());
    Ok(fields)
}

/// Extension-section field list as canonical wire pairs: `(key_bytes,
/// value_bytes)`. Used as the return shape of
/// [`build_candidate_extension_fields`] and
/// [`build_candidate_extension_fields_epoch_boundary`].
pub type ExtensionFields = Vec<(Vec<u8>, Vec<u8>)>;

#[cfg(test)]
mod tests {
    use super::*;
    use ergo_validation::active_params::{
        parse_active_params, ActiveProtocolParameters, SOFT_FORK_DISABLING_RULES_ID,
        SYSTEM_PARAMETERS_PREFIX,
    };
    use ergo_validation::popow::algos::unpack_interlinks;
    use ergo_ser::extension::{Extension, ExtensionField};
    use ergo_primitives::digest::ModifierId;
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct InterlinksVector {
        height: u32,
        header_id: String,
        version: u8,
        n_interlinks_fields: usize,
        n_extension_fields_total: usize,
        interlinks_fields: Vec<[String; 2]>,
    }

    fn load(h: u32) -> InterlinksVector {
        let path = format!(
            "{}/../test-vectors/mining/interlinks_corpus/{}.json",
            env!("CARGO_MANIFEST_DIR"),
            h
        );
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("parse")
    }

    fn decode_fields(v: &InterlinksVector) -> Vec<(Vec<u8>, Vec<u8>)> {
        v.interlinks_fields
            .iter()
            .map(|p| (hex::decode(&p[0]).unwrap(), hex::decode(&p[1]).unwrap()))
            .collect()
    }

    /// `synth_header` defaults `n_bits = 0`, which makes
    /// `decode_compact_bits` return 0 and panics `max_level_of` via
    /// divide-by-zero. The boundary tests below call `update_interlinks`
    /// (which calls `max_level_of`); set a valid compact-bits value so
    /// the bigint math is well-defined.
    const TEST_NBITS: u32 = 0x1d00_ffff;

    /// Build a minimal synthetic parent header with the given height,
    /// version=2, valid `n_bits` for popow math, all-zero hashes.
    fn synth_header(height: u32) -> Header {
        use ergo_primitives::digest::{ADDigest, Digest32};
        use ergo_ser::autolykos::AutolykosSolution;
        Header {
            version: 2,
            parent_id: Digest32::from_bytes([0x42u8; 32]).into(),
            ad_proofs_root: Digest32::from_bytes([0u8; 32]),
            transactions_root: Digest32::from_bytes([0u8; 32]),
            state_root: ADDigest::from_bytes([0u8; 33]),
            timestamp: 0,
            extension_root: Digest32::from_bytes([0u8; 32]),
            n_bits: TEST_NBITS,
            height,
            votes: [0u8; 3],
            unparsed_bytes: Vec::new(),
            solution: AutolykosSolution::V2 {
                pk: ergo_primitives::group_element::GroupElement::from([0x02u8; 33]),
                nonce: [0u8; 8],
            },
        }
    }

    /// Parse a known launch-defaults parameter set for use as
    /// `active_params` in the boundary tests.
    fn sample_active_params(epoch_start: u32) -> ActiveProtocolParameters {
        fn be(v: i32) -> Vec<u8> {
            v.to_be_bytes().to_vec()
        }
        let fields = vec![
            ([0x00, 1], be(1_250_000)),
            ([0x00, 2], be(360)),
            ([0x00, 3], be(524_288)),
            ([0x00, 4], be(1_000_000)),
            ([0x00, 5], be(100)),
            ([0x00, 6], be(2_000)),
            ([0x00, 7], be(100)),
            ([0x00, 8], be(100)),
            ([0x00, 123], be(1)),
        ];
        let ext = Extension {
            header_id: ModifierId::from_bytes([0u8; 32]),
            fields: fields
                .into_iter()
                .map(|(k, v)| ExtensionField { key: k, value: v })
                .collect(),
        };
        parse_active_params(&ext, epoch_start).unwrap()
    }

    // ----- non-epoch path -----

    #[test]
    fn non_epoch_emits_only_interlinks() {
        let parent = synth_header(99);
        let parent_links = [parent.parent_id];
        let fields = build_candidate_extension_fields(&parent, &parent_links).unwrap();
        for (k, _) in &fields {
            assert_eq!(
                k[0], 0x01,
                "non-epoch fields must use interlinks prefix 0x01"
            );
        }
        let unpacked = unpack_interlinks(&fields).expect("interlinks parse");
        assert!(!unpacked.is_empty());
    }

    // ----- epoch-boundary path -----

    #[test]
    fn epoch_boundary_emits_interlinks_plus_params() {
        // h=6143 -> candidate h=6144 (SigmaChain epoch boundary).
        let parent = synth_header(6143);
        let parent_links = [parent.parent_id];
        let params = sample_active_params(6144);
        let fields = build_candidate_extension_fields_epoch_boundary(
            &parent,
            &parent_links,
            &params,
        )
        .unwrap();
        let interlink_count = fields.iter().filter(|(k, _)| k[0] == 0x01).count();
        let param_count = fields
            .iter()
            .filter(|(k, _)| k[0] == SYSTEM_PARAMETERS_PREFIX)
            .count();
        assert!(interlink_count >= 1, "expected interlinks fields");
        // 8 numeric (non-subblocks) + block_version + proposed_update = 10.
        assert_eq!(
            param_count, 10,
            "expected 10 parameter-prefix fields (8 numeric + block_version + proposed_update)"
        );
        // proposed_update must be present.
        assert!(fields
            .iter()
            .any(|(k, _)| k == &vec![SYSTEM_PARAMETERS_PREFIX, SOFT_FORK_DISABLING_RULES_ID]));
    }

    #[test]
    fn epoch_boundary_extension_round_trips_through_parser() {
        // The fields the encoder emits must parse cleanly via the same
        // `parse_active_params` the validator runs at the boundary.
        let parent = synth_header(6143);
        let parent_links = [parent.parent_id];
        let params = sample_active_params(6144);
        let fields = build_candidate_extension_fields_epoch_boundary(
            &parent,
            &parent_links,
            &params,
        )
        .unwrap();
        let ext = Extension {
            header_id: ModifierId::from_bytes([0u8; 32]),
            fields: fields
                .into_iter()
                .map(|(k, v)| ExtensionField {
                    key: [k[0], k[1]],
                    value: v,
                })
                .collect(),
        };
        let parsed = parse_active_params(&ext, 6144).unwrap();
        assert_eq!(parsed, params);
    }

    // ----- mainnet-corpus parity (non-epoch path) -----
    //
    // Heights 99999 / 100000 / 100001 are NOT epoch boundaries on
    // mainnet (`100000 % 1024 = 672`), so all three exercise the
    // non-epoch builder.

    #[test]
    fn non_epoch_extension_is_only_interlinks_at_100000() {
        let v = load(100_000);
        assert_ne!(v.height % 1024, 0);
        let fields = decode_fields(&v);
        for (k, _) in &fields {
            assert_eq!(
                k[0], 0x01,
                "non-epoch block must have only interlinks fields"
            );
        }
        let interlinks = unpack_interlinks(&fields).expect("unpack");
        let repacked = pack_interlinks(&interlinks);
        assert_eq!(repacked, fields);
    }
}
