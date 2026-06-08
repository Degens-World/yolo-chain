//! Phase 5.4.4 — Validation, quorum-fail path (counting.es Phase 3).
//!
//! Test #4 of the handoff's lifecycle list. Runs counting.es phase3
//! against a counter whose tallies (R7, R9) are both zero — the
//! quorum threshold is `quorumFloor = 1,000,000 vYOLO`, so `R7 = 0`
//! is the canonical failing case. The proposal stays at qty 1 because
//! counting.es only reads `INPUTS(1)` when `proposalPassed = true`;
//! on failure the contract treats `proposalAdvanced` as a no-op and
//! the proposal box doesn't have to appear in the tx at all.
//!
//! Tx structure:
//!   INPUTS(0) = counter box (current Phase 1, R7=R9=0)
//!   INPUTS(1) = plain wallet funding box (NOT the proposal box —
//!               proposal.es would reject any spending path that
//!               doesn't advance qty 1 → 2)
//!   OUTPUTS(0) = counter successor with:
//!                 R4 = FAR_FUTURE_HEIGHT (resets Phase-0 sentinel so
//!                     a fresh phase1 round can fire later)
//!                 R5 = (0L, 0L), R6 = 32 zero bytes,
//!                 R8 = 0L (no recorded stake)
//!                 R7 = 0L, R9 = 0L (REQUIRED by phase3's counterReset)
//!   OUTPUTS(1) = fee, OUTPUTS(2) = change (wallet-emitted)
//!
//! Preconditions:
//!   * proposal_initiation has run (or chain already past Phase 0
//!     with R7=R9=0)
//!   * HEIGHT is inside the validation window
//!     `[voteDeadline + countingPhase, voteDeadline + countingPhase + executionGrace)`
//!     — test-windows arithmetic: countingPhase=5, executionGrace=4320.
//!
//! Post-condition: counter looks like fresh Phase-0 again. Proposal
//! box untouched at qty 1. Re-running this test is a no-op (the
//! counter R7=R9=0 invariant still holds, the contract evaluates
//! identically, no side-effect of value).

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use yolo_governance_node_tests::client::{AssetDto, NodeClient, PaymentRequestDto};
use yolo_governance_node_tests::config::TestConfig;
use yolo_governance_node_tests::registers::{coll_byte_hex, slong_hex, slong_pair_hex};

/// Reset sentinel — same value setup_dao_genesis used so the counter
/// post-test looks indistinguishable from the fresh Phase-0 box.
const FAR_FUTURE_HEIGHT: i64 = 1_000_000_000;

/// Voting-layer constants (from PARAMETERS.md, test-windows variant).
const COUNTING_PHASE_TEST: i64 = 5;
const EXECUTION_GRACE: i64 = 4320;

/// Fee floor.
const SETUP_FEE: u64 = 1_000_000;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RegisterEntry {
    key: String,
    hex: String,
    description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct TokenRef {
    name: String,
    token_id: String,
    amount: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BoxRef {
    box_id: String,
    value: u64,
    p2s_address: String,
    tokens: Vec<TokenRef>,
    registers: Vec<RegisterEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct StakeRefBox {
    box_id: String,
    value: u64,
    vyolo_amount: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct DaoState {
    setup_tx_id: String,
    setup_height: u32,
    counter: BoxRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stake_reference: Option<StakeRefBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposal_box: Option<BoxRef>,
}

#[derive(Debug, Deserialize)]
struct DeployToken {
    name: String,
    token_id: String,
    #[allow(dead_code)]
    tx_id: String,
    #[allow(dead_code)]
    amount: u64,
}

#[derive(Debug, Deserialize)]
struct Deployment {
    #[allow(dead_code)]
    network: String,
    #[allow(dead_code)]
    final_height: u32,
    #[allow(dead_code)]
    owner_address: String,
    tokens: Vec<DeployToken>,
}

#[derive(Debug, Deserialize)]
struct CompiledTree {
    contract: String,
    instance: Option<u8>,
    p2s_address: String,
    tree_hex: String,
    #[allow(dead_code)]
    proposition_hash: String,
}

#[derive(Debug, Deserialize)]
struct CompiledDeployment {
    #[allow(dead_code)]
    network: String,
    #[allow(dead_code)]
    final_height: u32,
    #[allow(dead_code)]
    owner_address: String,
    #[allow(dead_code)]
    token_ids: BTreeMap<String, String>,
    trees: Vec<CompiledTree>,
}

#[test]
fn validation_keeps_proposal_at_qty_1_when_quorum_fails() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let client = NodeClient::new(cfg);

    let status = client.wallet_status().expect("GET /wallet/status");
    if !status.is_unlocked {
        client.wallet_unlock().expect("POST /wallet/unlock");
    }
    let status = client.wallet_status().expect("re-read /wallet/status");
    assert!(status.is_unlocked, "wallet must be unlocked");
    eprintln!("wallet ready. change_address={}", status.change_address);

    let deployment = load_deployment();
    let compiled_test = load_compiled_test();
    let token_by_name: BTreeMap<String, String> = deployment
        .tokens
        .into_iter()
        .map(|t| (t.name, t.token_id))
        .collect();

    let counter_tree = find_tree(&compiled_test, "counting", None);
    let counter_p2s = counter_tree.p2s_address.clone();
    let counter_tree_hex = counter_tree.tree_hex.to_ascii_lowercase();
    let counter_nft = token_by_name
        .get("COUNTER_NFT")
        .expect("missing COUNTER_NFT")
        .clone();
    let proposal_token = token_by_name
        .get("PROPOSAL_TOKEN")
        .expect("missing PROPOSAL_TOKEN")
        .clone();

    // ---- 1. Reconcile counter via COUNTER_NFT ----
    let live_counter = lookup_unspent_by_token(&client, &counter_nft)
        .expect("COUNTER_NFT not in any unspent box");
    let counter_box_id = live_counter
        .get("boxId")
        .and_then(|v| v.as_str())
        .expect("counter boxId")
        .to_string();
    let counter_value = live_counter
        .get("value")
        .and_then(|v| v.as_u64())
        .expect("counter value");
    let counter_ergotree = live_counter
        .get("ergoTree")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    assert_eq!(
        counter_ergotree, counter_tree_hex,
        "COUNTER_NFT holder is not a counting.es box"
    );

    // ---- 2. Verify chain is in validation window (or already reset) ----
    //
    // counting.es phase3 fires when
    //   countingEnd <= HEIGHT < countingEnd + executionGrace
    // where countingEnd = voteDeadline (counter R4) + countingPhase.
    //
    // If a previous run already executed phase3 and reset the counter,
    // R4 will be the FAR_FUTURE_HEIGHT sentinel — countingEnd is then
    // billions of blocks away, so the window check trivially fails.
    // Treat that as success-already and just verify the proposal box.
    let counter_r4_decoded = decode_slong_register(&live_counter, "R4").expect("decode R4");
    let counter_r7_decoded = decode_slong_register(&live_counter, "R7").expect("decode R7");
    let counter_r9_decoded = decode_slong_register(&live_counter, "R9").expect("decode R9");
    if counter_r4_decoded == FAR_FUTURE_HEIGHT && counter_r7_decoded == 0 && counter_r9_decoded == 0
    {
        eprintln!(
            "counter already at Phase-0 sentinel (R4={FAR_FUTURE_HEIGHT}, R7=R9=0) — \
             previous phase3-fail tx already landed. Verifying proposal still at qty 1."
        );
        verify_proposal_still_at_qty_1(&client, &compiled_test, &proposal_token);
        return;
    }
    let counting_end = counter_r4_decoded + COUNTING_PHASE_TEST;
    let validation_end = counting_end + EXECUTION_GRACE;
    let height: i64 = client.full_height().expect("full_height").into();
    assert!(
        height >= counting_end && height < validation_end,
        "chain at h={height} is NOT in validation window [{counting_end}, {validation_end}) — \
         counter R4={counter_r4_decoded}, countingPhase={COUNTING_PHASE_TEST}. \
         Either we already validated (post-{validation_end}) or counting window hasn't closed yet."
    );
    assert_eq!(
        counter_r7_decoded, 0,
        "counter R7 (totalVotes) must be 0 for the quorum-fail path"
    );
    assert_eq!(
        counter_r9_decoded, 0,
        "counter R9 (validationVotes) must be 0 for the quorum-fail path"
    );
    eprintln!(
        "phase3 quorum-fail prereqs ok: h={height} ∈ [{counting_end}, {validation_end}), \
         R7={counter_r7_decoded}, R9={counter_r9_decoded}"
    );

    // ---- 3. Find plain funding box (used as INPUTS(1), only carries
    //         ERG for fees + change; phase3 ignores it on quorum-fail) ----
    let funding = pick_plain_funding_box(&client, SETUP_FEE * 4, &counter_box_id)
        .expect("no plain funding box");
    eprintln!(
        "funding box {} value={}",
        &funding.box_id[..16],
        funding.value
    );

    // ---- 4. Counter successor registers (post-validation reset) ----
    //
    // phase3 requires R7=R9=0 on the output. The other slots are
    // unconstrained; we set them to the same Phase-0 sentinel
    // setup_dao_genesis used, so the post-test counter is byte-for-byte
    // identical to a fresh setup.
    let counter_r4 = slong_hex(FAR_FUTURE_HEIGHT);
    let counter_r5 = slong_pair_hex(0, 0);
    let counter_r6 = coll_byte_hex(&[0u8; 32]);
    let counter_r7 = slong_hex(0);
    let counter_r8 = slong_hex(0);
    let counter_r9 = slong_hex(0);
    let counter_registers = vec![
        RegisterEntry {
            key: "R4".to_string(),
            hex: counter_r4.clone(),
            description: format!("vote deadline reset = {FAR_FUTURE_HEIGHT}"),
        },
        RegisterEntry {
            key: "R5".to_string(),
            hex: counter_r5.clone(),
            description: "(proportion, votes_for) reset = (0L, 0L)".to_string(),
        },
        RegisterEntry {
            key: "R6".to_string(),
            hex: counter_r6.clone(),
            description: "recipient hash reset = 32 zero bytes".to_string(),
        },
        RegisterEntry {
            key: "R7".to_string(),
            hex: counter_r7.clone(),
            description: "total votes = 0 (REQUIRED by counterReset)".to_string(),
        },
        RegisterEntry {
            key: "R8".to_string(),
            hex: counter_r8.clone(),
            description: "initiation stake reset = 0L".to_string(),
        },
        RegisterEntry {
            key: "R9".to_string(),
            hex: counter_r9.clone(),
            description: "validation votes = 0 (REQUIRED by counterReset)".to_string(),
        },
    ];
    let counter_request = PaymentRequestDto {
        address: counter_p2s.clone(),
        value: counter_value, // value preserved (out0.value >= self.value)
        assets: vec![AssetDto {
            token_id: counter_nft.clone(),
            amount: 1,
        }],
        additional_registers: Some(
            counter_registers
                .iter()
                .map(|r| (r.key.clone(), r.hex.clone()))
                .collect(),
        ),
    };

    // ---- 5. Submit ----
    //
    // Even though we're targeting phase3, the ErgoScript reducer
    // eagerly evaluates phase1's `val initiationBox = CONTEXT.dataInputs(0)`
    // when reducing `sigmaProp(phase1 || phase2 || phase3 || phase4)`.
    // An empty dataInputs list produces a `valid box index` TypeError
    // at reduce time, regardless of which phase predicate ends up
    // satisfied. Pass the stake reference box (from dao-state) as
    // dataInputs(0) so the read succeeds — phase3's boolean wins on
    // the merits because `isBeforeCounting = HEIGHT < voteDeadline`
    // is false (we're well past the deadline).
    let stake_ref = state_stake_reference();
    let inputs = vec![counter_box_id.clone(), funding.box_id.clone()];
    let data_inputs = vec![stake_ref];
    let tx_id = client
        .wallet_transaction_send_full(&[counter_request], &inputs, &data_inputs, Some(SETUP_FEE))
        .expect("POST /wallet/transaction/send (phase3 quorum-fail)");
    eprintln!("submitted phase3 quorum-fail tx_id={tx_id}");
    let tx_body = client.wait_for_tx(&tx_id).expect("phase3 tx inclusion");
    let inclusion_height = tx_body
        .get("inclusionHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("phase3 tx outputs");

    // ---- 6. Verify counter advanced + registers round-trip ----
    let counter_succ = find_output_with_token(outputs, &counter_nft)
        .unwrap_or_else(|| panic!("no output carrying COUNTER_NFT"));
    let counter_succ_utxo = client
        .utxo_by_id(&counter_succ.box_id)
        .expect("utxo lookup counter successor")
        .unwrap_or_else(|| panic!("counter successor {} not in UTXO", counter_succ.box_id));
    cross_check_registers(&counter_succ_utxo, &counter_registers, "counter successor");
    eprintln!(
        "counter reset: box={} at h={inclusion_height}",
        &counter_succ.box_id[..16]
    );

    // ---- 7. Verify proposal box still at qty 1 + untouched ----
    //
    // `/blockchain/box/unspent/byTokenId` returns ALL boxes carrying
    // PROPOSAL_TOKEN — including the wallet's stash (qty 9999) which
    // is the bake's residual. The proposal contract box is the one
    // at qty 1 under the proposal.es ergoTree. Filter explicitly.
    let proposal_tree_hex = find_tree(&compiled_test, "proposal", None)
        .tree_hex
        .to_ascii_lowercase();
    let proposal_contract_box = lookup_unspent_boxes_by_token(&client, &proposal_token)
        .into_iter()
        .find(|b| {
            let tree = b
                .get("ergoTree")
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            tree == proposal_tree_hex
        })
        .expect("proposal contract box (qty 1 under proposal.es) must still exist");
    let proposal_box_id = proposal_contract_box
        .get("boxId")
        .and_then(|v| v.as_str())
        .expect("proposal boxId")
        .to_string();
    let proposal_qty = proposal_contract_box
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|a| {
            a.iter().find_map(|asset| {
                (asset.get("tokenId")?.as_str()? == proposal_token)
                    .then(|| asset.get("amount")?.as_u64())
            })
        })
        .flatten()
        .expect("proposal qty");
    assert_eq!(
        proposal_qty, 1,
        "proposal token under proposal.es must stay at qty 1 after quorum-fail validation"
    );
    eprintln!(
        "proposal preserved: box={} qty=1 (unchanged)",
        &proposal_box_id[..16]
    );

    // ---- 8. Persist updated state ----
    let mut state = load_dao_state();
    state.setup_tx_id = tx_id;
    state.setup_height = inclusion_height;
    state.counter = BoxRef {
        box_id: counter_succ.box_id,
        value: counter_succ.value,
        p2s_address: counter_p2s,
        tokens: vec![TokenRef {
            name: "COUNTER_NFT".to_string(),
            token_id: counter_nft,
            amount: 1,
        }],
        registers: counter_registers,
    };
    // proposal_box and stake_reference remain valid as-is — both
    // untouched on chain.
    write_dao_state(&state);
    eprintln!(
        "wrote {} (counter reset to Phase-0 sentinel; proposal untouched)",
        dao_state_path().display()
    );
}

/// Shared post-condition check used by both the active phase3 path
/// and the idempotent adoption path: the proposal contract box is
/// still on chain at qty 1 under proposal.es.
fn verify_proposal_still_at_qty_1(
    client: &NodeClient,
    compiled_test: &CompiledDeployment,
    proposal_token: &str,
) {
    let proposal_tree_hex = find_tree(compiled_test, "proposal", None)
        .tree_hex
        .to_ascii_lowercase();
    let proposal_contract_box = lookup_unspent_boxes_by_token(client, proposal_token)
        .into_iter()
        .find(|b| {
            let tree = b
                .get("ergoTree")
                .and_then(|v| v.as_str())
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_default();
            tree == proposal_tree_hex
        })
        .expect("proposal contract box (qty 1 under proposal.es) must still exist");
    let proposal_box_id = proposal_contract_box
        .get("boxId")
        .and_then(|v| v.as_str())
        .expect("proposal boxId")
        .to_string();
    let proposal_qty = proposal_contract_box
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|a| {
            a.iter().find_map(|asset| {
                (asset.get("tokenId")?.as_str()? == proposal_token)
                    .then(|| asset.get("amount")?.as_u64())
            })
        })
        .flatten()
        .expect("proposal qty");
    assert_eq!(
        proposal_qty, 1,
        "proposal token under proposal.es must stay at qty 1 after quorum-fail validation"
    );
    eprintln!(
        "proposal preserved: box={} qty=1",
        &proposal_box_id[..16]
    );
}

// ---- helpers ----

#[derive(Debug)]
struct OutputBox {
    box_id: String,
    value: u64,
}

fn find_output_with_token(outputs: &[serde_json::Value], token_id: &str) -> Option<OutputBox> {
    outputs.iter().find_map(|o| {
        let tokens = o.get("assets")?.as_array()?;
        let has = tokens.iter().any(|a| {
            a.get("tokenId")
                .and_then(|t| t.as_str())
                .map(|s| s == token_id)
                .unwrap_or(false)
        });
        if !has {
            return None;
        }
        let box_id = o.get("boxId")?.as_str()?.to_string();
        let value = o.get("value")?.as_u64()?;
        Some(OutputBox { box_id, value })
    })
}

fn lookup_unspent_by_token(client: &NodeClient, token_id: &str) -> Option<serde_json::Value> {
    let path = format!("/blockchain/box/unspent/byTokenId/{}", token_id);
    let resp = client
        .raw_get_json_auth(&path)
        .expect("/blockchain/box/unspent/byTokenId");
    resp.as_array()?.first().cloned()
}

/// Like `lookup_unspent_by_token`, but returns ALL unspent boxes
/// carrying the given token. Used here to distinguish the proposal
/// contract box (qty 1 under proposal.es) from the wallet's stash
/// (qty 9999 under a plain P2PK) — both carry PROPOSAL_TOKEN but
/// only the former is the in-flight proposal.
fn lookup_unspent_boxes_by_token(
    client: &NodeClient,
    token_id: &str,
) -> Vec<serde_json::Value> {
    let path = format!("/blockchain/box/unspent/byTokenId/{}", token_id);
    let resp = client
        .raw_get_json_auth(&path)
        .expect("/blockchain/box/unspent/byTokenId");
    resp.as_array().cloned().unwrap_or_default()
}

fn cross_check_registers(utxo: &serde_json::Value, expected: &[RegisterEntry], label: &str) {
    let regs = utxo
        .get("additionalRegisters")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("{label}: missing additionalRegisters on UTXO"));
    for r in expected {
        let actual = regs
            .get(&r.key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{label}: missing {} on UTXO", r.key));
        assert_eq!(
            actual.to_ascii_lowercase(),
            r.hex.to_ascii_lowercase(),
            "{label} {}: sent {}, indexer reports {actual}",
            r.key,
            r.hex
        );
    }
}

#[derive(Debug)]
struct FundingBox {
    box_id: String,
    value: u64,
}

fn pick_plain_funding_box(
    client: &NodeClient,
    min_value: u64,
    exclude: &str,
) -> Option<FundingBox> {
    const PAGE_SIZE: u32 = 500;
    let mut offset: u32 = 0;
    loop {
        let page = client
            .wallet_boxes_unspent(offset, PAGE_SIZE)
            .expect("/wallet/boxes/unspent");
        if page.items.is_empty() {
            return None;
        }
        for b in &page.items {
            if b.box_id == exclude {
                continue;
            }
            if b.value < min_value {
                continue;
            }
            let Some(utxo) = client.utxo_by_id(&b.box_id).expect("utxo lookup") else {
                continue;
            };
            let has_tokens = utxo
                .get("assets")
                .and_then(|a| a.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);
            if has_tokens {
                continue;
            }
            return Some(FundingBox {
                box_id: b.box_id.clone(),
                value: b.value,
            });
        }
        offset = offset.checked_add(PAGE_SIZE)?;
    }
}

/// Decode an `SLong` register's JSON-hex into the raw `i64` it
/// represents. Mirrors `ergo_ser::register::read_register_value` for
/// the constant `SLong` form (`type=0x05`, then ZigZag-VLQ). Returns
/// `None` if the register is missing or not an `SLong` constant.
fn decode_slong_register(box_json: &serde_json::Value, key: &str) -> Option<i64> {
    let hex_str = box_json
        .get("additionalRegisters")?
        .get(key)?
        .as_str()?;
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.first().copied() != Some(0x05) {
        return None;
    }
    let mut cursor = 1usize;
    let mut value: u64 = 0;
    let mut shift = 0u32;
    loop {
        if cursor >= bytes.len() {
            return None;
        }
        let b = bytes[cursor];
        cursor += 1;
        value |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 56 {
            return None;
        }
    }
    // ZigZag decode.
    Some((value >> 1) as i64 ^ -((value & 1) as i64))
}

fn find_tree<'a>(
    compiled: &'a CompiledDeployment,
    contract: &str,
    instance: Option<u8>,
) -> &'a CompiledTree {
    compiled
        .trees
        .iter()
        .find(|t| t.contract == contract && t.instance == instance)
        .unwrap_or_else(|| panic!("no compiled tree for {} instance {:?}", contract, instance))
}

fn load_deployment() -> Deployment {
    let p = test_vectors_dir().join("sigmachain-deployment.json");
    let bytes = std::fs::read(&p).expect("read deployment.json");
    serde_json::from_slice(&bytes).expect("parse deployment.json")
}

fn load_compiled_test() -> CompiledDeployment {
    let p = test_vectors_dir().join("sigmachain-deployment-trees-test.json");
    let bytes = std::fs::read(&p).expect("read deployment-trees-test.json");
    serde_json::from_slice(&bytes).expect("parse deployment-trees-test.json")
}

fn load_dao_state() -> DaoState {
    let p = dao_state_path();
    let bytes = std::fs::read(&p).expect("read dao-state.json");
    serde_json::from_slice(&bytes).expect("parse dao-state.json")
}

fn write_dao_state(state: &DaoState) {
    let json = serde_json::to_string_pretty(state).expect("serialize dao-state");
    std::fs::write(dao_state_path(), json).expect("write dao-state.json");
}

fn test_vectors_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("test-vectors");
    p
}

fn dao_state_path() -> PathBuf {
    test_vectors_dir().join("sigmachain-dao-state.json")
}

/// Read the stake reference box id from dao-state.json. Set during
/// proposal_initiation; required here so the phase1 predicate's
/// `CONTEXT.dataInputs(0)` evaluates to a valid box during reduction.
fn state_stake_reference() -> String {
    let state = load_dao_state();
    state
        .stake_reference
        .map(|s| s.box_id)
        .expect(
            "dao-state.json has no stake_reference — \
             run proposal_initiation first (it bootstraps the stake box)",
        )
}
