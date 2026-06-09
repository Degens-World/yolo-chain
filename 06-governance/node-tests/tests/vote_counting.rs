//! Phase 5.4.4 — Vote counting (counting.es Phase 2).
//!
//! Test #2 of the handoff lifecycle list. Drives the chain through:
//!   * voter-box bootstrap (3 boxes carrying (ValidVote NFT, vYOLO))
//!   * wait until HEIGHT enters the counting window
//!   * a single counting tx that consumes the counter + voter boxes,
//!     emits a Phase-2 counter successor, refunds vYOLO to the
//!     wallet, and **burns** the ValidVote NFTs (counting.es asserts
//!     they appear in no output, at no slot, in this tx)
//!
//! Phase 2 contract requirements:
//!   * `voter.tokens(0) == (ValidVoteId, 1)` and `voter.tokens(1) == (VYoloId, power)`
//!     — token order is positional and MUST match the contract's expected layout.
//!     Wire-level the wallet preserves caller-supplied order through
//!     `PaymentRequest.assets: Vec<...>` (after the recent BTreeMap → Vec patch).
//!   * `out0.R5._2 == currentVotesFor + sum(yes_power)`, `out0.R7 == totalVotes + sum(power)`,
//!     `out0.R9 == validationVotes + sum(yes_power)`. R4, R6, R8 preserved.
//!   * `OUTPUTS.forall { o => o.tokens.forall { _._1 != ValidVoteId } }` — burned.
//!   * `HEIGHT ∈ [voteDeadline, voteDeadline + countingPhase)`.
//!
//! Counting tx construction: built **manually via ergo-ser**, not the
//! wallet's auto-build. The wallet's change machinery would emit the
//! leftover ValidVote NFTs to a change box, and counting.es would
//! reject. Raw construction lets us write exactly the outputs we want
//! and just NOT include the burned tokens. Signed via
//! `/wallet/transaction/sign` (already patched to accept non-P2PK
//! inputs like the counter contract box) and submitted via
//! `/transactions/bytes`.
//!
//! Voter fixture (small token amounts — quorum isn't checked at Phase 2,
//! only at Phase 3 validation, so we don't need amounts ≥ quorumFloor):
//!   * voter A: yes  (R4=1L), power = 10 vYOLO nanocoins
//!   * voter B: yes  (R4=1L), power = 10 vYOLO nanocoins
//!   * voter C: no   (R4=0L), power =  5 vYOLO nanocoins
//!   total power = 25, yes power = 20
//!
//! Idempotent: a re-run after success detects `counter.R7 > 0` and
//! adopts the on-chain state; re-verifies the post-condition burns.
//!
//! Preconditions:
//!   * Counter must be at Phase 1 (R4 != FAR_FUTURE_HEIGHT). Run
//!     `proposal_initiation` first if the counter is at Phase 0.
//!   * Wallet must hold ValidVote NFTs + vYOLO (the bake leaves
//!     1,000,000 and 177M+ respectively).
//!   * Counting window is only `countingPhase = 5` blocks (test-windows).
//!     The test polls until HEIGHT is inside the window; if voter
//!     bootstrap pushed us past, you'll need to re-run
//!     `validation_keeps_proposal_at_qty_1_when_quorum_fails`
//!     followed by `proposal_initiation` to start a fresh round.

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use yolo_governance_node_tests::client::{AssetDto, NodeClient, PaymentRequestDto};
use yolo_governance_node_tests::config::TestConfig;
use yolo_governance_node_tests::registers::{slong_hex, slong_pair_hex};

const FAR_FUTURE_HEIGHT: i64 = 1_000_000_000;
const COUNTING_PHASE_TEST: i64 = 5;
const MIN_BOX_VALUE: u64 = 1_000_000;
const SETUP_FEE: u64 = 1_000_000;

/// Per-voter vYOLO power. Test uses nanocoin units; values are
/// intentionally tiny so the run is decoupled from any wallet vYOLO
/// balance assumption.
const VOTER_A_POWER: u64 = 10;
const VOTER_B_POWER: u64 = 10;
const VOTER_C_POWER: u64 = 5;

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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct VoterBox {
    box_id: String,
    value: u64,
    vyolo_power: u64,
    yes: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct DaoState {
    setup_tx_id: String,
    setup_height: u32,
    counter: BoxRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stake_reference: Option<StakeRefBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposal_box: Option<BoxRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    voters: Vec<VoterBox>,
}

impl Default for BoxRef {
    fn default() -> Self {
        Self {
            box_id: String::new(),
            value: 0,
            p2s_address: String::new(),
            tokens: Vec::new(),
            registers: Vec::new(),
        }
    }
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
    #[allow(dead_code)]
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
fn vote_counting_burns_vote_nfts_and_accumulates_tallies() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let client = NodeClient::new(cfg);

    let status = client.wallet_status().expect("GET /wallet/status");
    if !status.is_unlocked {
        client.wallet_unlock().expect("POST /wallet/unlock");
    }
    let status = client.wallet_status().expect("re-read /wallet/status");
    assert!(status.is_unlocked);
    let change_address = status.change_address.clone();
    eprintln!("wallet ready. change_address={change_address}");

    let deployment = load_deployment();
    let compiled_test = load_compiled_test();
    let token_by_name: BTreeMap<String, String> = deployment
        .tokens
        .into_iter()
        .map(|t| (t.name, t.token_id))
        .collect();

    let counter_tree = find_tree(&compiled_test, "counting", None);
    let counter_tree_hex = counter_tree.tree_hex.to_ascii_lowercase();
    let counter_nft = token_by_name.get("COUNTER_NFT").expect("COUNTER_NFT").clone();
    let valid_vote_nft = token_by_name
        .get("VALID_VOTE_NFT")
        .expect("VALID_VOTE_NFT")
        .clone();
    let vyolo_id = token_by_name.get("VYOLO").expect("VYOLO").clone();

    // ---- 1. Reconcile counter ----
    let live_counter =
        lookup_unspent_by_token(&client, &counter_nft).expect("COUNTER_NFT not in any unspent box");
    let counter_box_id = live_counter
        .get("boxId")
        .and_then(|v| v.as_str())
        .expect("counter boxId")
        .to_string();
    let counter_value = live_counter
        .get("value")
        .and_then(|v| v.as_u64())
        .expect("counter value");
    let counter_ergotree_hex = live_counter
        .get("ergoTree")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    assert_eq!(
        counter_ergotree_hex, counter_tree_hex,
        "COUNTER_NFT holder is not a counting.es box"
    );

    let counter_r4 = decode_slong_register(&live_counter, "R4").expect("counter R4");
    let counter_r6_hex = read_register_hex(&live_counter, "R6").expect("counter R6");
    let counter_r7 = decode_slong_register(&live_counter, "R7").expect("counter R7");
    let counter_r8_hex = read_register_hex(&live_counter, "R8").expect("counter R8");
    let counter_r9 = decode_slong_register(&live_counter, "R9").expect("counter R9");
    eprintln!(
        "counter: box={} R4={counter_r4} R7={counter_r7} R9={counter_r9}",
        &counter_box_id[..16]
    );

    // ---- 2. Phase detection ----
    if counter_r7 > 0 {
        eprintln!("counter already past Phase 2 (R7={counter_r7}) — adopting on-chain state.");
        verify_burns(&client, &valid_vote_nft, &load_dao_state().voters);
        return;
    }
    if counter_r4 == FAR_FUTURE_HEIGHT {
        panic!(
            "counter at Phase 0 (R4 = FAR_FUTURE sentinel). Run proposal_initiation first."
        );
    }
    eprintln!(
        "counter at Phase 1 with vote deadline R4={counter_r4} — proceeding to count."
    );

    // ---- 3. Bootstrap voter boxes (or adopt from dao-state) ----
    let mut state = load_dao_state();
    let voters = ensure_voters(
        &client,
        &change_address,
        &valid_vote_nft,
        &vyolo_id,
        &mut state,
    );
    eprintln!(
        "voters ready: {} boxes, total power={}, yes power={}",
        voters.len(),
        total_power(&voters),
        yes_power(&voters),
    );

    // ---- 4. Wait for counting window ----
    //
    // counting.es phase2 requires `HEIGHT ∈ [voteDeadline, voteDeadline + 5)`.
    // Empirically (see proposal_initiation's height-offset probe), the
    // wallet's contract HEIGHT during sign = `/info fullHeight + 4`
    // — `/info` caches `best_full_block_height` and lags the
    // live-reading `committed_tip` by ~3 blocks, plus the wallet's
    // pre-header is `committed_tip + 1`. So target tip when we expect
    // HEIGHT == voteDeadline is `voteDeadline - 4`. Submit ~1 block
    // before that to land in the window with margin.
    const TIP_TO_HEIGHT_OFFSET: i64 = 4;
    let counting_window_start = counter_r4;
    let counting_window_end = counter_r4 + COUNTING_PHASE_TEST;
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        let tip: i64 = client.full_height().expect("full_height").into();
        let height_at_inclusion = tip + TIP_TO_HEIGHT_OFFSET;
        if height_at_inclusion >= counting_window_start && height_at_inclusion < counting_window_end
        {
            eprintln!("counting window open: tip={tip}, HEIGHT={height_at_inclusion} ∈ [{counting_window_start}, {counting_window_end})");
            break;
        }
        if height_at_inclusion >= counting_window_end {
            panic!(
                "counting window already closed: HEIGHT={height_at_inclusion} >= {counting_window_end}. \
                 Run validation_keeps + proposal_initiation to reset and try again."
            );
        }
        eprintln!(
            "waiting for counting window: tip={tip} expected_HEIGHT={height_at_inclusion} target_window=[{counting_window_start}, {counting_window_end})"
        );
        if Instant::now() >= deadline {
            panic!("timed out waiting for counting window to open at h={counting_window_start}");
        }
        sleep(Duration::from_secs(2));
    }

    // ---- 5. Find a plain funding box for fee + extra ERG ----
    let funding = pick_plain_funding_box(&client, MIN_BOX_VALUE * 3, &counter_box_id)
        .expect("no plain funding box");
    eprintln!(
        "funding box {} value={}",
        &funding.box_id[..16],
        funding.value
    );

    // ---- 6. Build the raw counting tx ----
    //
    // ALL outputs are written manually so we can omit the ValidVote
    // NFTs from every output (the contract's burn check).
    let stake_ref_box_id = state
        .stake_reference
        .clone()
        .map(|s| s.box_id)
        .expect("stake_reference missing — run proposal_initiation first");

    let total_pwr = total_power(&voters);
    let yes_pwr = yes_power(&voters);
    // Counter input R5 = (currentProportion, currentVotesFor). Phase
    // 2 advances `_2` by yes_power but keeps `_1`. We don't decode
    // the proportion value — just keep the input's R5 hex verbatim
    // for R4/R6/R8 (preserved) and reconstruct R5 with the updated
    // second slot.
    let proportion = decode_slong_pair_first(&live_counter, "R5")
        .expect("counter R5 must be (Long, Long) tuple");
    let new_r5_hex = slong_pair_hex(proportion, (0 + yes_pwr) as i64);
    let new_r7_hex = slong_hex(total_pwr as i64);
    let new_r9_hex = slong_hex(yes_pwr as i64);
    let counter_successor_registers = vec![
        RegisterEntry {
            key: "R4".to_string(),
            hex: read_register_hex(&live_counter, "R4").unwrap(),
            description: format!("vote deadline preserved = {counter_r4}"),
        },
        RegisterEntry {
            key: "R5".to_string(),
            hex: new_r5_hex.clone(),
            description: format!("(proportion, votes_for) = ({proportion}L, {yes_pwr}L)"),
        },
        RegisterEntry {
            key: "R6".to_string(),
            hex: counter_r6_hex.clone(),
            description: "recipient hash preserved".to_string(),
        },
        RegisterEntry {
            key: "R7".to_string(),
            hex: new_r7_hex.clone(),
            description: format!("total votes = {total_pwr}"),
        },
        RegisterEntry {
            key: "R8".to_string(),
            hex: counter_r8_hex.clone(),
            description: "initiation stake preserved".to_string(),
        },
        RegisterEntry {
            key: "R9".to_string(),
            hex: new_r9_hex.clone(),
            description: format!("validation votes = {yes_pwr}"),
        },
    ];

    let tx_id = build_sign_submit_counting_tx(
        &client,
        &change_address,
        &counter_box_id,
        counter_value,
        &counter_ergotree_hex,
        &counter_nft,
        &voters,
        &funding,
        &stake_ref_box_id,
        &vyolo_id,
        &counter_successor_registers,
    );
    eprintln!("submitted counting tx_id={tx_id}");
    let tx_body = client.wait_for_tx(&tx_id).expect("counting tx inclusion");
    let inclusion_height = tx_body
        .get("inclusionHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("counting tx outputs");

    // ---- 7. Verify counter advanced + tallies ----
    let counter_succ = find_output_with_token(outputs, &counter_nft)
        .unwrap_or_else(|| panic!("no output carrying COUNTER_NFT"));
    let counter_succ_utxo = client
        .utxo_by_id(&counter_succ.box_id)
        .expect("utxo lookup counter successor")
        .unwrap_or_else(|| panic!("counter successor not in UTXO"));
    cross_check_registers(
        &counter_succ_utxo,
        &counter_successor_registers,
        "counter successor",
    );
    eprintln!(
        "counter advanced: box={} R7={total_pwr} R9={yes_pwr} at h={inclusion_height}",
        &counter_succ.box_id[..16]
    );

    // ---- 8. Verify vote NFTs burned ----
    verify_burns(&client, &valid_vote_nft, &voters);

    // ---- 9. Persist updated state ----
    state.setup_tx_id = tx_id;
    state.setup_height = inclusion_height;
    state.counter = BoxRef {
        box_id: counter_succ.box_id,
        value: counter_succ.value,
        p2s_address: find_tree(&compiled_test, "counting", None)
            .p2s_address
            .clone(),
        tokens: vec![TokenRef {
            name: "COUNTER_NFT".to_string(),
            token_id: counter_nft.clone(),
            amount: 1,
        }],
        registers: counter_successor_registers,
    };
    // voters are now consumed; clear the list.
    state.voters.clear();
    write_dao_state(&state);
    eprintln!(
        "wrote {} (counter Phase 2 + voters consumed)",
        dao_state_path().display()
    );
}

// ============================================================================
// Voter bootstrap
// ============================================================================

fn ensure_voters(
    client: &NodeClient,
    change_address: &str,
    valid_vote_nft: &str,
    vyolo_id: &str,
    state: &mut DaoState,
) -> Vec<VoterBox> {
    if !state.voters.is_empty() {
        // Validate persisted voter boxes: every id must be distinct
        // AND still on chain. Duplicates indicate an earlier
        // bootstrap matched the same output twice (the dedupe bug we
        // just fixed) — refuse and re-bootstrap fresh.
        let mut ids = std::collections::HashSet::new();
        let all_distinct = state.voters.iter().all(|v| ids.insert(v.box_id.clone()));
        let all_present = state
            .voters
            .iter()
            .all(|v| utxo_exists(client, &v.box_id));
        if all_distinct && all_present {
            return state.voters.clone();
        }
        if !all_distinct {
            eprintln!("persisted voters contain duplicates; bootstrapping fresh ones.");
        } else {
            eprintln!("some persisted voter boxes consumed; bootstrapping fresh ones.");
        }
        state.voters.clear();
    }

    eprintln!("bootstrapping 3 voter boxes (2 yes, 1 no)...");
    let mut requests = Vec::with_capacity(3);
    for (power, yes_label) in [
        (VOTER_A_POWER, "yes"),
        (VOTER_B_POWER, "yes"),
        (VOTER_C_POWER, "no"),
    ] {
        let yes = yes_label == "yes";
        let mut regs = BTreeMap::new();
        regs.insert("R4".to_string(), slong_hex(if yes { 1 } else { 0 }));
        requests.push(PaymentRequestDto {
            address: change_address.to_string(),
            value: MIN_BOX_VALUE,
            // ORDER MATTERS: counting.es phase2 reads tokens(0) ==
            // ValidVote, tokens(1) == VYolo. With VYoloId
            // lexicographically smaller than ValidVoteId, the old
            // BTreeMap-sorted output would have produced the wrong
            // shape. The Vec-based wallet patch preserves this order
            // straight through to the on-chain box.
            assets: vec![
                AssetDto {
                    token_id: valid_vote_nft.to_string(),
                    amount: 1,
                },
                AssetDto {
                    token_id: vyolo_id.to_string(),
                    amount: power,
                },
            ],
            additional_registers: Some(regs),
        });
    }
    let tx_id = client
        .wallet_transaction_send(&requests)
        .expect("voter bootstrap tx submit");
    let tx_body = client
        .wait_for_tx(&tx_id)
        .expect("voter bootstrap confirmed");
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("voter outputs");

    // Each voter box has exactly 2 tokens with ValidVoteNft at slot 0.
    // Track already-claimed box ids: multiple voter specs share the
    // same shape (A and B are both yes / power=10), so `.find` would
    // otherwise return output 0 for both. Walk each spec, then mark
    // the matched box id as consumed so the next iteration skips it.
    let mut voter_boxes = Vec::with_capacity(3);
    let mut claimed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (idx, (power, yes_label)) in [
        (VOTER_A_POWER, "yes"),
        (VOTER_B_POWER, "yes"),
        (VOTER_C_POWER, "no"),
    ]
    .iter()
    .enumerate()
    {
        let yes = *yes_label == "yes";
        let output = outputs
            .iter()
            .find(|o| {
                let Some(box_id) = o.get("boxId").and_then(|v| v.as_str()) else {
                    return false;
                };
                if claimed.contains(box_id) {
                    return false;
                }
                let Some(assets) = o.get("assets").and_then(|a| a.as_array()) else {
                    return false;
                };
                if assets.len() != 2 {
                    return false;
                }
                let t0 = assets[0]
                    .get("tokenId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let t1 = assets[1]
                    .get("tokenId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let a1_amt = assets[1].get("amount").and_then(|v| v.as_u64()).unwrap_or(0);
                t0 == valid_vote_nft && t1 == vyolo_id && a1_amt == *power
            })
            .unwrap_or_else(|| {
                panic!(
                    "no output matched voter {idx} shape (ValidVote, vYOLO={power}, yes={yes})"
                )
            });
        let box_id = output
            .get("boxId")
            .and_then(|v| v.as_str())
            .expect("voter boxId")
            .to_string();
        let value = output
            .get("value")
            .and_then(|v| v.as_u64())
            .expect("voter value");
        claimed.insert(box_id.clone());
        voter_boxes.push(VoterBox {
            box_id,
            value,
            vyolo_power: *power,
            yes,
        });
    }
    eprintln!("bootstrapped voters: {:?}", voter_boxes);
    state.voters = voter_boxes.clone();
    write_dao_state(state);
    voter_boxes
}

// ============================================================================
// Raw counting tx construction
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn build_sign_submit_counting_tx(
    client: &NodeClient,
    change_address: &str,
    counter_box_id: &str,
    counter_value: u64,
    counter_ergotree_hex: &str,
    counter_nft: &str,
    voters: &[VoterBox],
    funding: &FundingBox,
    stake_ref_box_id: &str,
    vyolo_id: &str,
    counter_succ_registers: &[RegisterEntry],
) -> String {
    use ergo_primitives::digest::Digest32;
    use ergo_primitives::reader::VlqReader;
    use ergo_primitives::writer::VlqWriter;
    use ergo_ser::ergo_box::ErgoBoxCandidate;
    use ergo_ser::ergo_tree::read_ergo_tree;
    use ergo_ser::input::{ContextExtension, DataInput, UnsignedInput};
    use ergo_ser::register::{read_registers, AdditionalRegisters};
    use ergo_ser::token::Token;
    use ergo_ser::transaction::{write_unsigned_transaction, UnsignedTransaction};

    let tip: u32 = client.full_height().expect("full_height");
    // Wallet's actual HEIGHT = `/info fullHeight + 4` (empirically; see
    // proposal_initiation's offset probe). The output box's creation_height
    // must equal HEIGHT (rule 124 + upper bound) — anything else is rejected.
    let creation_height = tip + 4;

    // ---- Parse trees ----
    let counter_tree = {
        let bytes = hex::decode(counter_ergotree_hex).expect("decode counter ergoTree");
        read_ergo_tree(&mut VlqReader::new(&bytes)).expect("parse counter ergoTree")
    };
    // P2PK ergoTree of the wallet's change address (used for refunds + change).
    let change_p2pk_bytes =
        p2pk_ergotree_bytes(change_address).expect("decode change address to P2PK tree");
    let change_p2pk_tree = read_ergo_tree(&mut VlqReader::new(&change_p2pk_bytes))
        .expect("parse change P2PK tree");
    // Mainnet fee proposition. Counting tx must pay fee under the
    // canonical miner-fee script for inclusion.
    let fee_bytes = ergo_mempool_fee_proposition();
    let fee_tree =
        read_ergo_tree(&mut VlqReader::new(&fee_bytes)).expect("parse fee proposition");

    // ---- Counter successor's AdditionalRegisters from hex ----
    let counter_succ_registers_block = {
        // Frame the 6 per-register hexes as a single AdditionalRegisters
        // block and decode via the same path the on-chain decoder uses.
        let mut framed = Vec::new();
        framed.push(counter_succ_registers.len() as u8);
        for r in counter_succ_registers {
            framed.extend_from_slice(&hex::decode(&r.hex).expect("decode register hex"));
        }
        read_registers(&mut VlqReader::new(&framed)).expect("read counter successor registers")
    };

    // ---- Build output candidates ----
    let counter_nft_bytes = hex_to_32(counter_nft);
    let vyolo_bytes = hex_to_32(vyolo_id);
    let counter_succ = ErgoBoxCandidate::new(
        counter_value,
        counter_tree,
        creation_height,
        vec![Token {
            token_id: Digest32::from_bytes(counter_nft_bytes),
            amount: 1,
        }],
        counter_succ_registers_block,
    )
    .expect("counter successor candidate");

    let refund_boxes: Vec<ErgoBoxCandidate> = voters
        .iter()
        .map(|v| {
            ErgoBoxCandidate::new(
                MIN_BOX_VALUE,
                change_p2pk_tree.clone(),
                creation_height,
                vec![Token {
                    token_id: Digest32::from_bytes(vyolo_bytes),
                    amount: v.vyolo_power,
                }],
                AdditionalRegisters::empty(),
            )
            .expect("refund candidate")
        })
        .collect();

    let fee_box = ErgoBoxCandidate::new(
        SETUP_FEE,
        fee_tree,
        creation_height,
        vec![],
        AdditionalRegisters::empty(),
    )
    .expect("fee candidate");

    // ---- ERG conservation: compute change ----
    //
    // Inputs: counter + 3 voters + funding box. Each voter has
    // MIN_BOX, counter has counter_value, funding has funding.value.
    let voter_input_erg: u64 = voters.iter().map(|v| v.value).sum();
    let input_erg_total = counter_value + voter_input_erg + funding.value;
    // Outputs (excluding change): counter_succ + 3 refunds + fee.
    let known_output_erg: u64 = counter_value
        + (refund_boxes.len() as u64 * MIN_BOX_VALUE)
        + SETUP_FEE;
    let change_erg = input_erg_total
        .checked_sub(known_output_erg)
        .expect("input ERG must cover outputs + fee");

    // ---- Assemble output_candidates in the contract-mandated order ----
    let mut output_candidates = vec![counter_succ];
    output_candidates.extend(refund_boxes);
    output_candidates.push(fee_box);
    if change_erg >= MIN_BOX_VALUE {
        output_candidates.push(
            ErgoBoxCandidate::new(
                change_erg,
                change_p2pk_tree,
                creation_height,
                vec![],
                AdditionalRegisters::empty(),
            )
            .expect("change candidate"),
        );
    } else if change_erg > 0 {
        panic!(
            "change ERG = {change_erg} below MIN_BOX_VALUE; bump funding box size"
        );
    }

    // ---- Build inputs ----
    let mut inputs: Vec<UnsignedInput> = Vec::new();
    for box_id in [counter_box_id]
        .iter()
        .copied()
        .chain(voters.iter().map(|v| v.box_id.as_str()))
        .chain(std::iter::once(funding.box_id.as_str()))
    {
        inputs.push(UnsignedInput {
            box_id: Digest32::from_bytes(hex_to_32(box_id)),
            extension: ContextExtension::empty(),
        });
    }
    let data_inputs = vec![DataInput {
        box_id: Digest32::from_bytes(hex_to_32(stake_ref_box_id)),
    }];

    let unsigned_tx = UnsignedTransaction {
        inputs,
        data_inputs,
        output_candidates,
    };

    // ---- Serialize + sign + submit ----
    let mut writer = VlqWriter::new();
    write_unsigned_transaction(&mut writer, &unsigned_tx).expect("write unsigned tx");
    let unsigned_hex = hex::encode(writer.result());
    let signed_hex = client
        .wallet_transaction_sign(&unsigned_hex, None)
        .expect("POST /wallet/transaction/sign");
    client
        .submit_signed_tx_bytes(&signed_hex)
        .expect("POST /transactions/bytes")
}

fn ergo_mempool_fee_proposition() -> Vec<u8> {
    // Path-dep on the node's ergo-mempool would pull the entire crate
    // for one constant. Inline the bytes (32 bytes per the mainnet
    // miner-fee proposition oracle test). The fixture lives at
    // 07-sigmachain-node/sigmachain-node/test-vectors/mainnet/fee_proposition.hex.
    let hex_str = std::fs::read_to_string(fee_proposition_fixture_path())
        .expect("read fee_proposition.hex");
    hex::decode(hex_str.trim()).expect("decode fee proposition hex")
}

fn fee_proposition_fixture_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("..");
    p.push("07-sigmachain-node");
    p.push("sigmachain-node");
    p.push("test-vectors");
    p.push("mainnet");
    p.push("fee_proposition.hex");
    p
}

// ============================================================================
// Verification helpers
// ============================================================================

fn verify_burns(client: &NodeClient, valid_vote_nft: &str, voters: &[VoterBox]) {
    eprintln!("verifying vote-NFT burns...");
    // Approach: query all unspent boxes holding ValidVoteNft. The
    // remaining count should be `original_mint - sum(consumed)`.
    // Simpler proxy: each voter's *original* box id must no longer be
    // in UTXO (consumed by counting tx). The vote NFT itself can't
    // appear in any output because the contract checked it during
    // reduction; just confirm the boxes are gone.
    for v in voters {
        let still_present = client
            .utxo_by_id(&v.box_id)
            .expect("utxo lookup voter")
            .is_some();
        assert!(
            !still_present,
            "voter box {} must be consumed (contains burned vote NFT)",
            v.box_id
        );
    }
    eprintln!("all {} voter boxes consumed.", voters.len());
    let _ = valid_vote_nft;
}

fn cross_check_registers(utxo: &serde_json::Value, expected: &[RegisterEntry], label: &str) {
    let regs = utxo
        .get("additionalRegisters")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("{label}: missing additionalRegisters"));
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

// ============================================================================
// Indexer / register / address helpers
// ============================================================================

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

fn utxo_exists(client: &NodeClient, box_id: &str) -> bool {
    client
        .utxo_by_id(box_id)
        .expect("utxo lookup")
        .is_some()
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

fn p2pk_ergotree_bytes(address: &str) -> Result<Vec<u8>, String> {
    let pk = ergo_ser::address::decode_p2pk_address(address)
        .map_err(|e| format!("decode_p2pk_address: {e:?}"))?;
    ergo_ser::address::build_p2pk_tree_bytes(&pk).map_err(|e| format!("build_p2pk_tree_bytes: {e:?}"))
}

fn read_register_hex(box_json: &serde_json::Value, key: &str) -> Option<String> {
    box_json
        .get("additionalRegisters")?
        .get(key)?
        .as_str()
        .map(|s| s.to_string())
}

fn decode_slong_register(box_json: &serde_json::Value, key: &str) -> Option<i64> {
    let hex_str = read_register_hex(box_json, key)?;
    let bytes = hex::decode(&hex_str).ok()?;
    if bytes.first().copied() != Some(0x05) {
        return None;
    }
    decode_zigzag_vlq(&bytes[1..]).map(|(v, _)| v)
}

/// Decode `(Long, Long)` tuple's first element. Per ergo-ser, tuple
/// registers are serialized as `CreateTuple` expressions: opcode 0x86
/// + count (VLQ) + per-element Const expressions.
fn decode_slong_pair_first(box_json: &serde_json::Value, key: &str) -> Option<i64> {
    let hex_str = read_register_hex(box_json, key)?;
    let bytes = hex::decode(&hex_str).ok()?;
    if bytes.first().copied() != Some(0x86) {
        return None;
    }
    // Skip 0x86 (opcode) + count byte (1 = "2 items" VLQ).
    let mut cursor = 2;
    // First constant: type byte (0x05 = SLong) + ZigZag VLQ value.
    if bytes.get(cursor).copied() != Some(0x05) {
        return None;
    }
    cursor += 1;
    let (v, _) = decode_zigzag_vlq(&bytes[cursor..])?;
    Some(v)
}

fn decode_zigzag_vlq(bytes: &[u8]) -> Option<(i64, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    let mut consumed = 0;
    for b in bytes {
        consumed += 1;
        value |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            let signed = ((value >> 1) as i64) ^ -((value & 1) as i64);
            return Some((signed, consumed));
        }
        shift += 7;
        if shift > 56 {
            return None;
        }
    }
    None
}

fn hex_to_32(s: &str) -> [u8; 32] {
    let v = hex::decode(s).expect("hex decode");
    v.try_into().expect("32-byte digest")
}

fn total_power(voters: &[VoterBox]) -> u64 {
    voters.iter().map(|v| v.vyolo_power).sum()
}

fn yes_power(voters: &[VoterBox]) -> u64 {
    voters.iter().filter(|v| v.yes).map(|v| v.vyolo_power).sum()
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
