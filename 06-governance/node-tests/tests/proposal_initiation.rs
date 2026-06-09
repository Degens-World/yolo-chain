//! Phase 5.4.4 — Proposal initiation (counting.es Phase 1).
//!
//! Walks the counter NFT from Phase 0 ("no active proposal") into
//! Phase 1 ("voting open"). Exercises every shape touched by
//! initiation:
//!   * spending a contract-guarded P2S box (the counter) as a regular input
//!   * referencing a vYOLO stake box as `dataInputs(0)` (read-only)
//!   * emitting a register-bearing successor box at the same script
//!   * creating a fresh proposal box (proposal.es, state-token qty 1)
//!
//! Tx structure:
//!   INPUTS(0)  = counter box at Phase 0 (counting.es P2S, COUNTER_NFT)
//!   INPUTS(1+) = wallet-owned funding (carries PROPOSAL_TOKEN + ERG)
//!   dataInputs(0) = stake reference box (VYOLO as token(0), ≥ initiationHurdle)
//!   OUTPUTS(0) = counter successor at Phase 1 (R4..R9 advanced per spec)
//!   OUTPUTS(1) = proposal box at qty 1 (proposal.es, R4..R9 set)
//!   OUTPUTS(2) = fee, OUTPUTS(3) = change (wallet-emitted)
//!
//! Preconditions: setup_dao_genesis has run (counter box exists at
//! Phase 0 with `R4 = FAR_FUTURE_HEIGHT` sentinel — used here to
//! refuse a re-run after the counter already advanced).
//!
//! The stake reference box (a P2PK box with vYOLO as its first and
//! only token, ≥ initiationHurdle in amount) is bootstrapped inline
//! on first run and persisted to `sigmachain-dao-state.json` so
//! subsequent voting-lifecycle tests can re-use it.

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use blake2::{Blake2b, Digest};
use serde::{Deserialize, Serialize};
use yolo_governance_node_tests::client::{AssetDto, NodeClient, PaymentRequestDto};
use yolo_governance_node_tests::config::TestConfig;
use yolo_governance_node_tests::registers::{
    coll_byte_hex, sint_hex, slong_hex, slong_pair_hex,
};

type Blake2b256 = Blake2b<blake2::digest::consts::U32>;

// ---- Governance constants (from PARAMETERS.md + counting.es) ----

/// Initiation hurdle in vYOLO nanocoins (= 100,000 vYOLO).
const INITIATION_HURDLE: u64 = 100_000_000_000_000;

/// Stake reference box vYOLO amount — comfortably above the hurdle.
const STAKE_REF_VYOLO: u64 = 200_000_000_000_000;

/// Test-windows voting window — counting.es is compiled with
/// `votingWindow = 5` in the test-windows variant.
const VOTING_WINDOW_TEST: i64 = 5;

/// Proportion the proposal asks for, in PARAMETERS.md's units
/// (Denom = 10_000_000). 100_000 = 1% — below the elevatedProportion
/// threshold (1_000_000 = 10%), so `requiredSupport = minimumSupport = 5000`.
const PROPOSAL_PROPORTION: i64 = 100_000;

/// Support threshold the proposal bot would auto-set for proportions
/// below the elevated threshold. Off-chain convention; the contract
/// doesn't read R7/R8/R9.
const PROPOSAL_SUPPORT_BPS: i32 = 5000;

/// Storage-rent floor for boxes the wallet emits in this test.
const MIN_BOX_VALUE: u64 = 1_000_000;

/// Setup tx fee — leaves headroom for wallet-side init-cost on a
/// register-bearing multi-output tx.
/// Setup-tx fee — starts well above the prior session's bumped value
/// so a fresh submit replaces any stale initiation tx still parked
/// in the mempool. The outer retry loop doubles on each round, so the
/// effective ceiling reaches >2 billion nanoERG after 8 doublings.
const SETUP_FEE: u64 = 50_000_000;

// ---- Persisted state shapes (matching setup_dao_genesis output) ----

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
    /// Bootstrapped on first initiation-test run; reused thereafter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    stake_reference: Option<StakeRefBox>,
    /// Set after initiation; reused by counting / validation tests.
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
fn proposal_initiation_advances_counter() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let client = NodeClient::new(cfg);

    let status = client.wallet_status().expect("GET /wallet/status");
    if !status.is_unlocked {
        client.wallet_unlock().expect("POST /wallet/unlock");
    }
    let status = client.wallet_status().expect("re-read /wallet/status");
    assert!(status.is_unlocked, "wallet must be unlocked");
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
    let counter_p2s = counter_tree.p2s_address.clone();
    let counter_tree_hex = counter_tree.tree_hex.to_ascii_lowercase();
    let proposal_tree = find_tree(&compiled_test, "proposal", None);
    let proposal_p2s = proposal_tree.p2s_address.clone();
    let counter_nft = token_by_name
        .get("COUNTER_NFT")
        .expect("missing COUNTER_NFT")
        .clone();
    let proposal_token = token_by_name
        .get("PROPOSAL_TOKEN")
        .expect("missing PROPOSAL_TOKEN")
        .clone();
    let vyolo_id = token_by_name.get("VYOLO").expect("missing VYOLO").clone();

    let mut state = load_dao_state();

    // ---- 1. Reconcile counter box ----
    //
    // dao-state may point at a stale counter box id if a prior session
    // advanced the counter. Resolve via COUNTER_NFT — singleton, one
    // current holder.
    let live_counter = lookup_unspent_by_token(&client, &counter_nft).unwrap_or_else(|| {
        panic!(
            "COUNTER_NFT {} not in any unspent box. Run setup_dao_genesis first.",
            counter_nft
        )
    });
    let counter_box_id = live_counter
        .get("boxId")
        .and_then(|v| v.as_str())
        .expect("counter box missing boxId")
        .to_string();
    let counter_value = live_counter
        .get("value")
        .and_then(|v| v.as_u64())
        .expect("counter box missing value");
    let counter_ergotree = live_counter
        .get("ergoTree")
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    assert_eq!(
        counter_ergotree, counter_tree_hex,
        "COUNTER_NFT held in a box whose ergoTree is NOT the counting test-windows tree — \
         likely a chain mismatch with the compiled deployment"
    );

    // If the counter is already past Phase 0 (R4 != FAR_FUTURE_HEIGHT
    // sentinel set by setup_dao_genesis), the chain has already
    // observed an initiation. Treat that as a no-op success for this
    // test as long as a proposal-qty-1 box also exists — that's the
    // exact post-condition this test promises. Otherwise the chain
    // state is inconsistent and we surface for the operator.
    let counter_r4 = live_counter
        .get("additionalRegisters")
        .and_then(|r| r.get("R4"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let phase0_r4 = slong_hex(1_000_000_000).to_ascii_lowercase();
    if counter_r4 != phase0_r4 {
        return adopt_existing_initiation(
            &client,
            &mut state,
            &live_counter,
            &counter_box_id,
            counter_value,
            &counter_p2s,
            &counter_nft,
            &proposal_token,
            &proposal_tree.p2s_address,
            &proposal_tree.tree_hex.to_ascii_lowercase(),
        );
    }
    eprintln!(
        "counter at Phase 0. box={} value={} R4={}",
        &counter_box_id[..16],
        counter_value,
        &counter_r4[..16.min(counter_r4.len())]
    );

    // ---- 2. Ensure stake reference box exists ----
    let stake_ref = match state.stake_reference.clone() {
        Some(s) if utxo_exists(&client, &s.box_id) => {
            eprintln!(
                "reusing stake reference box {} (vYOLO={})",
                &s.box_id[..16],
                s.vyolo_amount
            );
            s
        }
        _ => bootstrap_stake_reference(&client, &change_address, &vyolo_id, &mut state),
    };

    // ---- 3. Find wallet boxes for PROPOSAL_TOKEN + extra ERG ----
    //
    // The PROPOSAL_TOKEN box was minted at MIN_BOX_VALUE (1M nanoERG)
    // during the bake; combined with the counter input (1M), total
    // input ERG would be 2M, leaving the tx 1M short of the
    // (counter_successor 1M + proposal 1M + fee 1M) requirement.
    // Pull a separate plain funding box to cover the gap (and any
    // change rounding).
    let proposal_funding = pick_wallet_box_with_token(&client, &proposal_token, 1)
        .unwrap_or_else(|| {
            panic!(
                "no wallet box carries PROPOSAL_TOKEN {proposal_token}. \
                 Was the bake completed?"
            )
        });
    eprintln!(
        "PROPOSAL_TOKEN funding box {} value={} amt={}",
        &proposal_funding.box_id[..16],
        proposal_funding.value,
        proposal_funding.token_amount
    );

    // counter + proposal_token_box together cover at most ~2M ERG;
    // ask for headroom (10M) so any change-output dust folds without
    // putting us back over the cliff.
    let extra_funding_min: u64 = 10 * MIN_BOX_VALUE;
    let extra_funding = pick_plain_funding_box(&client, extra_funding_min, &proposal_funding.box_id)
        .unwrap_or_else(|| {
            panic!(
                "no plain funding box with >= {extra_funding_min} nanoERG. \
                 Mine a few more blocks or top up the wallet."
            )
        });
    eprintln!(
        "extra funding box {} value={}",
        &extra_funding.box_id[..16],
        extra_funding.value
    );

    // ---- 4. Recipient ergotree (raw P2PK of wallet's change addr) ----
    //
    // Used both as proposal.R5 (raw bytes, validated at phase 3 via
    // blake2b256 against counter.R6) and to derive the recipient hash.
    let recipient_ergotree =
        p2pk_ergotree_bytes(&change_address).expect("decode change address to P2PK ergotree");
    let recipient_hash = blake2b256(&recipient_ergotree);

    // ---- 5. Counter successor registers (Phase 1) ----
    //
    // counting.es phase1 asserts R7 == 0, R9 == 0, R4 == HEIGHT + votingWindow,
    // R8 >= initiationHurdle. R4 is *exact equality*, so the height
    // we sample here MUST be the height the next block (the one
    // containing this tx) will have. The wallet builds its pre-header
    // with `height = committed_tip + 1`, so we query tip immediately
    // before constructing the registers and submitting — any block
    // mined between this read and the submit would shift the
    // wallet's HEIGHT and invalidate R4. Single-miner testnet makes
    // that race exceedingly unlikely; if it ever fires the symptom is
    // the wallet sign rejecting with `TrivialProp(false)` — handled
    // by the retry loop below.
    //
    // Retry strategy: the contract requires `R4 == HEIGHT + votingWindow`
    // by EXACT equality. A block mining in the window between our
    // `full_height()` read and the wallet's internal `committed_tip()`
    // read at sign time shifts HEIGHT by +1 and invalidates R4.
    // Each retry re-queries tip and rebuilds the registers + tx,
    // catching the race within a small attempt budget.
    let inputs = vec![
        counter_box_id.clone(),
        proposal_funding.box_id.clone(),
        extra_funding.box_id.clone(),
    ];
    let data_inputs = vec![stake_ref.box_id.clone()];

    // Try R4 = tip + offset + votingWindow with empirically-confirmed
    // offset=4 first (see commit Phase 5.4.4 prep: HEIGHT-offset
    // discovery). `/info fullHeight` lags `committed_tip` by ~3 (status
    // struct caches) and the wallet's pre-header height adds +1, so
    // HEIGHT_sign = `/info` + 4 with a moving chain. Sweep ±2 in case
    // the lag drifts under load. The outer fee-bump loop re-enters
    // here from scratch when a submitted tx fails to mine within the
    // window (R4 went stale before miner picked it up); each outer
    // round doubles the fee so the replacement evicts our own pending
    // tx in the mempool.
    let height_offsets: [i64; 7] = [4, 5, 3, 6, 2, 7, 1];
    let max_outer_attempts: u32 = 15;
    let mut current_fee: u64 = SETUP_FEE;
    let mut counter_registers: Vec<RegisterEntry> = Vec::new();
    let mut proposal_registers: Vec<RegisterEntry> = Vec::new();
    let mut counter_r4_value: i64 = 0;
    let mut tx_id_out: Option<String> = None;
    let mut tx_body_out: Option<serde_json::Value> = None;
    'outer: for outer_attempt in 1..=max_outer_attempts {
        eprintln!("outer round {outer_attempt}: fee={current_fee}");
        let mut last_error: Option<String> = None;
        let mut tx_id: Option<String> = None;
    for (attempt, offset) in height_offsets.iter().enumerate() {
        let attempt = attempt as u32 + 1;
        let tip_height: i64 = client
            .full_height()
            .expect("GET /info full height")
            .into();
        counter_r4_value = tip_height + offset + VOTING_WINDOW_TEST;
        eprintln!(
            "attempt {attempt}: tip={tip_height} offset={offset} → assumed HEIGHT={} → R4={counter_r4_value}",
            tip_height + offset
        );
        let counter_r5 = slong_pair_hex(PROPOSAL_PROPORTION, 0);
        let counter_r6 = coll_byte_hex(&recipient_hash);
        counter_registers = build_registers(&[
            (
                "R4",
                slong_hex(counter_r4_value),
                format!("vote deadline = HEIGHT+window = {counter_r4_value}"),
            ),
            (
                "R5",
                counter_r5.clone(),
                format!("(proportion, votes_for) = ({PROPOSAL_PROPORTION}L, 0L)"),
            ),
            ("R6", counter_r6.clone(), "recipient ergotree hash".to_string()),
            ("R7", slong_hex(0), "total votes = 0L".to_string()),
            (
                "R8",
                slong_hex(INITIATION_HURDLE as i64),
                format!("initiation stake = {INITIATION_HURDLE}L (>= hurdle)"),
            ),
            ("R9", slong_hex(0), "validation votes = 0L".to_string()),
        ]);

        let proposal_r4 = slong_pair_hex(PROPOSAL_PROPORTION, 0);
        let proposal_r5 = coll_byte_hex(&recipient_ergotree);
        proposal_registers = build_registers(&[
            (
                "R4",
                proposal_r4.clone(),
                format!("(proportion, 0L) = ({PROPOSAL_PROPORTION}L, 0L)"),
            ),
            ("R5", proposal_r5.clone(), "recipient ergotree raw bytes".to_string()),
            (
                "R6",
                slong_hex(1),
                "validationHeight sentinel = 1 (must equal counter NFT qty)".to_string(),
            ),
            (
                "R7",
                sint_hex(PROPOSAL_SUPPORT_BPS),
                format!("supportBps = {PROPOSAL_SUPPORT_BPS} (50%)"),
            ),
            (
                "R8",
                sint_hex(counter_r4_value as i32),
                format!("votingWindowEnd (off-chain metadata) = {counter_r4_value}"),
            ),
            (
                "R9",
                sint_hex(tip_height as i32),
                format!("discussionDeadline (off-chain metadata) = {tip_height}"),
            ),
        ]);

        let counter_successor = PaymentRequestDto {
            address: counter_p2s.clone(),
            value: counter_value, // value preserved (contract: out0.value >= SELF.value)
            assets: vec![AssetDto {
                token_id: counter_nft.clone(),
                amount: 1,
            }],
            additional_registers: Some(register_map(&counter_registers)),
        };
        let proposal_output = PaymentRequestDto {
            address: proposal_p2s.clone(),
            value: MIN_BOX_VALUE,
            assets: vec![AssetDto {
                token_id: proposal_token.clone(),
                amount: 1,
            }],
            additional_registers: Some(register_map(&proposal_registers)),
        };

        // Diagnostic: on the first failing attempt, generate (but don't
        // sign) the unsigned tx and dump the counter-successor box
        // contents we'd be asking the wallet to sign. If the wallet
        // disagrees with what we constructed, the bug is in our
        // PaymentRequestDto → output mapping, not the contract.
        if attempt == 1 {
            match client.wallet_transaction_generate_unsigned(
                &[counter_successor.clone(), proposal_output.clone()],
                &inputs,
                &data_inputs,
                Some(SETUP_FEE),
            ) {
                Ok(unsigned_hex) => {
                    let bytes = hex::decode(&unsigned_hex).expect("decode unsigned hex");
                    eprintln!("=== unsigned tx diagnostic ===");
                    eprintln!("unsigned tx total bytes: {}", bytes.len());
                    if let Err(e) = diagnose_unsigned_tx(&bytes) {
                        eprintln!("(diagnostic decode failed: {e})");
                    }
                    eprintln!("=== end diagnostic ===");
                }
                Err(e) => eprintln!("generateUnsigned diagnostic failed: {e:?}"),
            }
        }

        match client.wallet_transaction_send_full(
            &[counter_successor, proposal_output],
            &inputs,
            &data_inputs,
            Some(current_fee),
        ) {
            Ok(id) => {
                eprintln!(
                    "attempt {attempt}: contract accepted R4={counter_r4_value} (offset={offset}). \
                     Wallet's contract HEIGHT = tip + {offset}."
                );
                tx_id = Some(id);
                break;
            }
            Err(e) => {
                let s = e.to_string();
                // Both `TrivialProp(false)` (sign-time contract reduction)
                // and `script_failed` (submit-time mempool re-eval) are
                // retryable: they both indicate R4 didn't match HEIGHT at
                // some evaluation point. A block can mine between sign
                // and submit and flip one or both checks.
                if s.contains("TrivialProp(false)")
                    || s.contains("script_failed")
                    || s.contains("double_spend_loser")
                {
                    let cause = if s.contains("TrivialProp(false)") {
                        "sign-time TrivialProp"
                    } else if s.contains("script_failed") {
                        "submit-time script_failed"
                    } else {
                        "mempool double_spend_loser"
                    };
                    eprintln!("attempt {attempt}: retry-able failure at offset={offset} ({cause})");
                    last_error = Some(s);
                    continue;
                }
                panic!("POST /wallet/transaction/send (initiation): {e:?}");
            }
        }
    }
        let Some(tid) = tx_id else {
            eprintln!(
                "outer round {outer_attempt}: all {} offsets rejected at fee={current_fee}; bumping fee. Last error: {}",
                height_offsets.len(),
                last_error.as_deref().unwrap_or("none")
            );
            current_fee *= 2;
            continue 'outer;
        };
        eprintln!("submitted initiation tx_id={tid}; waiting up to 15s for inclusion...");
        // Short bounded wait. Fast-mining testnet → inclusion happens
        // within 1-2 blocks if it's going to happen at all. If the
        // contract goes stale before the miner picks our tx, the
        // outer loop bumps the fee and resubmits with a fresh tip.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let body = loop {
            match client.transaction_by_id(&tid) {
                Ok(Some(body)) => break Some(body),
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        break None;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                Err(e) => panic!("transaction_by_id: {e:?}"),
            }
        };
        match body {
            Some(b) => {
                tx_id_out = Some(tid);
                tx_body_out = Some(b);
                break 'outer;
            }
            None => {
                eprintln!(
                    "outer round {outer_attempt}: tx {tid} not mined within 60s; bumping fee and resubmitting with fresh tip."
                );
                current_fee *= 2;
                continue 'outer;
            }
        }
    }
    let tx_id = tx_id_out.unwrap_or_else(|| {
        panic!("initiation tx never mined after {max_outer_attempts} outer rounds")
    });
    let tx_body = tx_body_out.expect("body set alongside tx_id_out");
    let inclusion_height = tx_body
        .get("inclusionHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("initiation tx outputs");

    // ---- 8. Verify counter advanced ----
    let counter_succ = find_output_with_token(outputs, &counter_nft)
        .unwrap_or_else(|| panic!("no output carrying COUNTER_NFT {counter_nft}"));
    let counter_succ_utxo = client
        .utxo_by_id(&counter_succ.box_id)
        .expect("utxo lookup counter successor")
        .unwrap_or_else(|| panic!("counter successor {} not in UTXO", counter_succ.box_id));
    cross_check_registers(&counter_succ_utxo, &counter_registers, "counter successor");
    eprintln!(
        "counter advanced: box={} R4={counter_r4_value} at h={inclusion_height}",
        &counter_succ.box_id[..16]
    );

    // ---- 9. Verify proposal box ----
    let proposal_out = find_output_with_token(outputs, &proposal_token)
        .unwrap_or_else(|| panic!("no output carrying PROPOSAL_TOKEN {proposal_token}"));
    let proposal_utxo = client
        .utxo_by_id(&proposal_out.box_id)
        .expect("utxo lookup proposal box")
        .unwrap_or_else(|| panic!("proposal box {} not in UTXO", proposal_out.box_id));
    cross_check_registers(&proposal_utxo, &proposal_registers, "proposal box");
    let proposal_assets = proposal_utxo
        .get("assets")
        .and_then(|a| a.as_array())
        .expect("proposal assets array");
    let qty = proposal_assets
        .iter()
        .find_map(|a| {
            (a.get("tokenId")?.as_str()? == proposal_token).then(|| a.get("amount")?.as_u64())
        })
        .flatten()
        .expect("PROPOSAL_TOKEN amount");
    assert_eq!(qty, 1, "proposal token must enter chain at qty=1 (pending)");
    eprintln!(
        "proposal box created: box={} qty=1 at h={inclusion_height}",
        &proposal_out.box_id[..16]
    );

    // ---- 10. Persist updated state ----
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
    state.stake_reference = Some(stake_ref);
    state.proposal_box = Some(BoxRef {
        box_id: proposal_out.box_id,
        value: proposal_out.value,
        p2s_address: proposal_p2s,
        tokens: vec![TokenRef {
            name: "PROPOSAL_TOKEN".to_string(),
            token_id: proposal_token,
            amount: 1,
        }],
        registers: proposal_registers,
    });

    write_dao_state(&state);
    eprintln!(
        "wrote {} (counter successor + proposal box + stake ref)",
        dao_state_path().display()
    );
}

/// The chain already shows a counter past Phase 0 — adopt the on-chain
/// state as the test's success post-condition, provided a matching
/// proposal-qty-1 box also exists. Persists both into
/// `sigmachain-dao-state.json` so downstream lifecycle tests can
/// resolve them without depending on a fresh initiation tx.
#[allow(clippy::too_many_arguments)]
fn adopt_existing_initiation(
    client: &NodeClient,
    state: &mut DaoState,
    live_counter: &serde_json::Value,
    counter_box_id: &str,
    counter_value: u64,
    counter_p2s: &str,
    counter_nft: &str,
    proposal_token: &str,
    proposal_p2s: &str,
    proposal_tree_hex: &str,
) {
    eprintln!(
        "counter is already past Phase 0 — adopting on-chain state as success."
    );
    let counter_registers = read_registers(live_counter, &["R4", "R5", "R6", "R7", "R8", "R9"]);
    for (k, v) in &counter_registers {
        eprintln!("  counter.{k} = {}", short_hex(v));
    }

    // Look up the proposal-qty-1 box. The indexer's
    // `/blockchain/box/unspent/byTokenId` returns all live boxes
    // carrying PROPOSAL_TOKEN — the proposal box is the singleton
    // copy at qty 1 guarded by proposal.es. Distinguish from any
    // funding box that might still carry residual qty by matching the
    // ergoTree against the test-windows proposal tree.
    let proposal_box = client
        .raw_get_json_auth(&format!(
            "/blockchain/box/unspent/byTokenId/{}",
            proposal_token
        ))
        .expect("GET /blockchain/box/unspent/byTokenId proposal")
        .as_array()
        .and_then(|items| {
            items.iter().find_map(|b| {
                let tree = b
                    .get("ergoTree")
                    .and_then(|v| v.as_str())?
                    .to_ascii_lowercase();
                if tree != proposal_tree_hex {
                    return None;
                }
                let qty = b
                    .get("assets")
                    .and_then(|a| a.as_array())?
                    .iter()
                    .find_map(|a| {
                        (a.get("tokenId")?.as_str()? == proposal_token)
                            .then(|| a.get("amount")?.as_u64())
                    })
                    .flatten()?;
                if qty != 1 {
                    return None;
                }
                Some(b.clone())
            })
        })
        .unwrap_or_else(|| {
            panic!(
                "counter is past Phase 0 but no proposal-qty-1 box found under \
                 proposal.es tree. Chain state is inconsistent — reset and re-run."
            );
        });
    let proposal_box_id = proposal_box
        .get("boxId")
        .and_then(|v| v.as_str())
        .expect("proposal boxId")
        .to_string();
    let proposal_value = proposal_box.get("value").and_then(|v| v.as_u64()).unwrap_or(0);
    let proposal_registers = read_registers(&proposal_box, &["R4", "R5", "R6", "R7", "R8", "R9"]);
    eprintln!(
        "found proposal qty=1 box {} value={}",
        &proposal_box_id[..16],
        proposal_value
    );
    for (k, v) in &proposal_registers {
        eprintln!("  proposal.{k} = {}", short_hex(v));
    }

    // Sanity checks: proposal.R4._1 (proportion) must match
    // counter.R5._1, and blake2b256(proposal.R5) must match counter.R6.
    // These mirror the validation-time checks counting.es phase3 runs.
    let counter_r5_hex = registers_get(&counter_registers, "R5").unwrap_or_default();
    let counter_r6_hex = registers_get(&counter_registers, "R6").unwrap_or_default();
    let proposal_r4_hex = registers_get(&proposal_registers, "R4").unwrap_or_default();
    let proposal_r5_hex = registers_get(&proposal_registers, "R5").unwrap_or_default();
    assert_eq!(
        proposal_r4_hex.to_ascii_lowercase(),
        counter_r5_hex.to_ascii_lowercase(),
        "proposal.R4 (proportion tuple) must equal counter.R5 — phase3 advancement check"
    );
    let proposal_r5_bytes = strip_coll_byte_header(&proposal_r5_hex)
        .expect("proposal.R5 not a Coll[Byte] constant");
    let expected_recipient_hash = blake2b256(&proposal_r5_bytes);
    let expected_counter_r6 = coll_byte_hex(&expected_recipient_hash);
    assert_eq!(
        counter_r6_hex.to_ascii_lowercase(),
        expected_counter_r6.to_ascii_lowercase(),
        "blake2b256(proposal.R5) must equal counter.R6 — phase3 recipient check"
    );

    let counter_registers_persisted = describe_registers(&counter_registers, "counter (adopted)");
    let proposal_registers_persisted = describe_registers(&proposal_registers, "proposal (adopted)");

    state.counter = BoxRef {
        box_id: counter_box_id.to_string(),
        value: counter_value,
        p2s_address: counter_p2s.to_string(),
        tokens: vec![TokenRef {
            name: "COUNTER_NFT".to_string(),
            token_id: counter_nft.to_string(),
            amount: 1,
        }],
        registers: counter_registers_persisted,
    };
    state.proposal_box = Some(BoxRef {
        box_id: proposal_box_id,
        value: proposal_value,
        p2s_address: proposal_p2s.to_string(),
        tokens: vec![TokenRef {
            name: "PROPOSAL_TOKEN".to_string(),
            token_id: proposal_token.to_string(),
            amount: 1,
        }],
        registers: proposal_registers_persisted,
    });
    write_dao_state(state);
    eprintln!(
        "wrote {} (adopted existing counter + proposal state)",
        dao_state_path().display()
    );
}

fn read_registers(box_json: &serde_json::Value, keys: &[&str]) -> Vec<(String, String)> {
    let regs = box_json
        .get("additionalRegisters")
        .and_then(|v| v.as_object());
    keys.iter()
        .filter_map(|k| {
            regs.and_then(|m| m.get(*k))
                .and_then(|v| v.as_str())
                .map(|s| ((*k).to_string(), s.to_string()))
        })
        .collect()
}

fn registers_get(regs: &[(String, String)], key: &str) -> Option<String> {
    regs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn describe_registers(regs: &[(String, String)], context: &str) -> Vec<RegisterEntry> {
    regs.iter()
        .map(|(k, h)| RegisterEntry {
            key: k.clone(),
            hex: h.clone(),
            description: format!("{context}: adopted from chain"),
        })
        .collect()
}

fn short_hex(s: &str) -> String {
    if s.len() <= 32 {
        s.to_string()
    } else {
        format!("{}…{}", &s[..14], &s[s.len() - 6..])
    }
}

/// Strip a `Coll[Byte]` constant's type-code (`0x0e`) + VLQ length
/// prefix and return the raw payload bytes. Used to recover the
/// recipient ergoTree from proposal.R5 for the blake2b256 cross-check
/// against counter.R6.
fn strip_coll_byte_header(hex_str: &str) -> Option<Vec<u8>> {
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.first().copied() != Some(0x0e) {
        return None;
    }
    let mut cursor = 1usize;
    let mut len: u64 = 0;
    let mut shift = 0u32;
    loop {
        if cursor >= bytes.len() {
            return None;
        }
        let b = bytes[cursor];
        cursor += 1;
        len |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 56 {
            return None;
        }
    }
    if cursor + len as usize != bytes.len() {
        return None;
    }
    Some(bytes[cursor..].to_vec())
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
        .expect("GET /blockchain/box/unspent/byTokenId");
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
    token_amount: u64,
}

/// Walk wallet pages until a plain (no-tokens) box with at least
/// `min_value` nanoERG is found. `exclude` skips a known input we're
/// already spending (the same box can't appear twice in INPUTS).
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
                token_amount: 0,
            });
        }
        offset = offset.checked_add(PAGE_SIZE)?;
    }
}

fn pick_wallet_box_with_token(
    client: &NodeClient,
    token_id: &str,
    min_amount: u64,
) -> Option<FundingBox> {
    // `/wallet/boxes/unspent` is paginated by box_id ascending. The
    // bake's change boxes sit at high box_ids and can land past any
    // single-page limit. Walk pages until the wallet returns empty.
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
            let Some(utxo) = client.utxo_by_id(&b.box_id).expect("utxo lookup") else {
                continue;
            };
            let Some(assets) = utxo.get("assets").and_then(|a| a.as_array()) else {
                continue;
            };
            for a in assets {
                let tid = a.get("tokenId").and_then(|t| t.as_str()).unwrap_or("");
                let amt = a.get("amount").and_then(|t| t.as_u64()).unwrap_or(0);
                if tid == token_id && amt >= min_amount {
                    return Some(FundingBox {
                        box_id: b.box_id.clone(),
                        value: b.value,
                        token_amount: amt,
                    });
                }
            }
        }
        offset = offset.checked_add(PAGE_SIZE)?;
    }
}

/// Decode a P2PK Base58 address into its canonical (non-segregated)
/// ergoTree wire bytes. Goes through the node's own `ergo-ser`
/// path-dep so the bytes match what the wallet stamps on its own
/// change boxes.
fn p2pk_ergotree_bytes(address: &str) -> Result<Vec<u8>, String> {
    let pk = ergo_ser::address::decode_p2pk_address(address)
        .map_err(|e| format!("decode_p2pk_address: {e:?}"))?;
    ergo_ser::address::build_p2pk_tree_bytes(&pk).map_err(|e| format!("build_p2pk_tree_bytes: {e:?}"))
}

fn blake2b256(bytes: &[u8]) -> Vec<u8> {
    let mut h = Blake2b256::new();
    h.update(bytes);
    h.finalize().to_vec()
}

/// Bootstrap a clean vYOLO-only P2PK box that the initiation tx can
/// reference as `dataInputs(0)`. The on-chain hurdle check requires
/// the box's `tokens(0)` to be `(VYoloId, ≥ initiationHurdle)`, so the
/// box must have NO other tokens — otherwise token sorting could
/// place vYOLO at slot 1+.
fn bootstrap_stake_reference(
    client: &NodeClient,
    change_address: &str,
    vyolo_id: &str,
    state: &mut DaoState,
) -> StakeRefBox {
    eprintln!(
        "bootstrapping stake reference box: sending {STAKE_REF_VYOLO} vYOLO \
         (={}) to change address with no other tokens",
        STAKE_REF_VYOLO / 1_000_000_000
    );
    let req = PaymentRequestDto {
        address: change_address.to_string(),
        value: MIN_BOX_VALUE,
        assets: vec![AssetDto {
            token_id: vyolo_id.to_string(),
            amount: STAKE_REF_VYOLO,
        }],
        additional_registers: None,
    };
    let tx_id = client
        .wallet_transaction_send(&[req])
        .expect("stake-reference setup tx submit");
    let tx_body = client
        .wait_for_tx(&tx_id)
        .expect("stake-reference setup confirm");
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("stake-ref tx outputs");

    // Find the output box that contains vYOLO at slot 0 AND only one
    // token — that's the box we explicitly requested (not the change
    // box, which carries multiple tokens).
    let stake_box = outputs
        .iter()
        .find_map(|o| {
            let assets = o.get("assets")?.as_array()?;
            if assets.len() != 1 {
                return None;
            }
            let tid = assets[0].get("tokenId")?.as_str()?;
            let amt = assets[0].get("amount")?.as_u64()?;
            if tid != vyolo_id || amt < INITIATION_HURDLE {
                return None;
            }
            let box_id = o.get("boxId")?.as_str()?.to_string();
            let value = o.get("value")?.as_u64()?;
            Some(StakeRefBox {
                box_id,
                value,
                vyolo_amount: amt,
            })
        })
        .expect("stake-ref tx did not produce a vYOLO-only output");
    eprintln!(
        "stake reference box {} value={} vYOLO={}",
        &stake_box.box_id[..16],
        stake_box.value,
        stake_box.vyolo_amount
    );
    state.stake_reference = Some(stake_box.clone());
    write_dao_state(state);
    stake_box
}

fn build_registers(entries: &[(&str, String, String)]) -> Vec<RegisterEntry> {
    entries
        .iter()
        .map(|(k, h, d)| RegisterEntry {
            key: (*k).to_string(),
            hex: h.clone(),
            description: d.clone(),
        })
        .collect()
}

fn register_map(regs: &[RegisterEntry]) -> BTreeMap<String, String> {
    regs.iter().map(|r| (r.key.clone(), r.hex.clone())).collect()
}

/// Verify that every register the wallet was asked to set appears
/// unchanged on the indexer's view of the resulting box. Catches any
/// silent re-encoding or dropped-field drift between the wallet
/// bridge and the on-chain decoder.
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
    let bytes = std::fs::read(&p).expect("read dao-state.json — run setup_dao_genesis first");
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

/// Parse an unsigned tx hex via ergo-ser and dump per-output box
/// contents (value, ergoTree length, tokens, registers).
fn diagnose_unsigned_tx(bytes: &[u8]) -> Result<(), String> {
    use ergo_primitives::reader::VlqReader;
    use ergo_primitives::writer::VlqWriter;
    use ergo_ser::register::write_registers;
    use ergo_ser::transaction::read_unsigned_transaction;

    let mut r = VlqReader::new(bytes);
    let utx = read_unsigned_transaction(&mut r)
        .map_err(|e| format!("read_unsigned_transaction: {e:?}"))?;
    eprintln!(
        "inputs={} data_inputs={} outputs={}",
        utx.inputs.len(),
        utx.data_inputs.len(),
        utx.output_candidates.len()
    );
    for (i, candidate) in utx.output_candidates.iter().enumerate() {
        eprintln!("--- OUTPUT[{i}] ---");
        eprintln!("  value={}", candidate.value);
        eprintln!("  creation_height={}", candidate.creation_height);
        eprintln!("  tokens={}", candidate.tokens.len());
        for (ti, tok) in candidate.tokens.iter().enumerate() {
            eprintln!(
                "    [{ti}] id={} amount={}",
                hex::encode(tok.token_id.as_bytes()),
                tok.amount
            );
        }
        let regs = &candidate.additional_registers;
        eprintln!("  registers={}", regs.count());
        let mut w = VlqWriter::new();
        write_registers(&mut w, regs).map_err(|e| format!("write_registers: {e:?}"))?;
        let block = w.result();
        // Skip count byte to print per-register payload hex.
        let per_register = ergo_ser::register::split_register_bytes(&block)
            .map_err(|e| format!("split_register_bytes: {e:?}"))?;
        for (j, payload) in per_register.iter().enumerate() {
            let slot_name = match j {
                0 => "R4",
                1 => "R5",
                2 => "R6",
                3 => "R7",
                4 => "R8",
                5 => "R9",
                _ => "R?",
            };
            eprintln!("    {} (slot {j}) = {}", slot_name, hex::encode(payload));
        }
    }
    Ok(())
}
