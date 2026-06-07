//! Phase 5.4.1 — single-tx round-trip against the live SigmaChain node.
//!
//! Gated by the `live` feature so a plain `cargo test` compiles to
//! nothing in this file.
//!
//! Preconditions (see SETUP.md):
//!   - node running on YOLO_NODE_URL (default 127.0.0.1:9054)
//!   - api.security.api_key_hash matches Blake2b256(YOLO_NODE_API_KEY)
//!   - wallet restored (one-shot) and the password is exported as
//!     YOLO_WALLET_PASSWORD
//!   - chain past the 4,320-block miner_reward_delay so balances > 0
//!   - CPU miner running in another shell (so the round-trip mines
//!     within the test's tx-wait timeout)
//!
//! Run:
//!   set -a; source .env; set +a
//!   cargo test --features live live_node_single_tx_round_trip \
//!       -- --nocapture --test-threads=1

#![cfg(feature = "live")]

use yolo_governance_node_tests::client::{NodeClient, PaymentRequestDto};
use yolo_governance_node_tests::config::TestConfig;

/// 1,000,000 nanoYOLO. SigmaChain enforces a per-box minimum of
/// `min_value_per_byte * box_size` (≈ 26,640 nanoYOLO for a small P2PK
/// box at the current MonetaryParams). Sending a round 1e6 stays
/// comfortably above the floor without forcing us to track the exact
/// box-size math here.
const SEND_VALUE: u64 = 1_000_000;

#[test]
fn live_node_single_tx_round_trip() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let client = NodeClient::new(cfg);

    // 1. Confirm node is reachable + record start height.
    let start_info = client
        .info()
        .expect("GET /info failed; is the node running?");
    let start_height = start_info.full_height.unwrap_or(0);
    eprintln!(
        "node up: name={:?} network={:?} fullHeight={}",
        start_info.name, start_info.network, start_height
    );
    assert!(
        start_height >= 4_320,
        "chain is at height {start_height}; needs >= 4,320 (miner_reward_delay) \
         before any miner reward is spendable. Run the CPU miner more."
    );

    // 2. Wallet status — unlock if needed.
    let mut status = client
        .wallet_status()
        .expect("GET /wallet/status failed; api_key wrong?");
    if !status.is_initialized {
        panic!(
            "wallet is not initialized. Run the /wallet/restore ceremony per SETUP.md before tests."
        );
    }
    if !status.is_unlocked {
        eprintln!("wallet locked; unlocking");
        client
            .wallet_unlock()
            .expect("POST /wallet/unlock failed; wrong password?");
        status = client.wallet_status().expect("re-read /wallet/status");
        assert!(status.is_unlocked, "wallet still reports locked after unlock");
    }
    assert!(
        !status.change_address.is_empty(),
        "wallet change_address is empty after unlock; wallet scanner not ready?"
    );
    let change_address = status.change_address.clone();
    eprintln!("wallet ready. change_address={}", change_address);

    // 3. Balance gate.
    let balances = client
        .wallet_balances()
        .expect("GET /wallet/balances failed");
    eprintln!(
        "wallet height={} balance={} nanoYOLO assets={}",
        balances.height,
        balances.balance,
        balances.assets.len()
    );
    assert!(
        balances.balance > SEND_VALUE,
        "wallet balance {} <= SEND_VALUE {}. Mine more blocks past the 4,320 maturity \
         gate, or wait for the wallet scanner to catch up (wallet_height={}).",
        balances.balance,
        SEND_VALUE,
        balances.height,
    );

    // 4. Build + send.
    let requests = vec![PaymentRequestDto {
        address: change_address.clone(),
        value: SEND_VALUE,
        assets: vec![],
        additional_registers: None,
    }];
    let tx_id = client
        .wallet_transaction_send(&requests)
        .expect("POST /wallet/transaction/send failed");
    eprintln!("submitted tx_id={tx_id}");
    assert_eq!(tx_id.len(), 64, "tx_id should be 32-byte hex");

    // 5. Wait for inclusion. Requires the CPU miner is running.
    let tx_body = client
        .wait_for_tx(&tx_id)
        .expect("tx never appeared in the canonical chain — is the CPU miner running?");
    eprintln!("tx mined. body keys: {:?}", json_keys(&tx_body));

    // 6. Pull the first output box id and confirm it exists in the utxo
    //    set. /transactions/byId returns Scala-shaped JSON; the first
    //    output's `boxId` is the field we want.
    let first_box_id = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|b| b.get("boxId"))
        .and_then(|b| b.as_str())
        .expect("could not extract outputs[0].boxId from tx body");
    eprintln!("first output box_id={first_box_id}");

    let utxo = client
        .utxo_by_id(first_box_id)
        .expect("GET /utxo/byId failed");
    let utxo = utxo.expect("first output box is not in the utxo set; was it spent?");
    let utxo_value = utxo
        .get("value")
        .and_then(|v| v.as_u64())
        .expect("utxo value missing");
    assert_eq!(
        utxo_value, SEND_VALUE,
        "round-trip value mismatch: sent {} but utxo holds {}",
        SEND_VALUE, utxo_value
    );

    eprintln!("OK — round trip complete from height {start_height} → tx {tx_id}");
}

fn json_keys(v: &serde_json::Value) -> Vec<String> {
    v.as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}
