//! Phase 5.4.3 — Vault deposit + redeem round-trip against the live
//! SigmaChain.
//!
//! Mirrors `06-governance/tests/integration_test.rs` STEP 1 and
//! STEP 4 against vault/reserve pair 1. The deposit half locks
//! `DEPOSIT_AMOUNT` nano-YOLO into the vault and routes
//! `DEPOSIT_AMOUNT` vYOLO from the reserve to the wallet. The redeem
//! half is the inverse: vYOLO goes back into the reserve, YOLO comes
//! back to the wallet. Post-redeem the vault is at its initial value
//! and the reserve carries the full `TOTAL_SUPPLY / 5` vYOLO again.
//!
//! The tx is built by the wallet bridge: we hand it three explicit
//! input box ids (vault, reserve, wallet funding) and three payment
//! requests (vault successor, reserve successor, user recipient) in
//! the contract-required output order. The wallet appends the fee
//! and change outputs. Vault/reserve inputs are non-wallet contract
//! boxes whose scripts reduce to `sigmaProp(boolean)` given the right
//! context — the wallet's prover handles them with empty secrets.
//!
//! Preconditions: setup_pair_1 has run, miner is producing blocks,
//! wallet is unlocked.

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use yolo_governance_node_tests::client::{
    AssetDto, NodeClient, PaymentRequestDto,
};
use yolo_governance_node_tests::config::TestConfig;

/// 5 YOLO. Small enough to fit comfortably inside one ~35 YOLO
/// coinbase funding box (genesis difficulty miner reward), big enough
/// to make pre/post-tx asserts unambiguous.
const DEPOSIT_AMOUNT: u64 = 5 * 1_000_000_000;

/// vYOLO recipient box ERG.
const RECIPIENT_BOX_VALUE: u64 = 1_000_000;

/// Reserve fee (matches MIN_FEE in wallet_bridge.rs).
const TX_FEE: u64 = 1_000_000;

#[derive(Debug, Deserialize, Clone)]
struct PairState {
    #[allow(dead_code)]
    pair: u8,
    vault: BoxRef,
    reserve: BoxRef,
}

#[derive(Debug, Deserialize, Clone)]
struct BoxRef {
    box_id: String,
    value: u64,
    p2s_address: String,
    tokens: Vec<TokenRef>,
}

#[derive(Debug, Deserialize, Clone)]
struct TokenRef {
    name: String,
    token_id: String,
    amount: u64,
}

#[derive(Debug, Deserialize)]
struct DeployToken {
    name: String,
    token_id: String,
}

#[derive(Debug, Deserialize)]
struct Deployment {
    tokens: Vec<DeployToken>,
}

#[test]
fn deposit_then_redeem_round_trip_pair_1() {
    let cfg = TestConfig::from_env().expect("env vars not set; see SETUP.md");
    let client = NodeClient::new(cfg);
    let status = client.wallet_status().expect("/wallet/status");
    if !status.is_unlocked {
        client.wallet_unlock().expect("/wallet/unlock");
    }
    let status = client.wallet_status().expect("re-read /wallet/status");
    assert!(status.is_unlocked, "wallet must be unlocked");
    let change_address = status.change_address.clone();
    eprintln!("wallet ready. change_address={change_address}");

    let pair = load_pair_state();
    let deployment = load_deployment();
    // Reconcile pair state with on-chain reality. Each NFT is a
    // qty-1 singleton — wherever state_nft_1 currently lives IS the
    // live vault box. Same for reserve_nft_1. This recovers from a
    // previous run that already deposited/redeemed against the
    // original setup boxes.
    let pair = reconcile_pair_with_chain(&client, &pair);
    let token_by_name: BTreeMap<String, String> = deployment
        .tokens
        .into_iter()
        .map(|t| (t.name, t.token_id))
        .collect();
    let vyolo_id = token_by_name.get("VYOLO").expect("VYOLO").clone();
    let state_nft_1 = token_by_name
        .get("YOLO_VAULT_STATE_NFT_1")
        .expect("state_nft_1")
        .clone();
    let reserve_nft_1 = token_by_name
        .get("YOLO_VAULT_RESERVE_NFT_1")
        .expect("reserve_nft_1")
        .clone();

    let initial_vault_value = pair.vault.value;
    let initial_reserve_vyolo = pair
        .reserve
        .tokens
        .iter()
        .find(|t| t.name == "VYOLO")
        .map(|t| t.amount)
        .expect("reserve carries VYOLO");

    let funding = pick_plain_funding_box(&client, DEPOSIT_AMOUNT + TX_FEE + RECIPIENT_BOX_VALUE);
    eprintln!(
        "funding box {} value={}",
        &funding.box_id[..16],
        funding.value
    );

    // ============================================================
    // DEPOSIT
    // ============================================================
    let (post_deposit, vyolo_recipient_box_id) = run_half(
        &client,
        &change_address,
        &pair,
        &state_nft_1,
        &reserve_nft_1,
        &vyolo_id,
        &funding,
        true,
        DEPOSIT_AMOUNT,
        initial_reserve_vyolo,
        initial_vault_value,
    );
    eprintln!(
        "DEPOSIT ok — new vault {} new reserve {} recipient {}",
        &post_deposit.vault.box_id[..16],
        &post_deposit.reserve.box_id[..16],
        &vyolo_recipient_box_id[..16]
    );
    let vault_after_deposit = client
        .utxo_by_id(&post_deposit.vault.box_id)
        .unwrap()
        .expect("post-deposit vault box in utxo");
    let vault_value_after = vault_after_deposit
        .get("value")
        .and_then(|v| v.as_u64())
        .unwrap();
    assert_eq!(
        vault_value_after,
        initial_vault_value + DEPOSIT_AMOUNT,
        "deposit must increase vault value by DEPOSIT_AMOUNT"
    );

    // ============================================================
    // REDEEM (reverse)
    // ============================================================
    // Use the vYOLO recipient box minted by the deposit directly —
    // /wallet/boxes/unspent is paginated by box_id, so freshly-minted
    // boxes can sit on a later page than we'd reasonably scan.
    let vyolo_box = client
        .utxo_by_id(&vyolo_recipient_box_id)
        .unwrap()
        .expect("post-deposit vYOLO recipient still in utxo");
    let vyolo_funding = FundingBox {
        box_id: vyolo_recipient_box_id.clone(),
        value: vyolo_box.get("value").and_then(|v| v.as_u64()).unwrap(),
        token_amount: vyolo_box
            .get("assets")
            .and_then(|a| a.as_array())
            .and_then(|a| {
                a.iter().find_map(|asset| {
                    if asset.get("tokenId").and_then(|t| t.as_str())? == vyolo_id {
                        asset.get("amount").and_then(|t| t.as_u64())
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0),
    };
    eprintln!(
        "redeem funding box {} (vYOLO={})",
        &vyolo_funding.box_id[..16],
        vyolo_funding.token_amount
    );
    let (post_redeem, _) = run_half(
        &client,
        &change_address,
        &post_deposit,
        &state_nft_1,
        &reserve_nft_1,
        &vyolo_id,
        &vyolo_funding,
        false,
        DEPOSIT_AMOUNT,
        initial_reserve_vyolo - DEPOSIT_AMOUNT,
        initial_vault_value + DEPOSIT_AMOUNT,
    );
    eprintln!(
        "REDEEM ok — final vault {}",
        &post_redeem.vault.box_id[..16]
    );

    let final_vault = client
        .utxo_by_id(&post_redeem.vault.box_id)
        .unwrap()
        .expect("final vault box in utxo");
    let final_value = final_vault
        .get("value")
        .and_then(|v| v.as_u64())
        .unwrap();
    assert_eq!(
        final_value, initial_vault_value,
        "post-redeem vault value must match initial"
    );
}

// ---- run_half — shared build+submit+verify for deposit & redeem ----

#[allow(clippy::too_many_arguments)]
fn run_half(
    client: &NodeClient,
    change_address: &str,
    pair: &PairState,
    state_nft_id: &str,
    reserve_nft_id: &str,
    vyolo_id: &str,
    funding: &FundingBox,
    is_deposit: bool,
    amount: u64,
    pre_reserve_vyolo: u64,
    pre_vault_value: u64,
) -> (PairState, String) {
    let new_vault_value = if is_deposit {
        pre_vault_value + amount
    } else {
        pre_vault_value - amount
    };
    let new_reserve_vyolo = if is_deposit {
        pre_reserve_vyolo - amount
    } else {
        pre_reserve_vyolo + amount
    };

    // OUTPUTS(0): successor vault. vault.es requires OUTPUTS(0) is the
    // successor; the wallet emits output candidates in request order.
    let vault_request = PaymentRequestDto {
        address: pair.vault.p2s_address.clone(),
        value: new_vault_value,
        assets: vec![AssetDto {
            token_id: state_nft_id.to_string(),
            amount: 1,
        }],
    };

    // OUTPUTS(1): successor reserve.
    let reserve_request = PaymentRequestDto {
        address: pair.reserve.p2s_address.clone(),
        value: pair.reserve.value,
        assets: vec![
            AssetDto {
                token_id: reserve_nft_id.to_string(),
                amount: 1,
            },
            AssetDto {
                token_id: vyolo_id.to_string(),
                amount: new_reserve_vyolo,
            },
        ],
    };

    // OUTPUTS(2): user-facing recipient. On deposit gets `amount`
    // vYOLO; on redeem gets `amount` YOLO (no tokens).
    let recipient_request = if is_deposit {
        PaymentRequestDto {
            address: change_address.to_string(),
            value: RECIPIENT_BOX_VALUE,
            assets: vec![AssetDto {
                token_id: vyolo_id.to_string(),
                amount,
            }],
        }
    } else {
        PaymentRequestDto {
            address: change_address.to_string(),
            value: amount,
            assets: vec![],
        }
    };

    // The wallet auto-appends a fee output (TX_FEE) and a change
    // output (leftover ERG + leftover tokens at change_address). For
    // the redeem the funding box carries excess vYOLO; the wallet
    // will roll it into the change box automatically.
    let inputs = vec![
        pair.vault.box_id.clone(),
        pair.reserve.box_id.clone(),
        funding.box_id.clone(),
    ];

    let tx_id = client
        .wallet_transaction_send_with_inputs(
            &[vault_request, reserve_request, recipient_request],
            &inputs,
            Some(TX_FEE),
        )
        .expect("send tx");
    eprintln!("submitted tx_id={tx_id}");
    let tx_body = client.wait_for_tx(&tx_id).expect("wait_for_tx");
    let outputs = tx_body
        .get("outputs")
        .and_then(|v| v.as_array())
        .expect("outputs array");

    let new_vault =
        find_output_with_token(outputs, state_nft_id).expect("new vault output");
    let new_reserve =
        find_output_with_token(outputs, reserve_nft_id).expect("new reserve output");
    // OUTPUTS(2) per the request order in this function = the
    // user-facing recipient. On deposit it carries vYOLO; on redeem
    // it carries plain YOLO. The caller may need its box_id to fund
    // the next half.
    let recipient_box_id = outputs
        .get(2)
        .and_then(|o| o.get("boxId").and_then(|b| b.as_str()))
        .unwrap_or("")
        .to_string();

    (PairState {
        pair: pair.pair,
        vault: BoxRef {
            box_id: new_vault.0,
            value: new_vault.1,
            p2s_address: pair.vault.p2s_address.clone(),
            tokens: vec![TokenRef {
                name: "YOLO_VAULT_STATE_NFT_1".to_string(),
                token_id: state_nft_id.to_string(),
                amount: 1,
            }],
        },
        reserve: BoxRef {
            box_id: new_reserve.0,
            value: new_reserve.1,
            p2s_address: pair.reserve.p2s_address.clone(),
            tokens: vec![
                TokenRef {
                    name: "YOLO_VAULT_RESERVE_NFT_1".to_string(),
                    token_id: reserve_nft_id.to_string(),
                    amount: 1,
                },
                TokenRef {
                    name: "VYOLO".to_string(),
                    token_id: vyolo_id.to_string(),
                    amount: new_reserve_vyolo,
                },
            ],
        },
    }, recipient_box_id)
}

// ---- helpers ----

#[derive(Debug)]
struct FundingBox {
    box_id: String,
    value: u64,
    token_amount: u64,
}

fn pick_plain_funding_box(client: &NodeClient, min_value: u64) -> FundingBox {
    let page = client
        .wallet_boxes_unspent(0, 200)
        .expect("/wallet/boxes/unspent");
    for b in &page.items {
        let utxo = client
            .utxo_by_id(&b.box_id)
            .ok()
            .flatten()
            .expect("utxo lookup");
        let has_tokens = utxo
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        if !has_tokens && b.value >= min_value {
            return FundingBox {
                box_id: b.box_id.clone(),
                value: b.value,
                token_amount: 0,
            };
        }
    }
    panic!("no plain funding box >= {min_value} found");
}

fn pick_box_with_token(client: &NodeClient, token_id: &str, min_amount: u64) -> FundingBox {
    let page = client
        .wallet_boxes_unspent(0, 200)
        .expect("/wallet/boxes/unspent");
    for b in &page.items {
        let utxo = client
            .utxo_by_id(&b.box_id)
            .ok()
            .flatten()
            .expect("utxo lookup");
        let assets = utxo
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|a| a.clone())
            .unwrap_or_default();
        for a in &assets {
            let tid = a.get("tokenId").and_then(|t| t.as_str()).unwrap_or("");
            let amt = a.get("amount").and_then(|t| t.as_u64()).unwrap_or(0);
            if tid == token_id && amt >= min_amount {
                return FundingBox {
                    box_id: b.box_id.clone(),
                    value: b.value,
                    token_amount: amt,
                };
            }
        }
    }
    panic!("no funding box with token {} amount >= {min_amount}", token_id);
}

fn find_output_with_token(
    outputs: &[serde_json::Value],
    token_id: &str,
) -> Option<(String, u64)> {
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
        let bid = o.get("boxId").and_then(|b| b.as_str()).map(|s| s.to_string())?;
        let v = o.get("value").and_then(|v| v.as_u64())?;
        Some((bid, v))
    })
}

fn reconcile_pair_with_chain(client: &NodeClient, original: &PairState) -> PairState {
    let state_nft = &original.vault.tokens[0].token_id;
    let reserve_nft = &original
        .reserve
        .tokens
        .iter()
        .find(|t| t.name == "YOLO_VAULT_RESERVE_NFT_1")
        .unwrap()
        .token_id;
    let vyolo_id = &original
        .reserve
        .tokens
        .iter()
        .find(|t| t.name == "VYOLO")
        .unwrap()
        .token_id;

    let vault_live = lookup_unspent_box_by_token(client, state_nft)
        .expect("indexer must locate the singleton state_nft box");
    let reserve_live = lookup_unspent_box_by_token(client, reserve_nft)
        .expect("indexer must locate the singleton reserve_nft box");
    let reserve_vyolo_amount = reserve_live
        .assets
        .iter()
        .find_map(|(tid, amt)| if tid == vyolo_id { Some(*amt) } else { None })
        .expect("reserve box must carry VYOLO");

    PairState {
        pair: original.pair,
        vault: BoxRef {
            box_id: vault_live.box_id,
            value: vault_live.value,
            p2s_address: original.vault.p2s_address.clone(),
            tokens: vec![TokenRef {
                name: "YOLO_VAULT_STATE_NFT_1".to_string(),
                token_id: state_nft.clone(),
                amount: 1,
            }],
        },
        reserve: BoxRef {
            box_id: reserve_live.box_id,
            value: reserve_live.value,
            p2s_address: original.reserve.p2s_address.clone(),
            tokens: vec![
                TokenRef {
                    name: "YOLO_VAULT_RESERVE_NFT_1".to_string(),
                    token_id: reserve_nft.clone(),
                    amount: 1,
                },
                TokenRef {
                    name: "VYOLO".to_string(),
                    token_id: vyolo_id.clone(),
                    amount: reserve_vyolo_amount,
                },
            ],
        },
    }
}

struct LiveBox {
    box_id: String,
    value: u64,
    assets: Vec<(String, u64)>,
}

fn lookup_unspent_box_by_token(client: &NodeClient, token_id: &str) -> Option<LiveBox> {
    // Indexer route #20 — returns a bare list of unspent
    // IndexedErgoBox carrying the requested token.
    let path = format!("/blockchain/box/unspent/byTokenId/{}", token_id);
    let resp: serde_json::Value = client
        .raw_get_json_auth(&path)
        .expect("/blockchain/box/unspent/byTokenId");
    let items = resp.as_array()?;
    let item = items.first()?;
    Some(LiveBox {
        box_id: item.get("boxId")?.as_str()?.to_string(),
        value: item.get("value")?.as_u64()?,
        assets: item
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|asset| {
                        Some((
                            asset.get("tokenId")?.as_str()?.to_string(),
                            asset.get("amount")?.as_u64()?,
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn load_pair_state() -> PairState {
    let p = test_vectors_dir().join("sigmachain-pair-1-state.json");
    let bytes = std::fs::read(&p).expect("read pair-1-state.json");
    serde_json::from_slice(&bytes).expect("parse pair-1-state.json")
}

fn load_deployment() -> Deployment {
    let p = test_vectors_dir().join("sigmachain-deployment.json");
    let bytes = std::fs::read(&p).expect("read deployment.json");
    serde_json::from_slice(&bytes).expect("parse deployment.json")
}

fn test_vectors_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("test-vectors");
    p
}
