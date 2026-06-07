//! Phase 5.4.4 prep — DAO genesis setup on the live SigmaChain.
//!
//! Creates the counter box at Phase 0 ("no active proposal") so the
//! voting-lifecycle tests can spend it. Exercises the
//! `additionalRegisters` field newly added to `PaymentRequestDto` —
//! before this patch, the wallet had no way to emit an output carrying
//! R4-R9.
//!
//! The counter box shape (per counting.es header comment + progress doc):
//!
//! * script   = counting.es test-windows variant (`countingPhase = 5`)
//! * value    = MIN_BOX_VALUE (storage-rent floor)
//! * tokens   = [(COUNTER_NFT, 1)]
//! * R4 = Long  — vote deadline; set to `FAR_FUTURE_HEIGHT` so
//!                `isBeforeCounting` is true at setup time and stays
//!                true until a deliberate initiation tx overwrites R4.
//! * R5 = (Long, Long)  — `(0L, 0L)`: no proposal proportion, no
//!                votes-for yet.
//! * R6 = Coll[Byte]    — 32 zero bytes: recipient hash placeholder.
//! * R7 = Long          — `0L`: total votes accumulated.
//! * R8 = Long          — `0L`: initiation stake (proposer's vYOLO).
//! * R9 = Long          — `0L`: validation-vote subset.
//!
//! Preconditions: bake_yolodao_genesis_tokens + compile_yolodao_contracts
//! (the test-windows variant) have both run, the miner is running, the
//! wallet is unlocked and holds the COUNTER_NFT.
//!
//! Persists `06-governance/test-vectors/sigmachain-dao-state.json` with
//! the counter box id + height so downstream voting-lifecycle tests can
//! pick it up.

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use yolo_governance_node_tests::client::{AssetDto, NodeClient, PaymentRequestDto};
use yolo_governance_node_tests::config::TestConfig;
use yolo_governance_node_tests::registers::{coll_byte_hex, slong_hex, slong_pair_hex};

/// Storage-rent floor. The counter box carries one token and six
/// registers; 1_000_000 nanoERG sits comfortably above the
/// `min_value_per_byte * box_size` floor for that shape.
const COUNTER_BOX_VALUE: u64 = 1_000_000;

/// Vote-deadline sentinel for the Phase 0 (no-active-proposal) counter
/// box. `1e9` is far past any test-chain height we expect to reach
/// before a Phase 1 initiation overwrites R4.
const FAR_FUTURE_HEIGHT: i64 = 1_000_000_000;

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

#[derive(Debug, Serialize, Deserialize)]
struct DaoState {
    setup_tx_id: String,
    setup_height: u32,
    counter: BoxRef,
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
fn setup_dao_genesis_creates_counter_box_at_phase_zero() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let client = NodeClient::new(cfg);

    // Wallet must be unlocked.
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

    // Counter uses the test-windows variant so a downstream initiation
    // → counting → validation lifecycle test only has to grind through
    // ~5 blocks per phase rather than the production 12_960/1_080/4_320.
    let counter_tree = find_tree(&compiled_test, "counting", None);
    let counter_p2s = counter_tree.p2s_address.clone();
    let counter_tree_hex = counter_tree.tree_hex.to_ascii_lowercase();
    let counter_nft = token_by_name
        .get("COUNTER_NFT")
        .expect("missing COUNTER_NFT in deployment")
        .clone();
    eprintln!(
        "counter.p2s = {}...{}",
        &counter_p2s[..16],
        &counter_p2s[counter_p2s.len() - 8..]
    );

    // Pre-flight: refuse if the COUNTER_NFT already sits in a box
    // guarded by counting.es (a real previous setup). The NFT sitting
    // in the wallet's P2PK change box from the bake is the expected
    // pre-state and is NOT a duplicate — distinguish by comparing the
    // box's ergoTree against the test-windows counting tree.
    if let Some(existing) = lookup_unspent_by_token(&client, &counter_nft) {
        let existing_tree = existing
            .get("ergoTree")
            .and_then(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if existing_tree == counter_tree_hex {
            panic!(
                "COUNTER_NFT {} already sits in a counting-contract box on chain. \
                 Reset state before re-running setup_dao_genesis.",
                counter_nft
            );
        }
        eprintln!(
            "COUNTER_NFT currently held in non-counter box (likely wallet P2PK from bake). \
             Proceeding to mint counter genesis box."
        );
    }

    // Build per-register hex.
    let r4 = slong_hex(FAR_FUTURE_HEIGHT);
    let r5 = slong_pair_hex(0, 0);
    let r6 = coll_byte_hex(&[0u8; 32]);
    let r7 = slong_hex(0);
    let r8 = slong_hex(0);
    let r9 = slong_hex(0);

    let mut additional_registers = BTreeMap::new();
    additional_registers.insert("R4".to_string(), r4.clone());
    additional_registers.insert("R5".to_string(), r5.clone());
    additional_registers.insert("R6".to_string(), r6.clone());
    additional_registers.insert("R7".to_string(), r7.clone());
    additional_registers.insert("R8".to_string(), r8.clone());
    additional_registers.insert("R9".to_string(), r9.clone());

    let counter_request = PaymentRequestDto {
        address: counter_p2s.clone(),
        value: COUNTER_BOX_VALUE,
        assets: vec![AssetDto {
            token_id: counter_nft.clone(),
            amount: 1,
        }],
        additional_registers: Some(additional_registers),
    };

    let tx_id = client
        .wallet_transaction_send(&[counter_request])
        .expect("POST /wallet/transaction/send (counter setup)");
    eprintln!("submitted counter genesis tx_id={tx_id}");
    let tx_body = client
        .wait_for_tx(&tx_id)
        .expect("counter genesis tx inclusion");

    let inclusion_height = tx_body
        .get("inclusionHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("counter tx outputs");
    let counter_output = find_output_with_token(outputs, &counter_nft)
        .unwrap_or_else(|| panic!("no tx output carrying COUNTER_NFT {counter_nft}"));
    eprintln!(
        "counter box {} mined at h={inclusion_height}",
        &counter_output.box_id[..16]
    );

    // Confirm the new box is in the live UTXO set.
    let utxo = client
        .utxo_by_id(&counter_output.box_id)
        .expect("utxo lookup counter")
        .unwrap_or_else(|| panic!("counter box {} missing from UTXO", counter_output.box_id));

    // Cross-check the register block decoded by the indexer matches
    // what we sent — guards against any silent re-encoding drift.
    let utxo_regs = utxo
        .get("additionalRegisters")
        .and_then(|v| v.as_object())
        .expect("counter UTXO has additionalRegisters object");
    for (expected_key, expected_hex) in [
        ("R4", &r4),
        ("R5", &r5),
        ("R6", &r6),
        ("R7", &r7),
        ("R8", &r8),
        ("R9", &r9),
    ] {
        let actual = utxo_regs
            .get(expected_key)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("counter box missing {expected_key} in indexer view"));
        assert_eq!(
            actual.to_ascii_lowercase(),
            expected_hex.to_ascii_lowercase(),
            "counter {expected_key} mismatch: sent {expected_hex}, indexer reports {actual}"
        );
    }

    let state = DaoState {
        setup_tx_id: tx_id.clone(),
        setup_height: inclusion_height,
        counter: BoxRef {
            box_id: counter_output.box_id,
            value: counter_output.value,
            p2s_address: counter_p2s,
            tokens: vec![TokenRef {
                name: "COUNTER_NFT".to_string(),
                token_id: counter_nft,
                amount: 1,
            }],
            registers: vec![
                RegisterEntry {
                    key: "R4".to_string(),
                    hex: r4,
                    description: format!("vote deadline (Long) = {FAR_FUTURE_HEIGHT}"),
                },
                RegisterEntry {
                    key: "R5".to_string(),
                    hex: r5,
                    description: "(proportion, votes_for) = (0L, 0L)".to_string(),
                },
                RegisterEntry {
                    key: "R6".to_string(),
                    hex: r6,
                    description: "recipient ergotree hash (Coll[Byte], 32 zero bytes)".to_string(),
                },
                RegisterEntry {
                    key: "R7".to_string(),
                    hex: r7,
                    description: "total votes (Long) = 0".to_string(),
                },
                RegisterEntry {
                    key: "R8".to_string(),
                    hex: r8,
                    description: "initiation stake (Long) = 0".to_string(),
                },
                RegisterEntry {
                    key: "R9".to_string(),
                    hex: r9,
                    description: "validation votes (Long) = 0".to_string(),
                },
            ],
        },
    };

    let out_path = dao_state_path();
    let json = serde_json::to_string_pretty(&state).expect("serialize dao-state");
    std::fs::write(&out_path, json).expect("write sigmachain-dao-state.json");
    eprintln!(
        "wrote {} (counter box {})",
        out_path.display(),
        &state.counter.box_id[..12]
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

/// Look up the (assumed singleton) unspent box currently holding a
/// given token. Returns the first matching indexer record, or `None`
/// if no live box carries the token. Indexer route #20 returns a bare
/// array of `IndexedErgoBox` shapes — same pattern
/// `deposit_redeem_pair_1::lookup_unspent_box_by_token` uses to chase
/// the latest vault/reserve successor.
fn lookup_unspent_by_token(client: &NodeClient, token_id: &str) -> Option<serde_json::Value> {
    let path = format!("/blockchain/box/unspent/byTokenId/{}", token_id);
    let resp = client
        .raw_get_json_auth(&path)
        .expect("GET /blockchain/box/unspent/byTokenId");
    let items = resp.as_array()?;
    items.first().cloned()
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

fn test_vectors_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("test-vectors");
    p
}

fn dao_state_path() -> PathBuf {
    test_vectors_dir().join("sigmachain-dao-state.json")
}
