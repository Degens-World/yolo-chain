//! Phase 5.4.3 — Setup helper for vault/reserve pair 1 on the live
//! SigmaChain.
//!
//! Creates the initial vault and reserve boxes for pair 1 in a single
//! payment tx via the wallet API. The output box ids are persisted to
//! `06-governance/test-vectors/sigmachain-pair-1-state.json` so the
//! deposit and redeem tests can resolve them.
//!
//! Initial box shapes (mirror `06-governance/tests/integration_test.rs`
//! STEP 1 fixtures, adapted for live storage-rent minimums):
//!
//! * vault_1   = (script=vault.es[1], value=1 YOLO, tokens=[(STATE_NFT_1, 1)])
//! * reserve_1 = (script=reserve.es[1], value=MIN_BOX, tokens=[(RESERVE_NFT_1, 1), (VYOLO, TOTAL_SUPPLY/5)])
//!
//! Preconditions: bake_yolodao_genesis_tokens + compile_yolodao_contracts
//! have both run, the miner is running, the wallet is unlocked.

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use yolo_governance_node_tests::client::{AssetDto, NodeClient, PaymentRequestDto};
use yolo_governance_node_tests::config::TestConfig;

/// Initial value the vault carries — 1 YOLO (1e9 nano). The in-memory
/// integration test uses `N` (1e9) as the "min value for empty vault"
/// because storage rent rejects sub-minimum boxes; SigmaChain's
/// `min_value_per_byte * box_size` floor for a P2S box with one token
/// sits around ~30,000 nano, so 1e9 is comfortably above.
const VAULT_INITIAL_VALUE: u64 = 1_000_000_000;

/// Storage-rent floor for the reserve box. The reserve carries TWO
/// tokens (reserve NFT + vYOLO supply) so its size is larger; bump
/// the value a hair above the vault's headroom estimate.
const RESERVE_INITIAL_VALUE: u64 = 2_000_000;

/// Reserve fee for the setup tx.
const SETUP_FEE: u64 = 1_000_000;

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
}

#[derive(Debug, Serialize, Deserialize)]
struct PairState {
    pair: u8,
    setup_tx_id: String,
    setup_height: u32,
    vault: BoxRef,
    reserve: BoxRef,
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
    #[allow(dead_code)]
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
fn setup_pair_1_creates_vault_and_reserve_boxes() {
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
    let compiled = load_compiled();
    let token_by_name: BTreeMap<String, String> = deployment
        .tokens
        .into_iter()
        .map(|t| (t.name, t.token_id))
        .collect();

    let vault_1_p2s = find_tree(&compiled, "vault", Some(1)).p2s_address.clone();
    let reserve_1_p2s = find_tree(&compiled, "reserve", Some(1)).p2s_address.clone();
    let state_nft_1 = token_by_name
        .get("YOLO_VAULT_STATE_NFT_1")
        .expect("missing STATE_NFT_1")
        .clone();
    let reserve_nft_1 = token_by_name
        .get("YOLO_VAULT_RESERVE_NFT_1")
        .expect("missing RESERVE_NFT_1")
        .clone();
    let vyolo = token_by_name
        .get("VYOLO")
        .expect("missing VYOLO")
        .clone();

    // PARAMETERS.md line 26: "vYOLO per reserve (initial) | TOTAL_SUPPLY / 5"
    let total_vyolo: u64 = 177_412_882_500_000_000;
    let initial_reserve_vyolo: u64 = total_vyolo / 5;
    eprintln!(
        "vault.p2s = {} reserve.p2s = {}",
        &vault_1_p2s[..16],
        &reserve_1_p2s[..16]
    );
    eprintln!(
        "initial reserve vYOLO = {} ({:.3e})",
        initial_reserve_vyolo, initial_reserve_vyolo as f64
    );

    // Bundling vault + reserve into a single tx blows the wallet
    // self-verify init-cost cap because the change output ends up
    // carrying many token-bearing inputs the selector grabbed. Two
    // back-to-back txs land cleanly: one for the vault box, one for
    // the reserve box.
    let vault_request = PaymentRequestDto {
        address: vault_1_p2s.clone(),
        value: VAULT_INITIAL_VALUE,
        assets: vec![AssetDto {
            token_id: state_nft_1.clone(),
            amount: 1,
        }],
        additional_registers: None,
    };
    let vault_tx = client
        .wallet_transaction_send(&[vault_request])
        .expect("vault setup tx submit");
    eprintln!("submitted vault setup tx_id={vault_tx}");
    let vault_tx_body = client.wait_for_tx(&vault_tx).expect("vault setup confirm");
    let vault_height = vault_tx_body
        .get("inclusionHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let vault_outputs = vault_tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("vault outputs");
    let vault_box = find_output_with_token(vault_outputs, &state_nft_1)
        .unwrap_or_else(|| panic!("no output carrying state_nft_1 {}", state_nft_1));
    eprintln!(
        "vault box {} mined at h={vault_height}",
        &vault_box.box_id[..16]
    );

    let reserve_request = PaymentRequestDto {
        address: reserve_1_p2s.clone(),
        value: RESERVE_INITIAL_VALUE,
        assets: vec![
            AssetDto {
                token_id: reserve_nft_1.clone(),
                amount: 1,
            },
            AssetDto {
                token_id: vyolo.clone(),
                amount: initial_reserve_vyolo,
            },
        ],
        additional_registers: None,
    };
    let reserve_tx = client
        .wallet_transaction_send(&[reserve_request])
        .expect("reserve setup tx submit");
    eprintln!("submitted reserve setup tx_id={reserve_tx}");
    let reserve_tx_body = client
        .wait_for_tx(&reserve_tx)
        .expect("reserve setup confirm");
    let inclusion_height = reserve_tx_body
        .get("inclusionHeight")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let reserve_outputs = reserve_tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("reserve outputs");
    let reserve_box = find_output_with_token(reserve_outputs, &reserve_nft_1)
        .unwrap_or_else(|| panic!("no output carrying reserve_nft_1 {}", reserve_nft_1));
    eprintln!(
        "reserve box {} mined at h={inclusion_height}",
        &reserve_box.box_id[..16]
    );

    // Record the reserve tx as the "setup_tx_id" in the persisted
    // state — it's the later of the two and pins the post-setup tip.
    let tx_id = reserve_tx;

    // Confirm both are still in the UTXO set (mined + applied + not
    // spent by anyone yet).
    assert!(
        client
            .utxo_by_id(&vault_box.box_id)
            .expect("utxo lookup vault")
            .is_some(),
        "vault box {} not in UTXO set",
        vault_box.box_id
    );
    assert!(
        client
            .utxo_by_id(&reserve_box.box_id)
            .expect("utxo lookup reserve")
            .is_some(),
        "reserve box {} not in UTXO set",
        reserve_box.box_id
    );

    let state = PairState {
        pair: 1,
        setup_tx_id: tx_id,
        setup_height: inclusion_height,
        vault: BoxRef {
            box_id: vault_box.box_id,
            value: vault_box.value,
            p2s_address: vault_1_p2s,
            tokens: vec![TokenRef {
                name: "YOLO_VAULT_STATE_NFT_1".to_string(),
                token_id: state_nft_1,
                amount: 1,
            }],
        },
        reserve: BoxRef {
            box_id: reserve_box.box_id,
            value: reserve_box.value,
            p2s_address: reserve_1_p2s,
            tokens: vec![
                TokenRef {
                    name: "YOLO_VAULT_RESERVE_NFT_1".to_string(),
                    token_id: reserve_nft_1,
                    amount: 1,
                },
                TokenRef {
                    name: "VYOLO".to_string(),
                    token_id: vyolo,
                    amount: initial_reserve_vyolo,
                },
            ],
        },
    };

    let out_path = pair_state_path(1);
    let json = serde_json::to_string_pretty(&state).expect("serialize state");
    std::fs::write(&out_path, json).expect("write pair-1-state.json");
    let _ = SETUP_FEE; // keep constant referenced
    eprintln!(
        "wrote {} (vault box {}, reserve box {})",
        out_path.display(),
        &state.vault.box_id[..12],
        &state.reserve.box_id[..12]
    );
}

// ---- helpers ----

#[derive(Debug)]
struct OutputBox {
    box_id: String,
    value: u64,
}

fn find_output_with_token(
    outputs: &[serde_json::Value],
    token_id: &str,
) -> Option<OutputBox> {
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

fn load_deployment() -> Deployment {
    let p = test_vectors_dir().join("sigmachain-deployment.json");
    let bytes = std::fs::read(&p).expect("read deployment.json");
    serde_json::from_slice(&bytes).expect("parse deployment.json")
}

fn load_compiled() -> CompiledDeployment {
    let p = test_vectors_dir().join("sigmachain-deployment-trees.json");
    let bytes = std::fs::read(&p).expect("read deployment-trees.json");
    serde_json::from_slice(&bytes).expect("parse deployment-trees.json")
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

fn pair_state_path(pair: u8) -> PathBuf {
    test_vectors_dir().join(format!("sigmachain-pair-{}-state.json", pair))
}
