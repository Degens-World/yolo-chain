//! Phase 5.4.2 — Mint the YoloDAO genesis tokens against the live
//! SigmaChain node.
//!
//! For every token class enumerated in `06-governance/PARAMETERS.md`
//! the helper:
//!   1. Picks an unspent wallet box as `inputs[0]`.
//!   2. POSTs `/wallet/transaction/send` with `assets[0].tokenId =
//!      inputs[0].box_id` (the Ergo mint convention).
//!   3. Waits for the tx to land in `/blockchain/transaction/byId`.
//!   4. Records `(token_name, token_id, tx_id, mint_amount)` into a
//!      deployment artifact at
//!      `06-governance/test-vectors/sigmachain-deployment.json`.
//!
//! After this runs once on a fresh chain, every downstream Phase 5.4
//! test reads the deployment file for the token ids — they are stable
//! across the chain's lifetime (only invalidated by wiping the data
//! dir).
//!
//! Preconditions: see `SETUP.md`. The CPU miner MUST be running so
//! each mint tx can mine within `tx_wait_timeout`.

#![cfg(feature = "live")]

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use yolo_governance_node_tests::client::{NodeClient, PaymentRequestDto, WalletBoxEntry};
use yolo_governance_node_tests::config::TestConfig;

/// Minimum nanoYOLO value carried by the mint output box. SigmaChain
/// enforces `value >= box_size * min_value_per_byte` (~26,640 for a
/// small P2PK with one token); 1_000_000 is comfortably above and
/// matches the storage-rent guidance in 06-governance/PARAMETERS.md.
const MINT_BOX_VALUE: u64 = 1_000_000;

/// Default wallet fee (mainnet MinFee).
const MINT_FEE: u64 = 1_000_000;

/// Per-mint input-box value floor. Coinbase boxes on SigmaChain at
/// genesis difficulty are ~35e9 nanoYOLO, so this is trivially met,
/// but defensively reject anything smaller so the wallet doesn't
/// silently produce a sub-minimum change box.
const MIN_INPUT_VALUE: u64 = MINT_BOX_VALUE + MINT_FEE + MINT_BOX_VALUE;

/// Total vYOLO supply per `06-governance/PARAMETERS.md` line 16.
const VYOLO_TOTAL_SUPPLY: u64 = 177_412_882_500_000_000;

/// Valid Vote NFT supply — placeholder. The contract burns one per
/// voter per proposal; 1e6 is enough for an end-to-end test suite
/// that won't run more than a few dozen proposals.
const VALID_VOTE_NFT_SUPPLY: u64 = 1_000_000;

/// Proposal token supply — each proposal box carries one (qty=1
/// while pending, qty=2 once passed; burned on treasury withdrawal).
/// 1e4 covers any reasonable test suite — proposal lifecycle tests
/// use < 100 proposals total.
const PROPOSAL_TOKEN_SUPPLY: u64 = 10_000;

/// Manifest of token classes to mint. The order is consensus-bearing
/// for the deployment artifact's stable indexing — downstream tests
/// read by name, not by index, but the order is preserved for
/// reproducibility.
fn mint_manifest() -> Vec<(&'static str, u64)> {
    vec![
        ("YOLO_VAULT_STATE_NFT_1", 1),
        ("YOLO_VAULT_STATE_NFT_2", 1),
        ("YOLO_VAULT_STATE_NFT_3", 1),
        ("YOLO_VAULT_STATE_NFT_4", 1),
        ("YOLO_VAULT_STATE_NFT_5", 1),
        ("YOLO_VAULT_RESERVE_NFT_1", 1),
        ("YOLO_VAULT_RESERVE_NFT_2", 1),
        ("YOLO_VAULT_RESERVE_NFT_3", 1),
        ("YOLO_VAULT_RESERVE_NFT_4", 1),
        ("YOLO_VAULT_RESERVE_NFT_5", 1),
        ("VYOLO", VYOLO_TOTAL_SUPPLY),
        ("TREASURY_NFT", 1),
        ("COUNTER_NFT", 1),
        ("VALID_VOTE_NFT", VALID_VOTE_NFT_SUPPLY),
        ("PROPOSAL_TOKEN", PROPOSAL_TOKEN_SUPPLY),
    ]
}

#[derive(Debug, Serialize, Deserialize)]
struct MintedToken {
    name: String,
    token_id: String,
    tx_id: String,
    amount: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct Deployment {
    /// SigmaChain network name from `/info`.
    network: String,
    /// `/info.fullHeight` at the moment the deployment finished.
    final_height: u32,
    /// Wallet change address that owns every minted token.
    owner_address: String,
    /// One entry per mint manifest line, in manifest order.
    tokens: Vec<MintedToken>,
}

#[test]
fn bake_yolodao_genesis_tokens() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let block_wait_timeout = cfg.block_wait_timeout;
    let client = NodeClient::new(cfg);

    // Wallet has to be unlocked + caught up.
    let status = client.wallet_status().expect("GET /wallet/status");
    if !status.is_unlocked {
        client.wallet_unlock().expect("POST /wallet/unlock");
    }
    let status = client.wallet_status().expect("re-read /wallet/status");
    assert!(status.is_unlocked, "wallet must be unlocked");
    let change_address = status.change_address.clone();
    assert!(
        !change_address.is_empty(),
        "wallet change_address empty after unlock"
    );
    eprintln!("wallet ready. change_address={}", change_address);

    let info = client.info().expect("GET /info");
    eprintln!(
        "starting bake. network={:?} fullHeight={}",
        info.network,
        info.full_height.unwrap_or(0)
    );

    // Reserve enough unspent boxes — one per mint. Page size 200
    // exceeds the 14-token manifest with headroom for restarts.
    let pool = client
        .wallet_boxes_unspent(0, 200)
        .expect("GET /wallet/boxes/unspent");
    let usable: Vec<WalletBoxEntry> = pool
        .items
        .into_iter()
        .filter(|b| b.value >= MIN_INPUT_VALUE)
        .collect();
    let manifest = mint_manifest();
    assert!(
        usable.len() >= manifest.len(),
        "need at least {} usable unspent boxes (value >= {}), have {}",
        manifest.len(),
        MIN_INPUT_VALUE,
        usable.len()
    );

    let mut tokens: Vec<MintedToken> = Vec::with_capacity(manifest.len());
    for (idx, (name, amount)) in manifest.iter().enumerate() {
        let input = &usable[idx];
        eprintln!(
            "[{}/{}] minting {} (amount={}) via input box {}",
            idx + 1,
            manifest.len(),
            name,
            amount,
            &input.box_id
        );

        let request = PaymentRequestDto {
            address: change_address.clone(),
            value: MINT_BOX_VALUE,
            assets: vec![yolo_governance_node_tests::client::AssetDto {
                token_id: input.box_id.clone(),
                amount: *amount,
            }],
            additional_registers: None,
        };
        let tx_id = client
            .wallet_transaction_send_with_inputs(
                &[request],
                &[input.box_id.clone()],
                Some(MINT_FEE),
            )
            .unwrap_or_else(|e| {
                panic!("mint {} failed at /wallet/transaction/send: {e:?}", name)
            });

        let _ = client
            .wait_for_tx(&tx_id)
            .unwrap_or_else(|e| panic!("mint {} never confirmed: {e:?}", name));

        tokens.push(MintedToken {
            name: name.to_string(),
            token_id: input.box_id.clone(),
            tx_id,
            amount: *amount,
        });

        // Be polite to the mempool — give it a moment so the next
        // box-selection sees the prior tx applied. Empirically the
        // mempool admission is already serialized; a 200ms cushion is
        // enough on a single CPU miner.
        std::thread::sleep(Duration::from_millis(200));
    }

    let info_final = client.info().expect("GET /info post-bake");
    let deployment = Deployment {
        network: info_final.network.clone(),
        final_height: info_final.full_height.unwrap_or(0),
        owner_address: change_address,
        tokens,
    };

    let out_path = deployment_path();
    let out_dir = out_path
        .parent()
        .expect("deployment path has parent dir");
    std::fs::create_dir_all(out_dir).expect("create test-vectors dir");
    let json = serde_json::to_string_pretty(&deployment).expect("serialize deployment");
    std::fs::write(&out_path, json).expect("write deployment.json");
    eprintln!(
        "wrote {} ({} tokens, final height {})",
        out_path.display(),
        deployment.tokens.len(),
        deployment.final_height
    );
    // Sanity: the file we just wrote is parseable.
    let bytes = std::fs::read(&out_path).expect("read back deployment.json");
    let parsed: Deployment = serde_json::from_slice(&bytes).expect("re-parse deployment.json");
    assert_eq!(parsed.tokens.len(), deployment.tokens.len());

    // Defensive guard against the "never burn tokens" rule from
    // memory: every token id is unique (any collision would mean a
    // duplicate mint of the same token, which can only happen on
    // catastrophic state corruption — surface loudly).
    let mut seen = std::collections::HashSet::new();
    for t in &deployment.tokens {
        assert!(
            seen.insert(t.token_id.clone()),
            "duplicate token id {} for {}",
            t.token_id,
            t.name
        );
    }

    let _ = block_wait_timeout; // suppress unused-binding lint
}

fn deployment_path() -> PathBuf {
    // tests/bake_genesis.rs lives at CARGO_MANIFEST_DIR/tests, so the
    // governance crate root is two levels up — but
    // `06-governance/node-tests/Cargo.toml` IS the crate root, and the
    // test-vectors dir is a sibling of `tests/`. Go up one level
    // (out of `node-tests`) into `06-governance`, then into
    // `test-vectors`.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("test-vectors");
    p.push("sigmachain-deployment.json");
    p
}
