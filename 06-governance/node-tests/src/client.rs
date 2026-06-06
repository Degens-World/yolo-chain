//! ureq-blocking wrapper around the SigmaChain node's HTTP API.
//!
//! Scope: the routes Phase 5.4 actually drives. Wallet lifecycle,
//! balance/status reads, payment sending, raw tx submit, /info,
//! and /utxo/byId. Everything else stays an explicit miss.
//!
//! The `api_key` header value comes from `TestConfig` and is never
//! logged. All wire DTOs use the Scala camelCase shape via serde rename.

use serde::{Deserialize, Serialize};
use std::thread::sleep;
use std::time::Instant;
use ureq::{Agent, AgentBuilder};

use crate::config::TestConfig;
use crate::error::{NodeError, Result};

const API_KEY_HEADER: &str = "api_key";

/// Thin blocking HTTP client. One agent reused across all calls so
/// keep-alive amortizes the wallet/utxo polling loops.
pub struct NodeClient {
    agent: Agent,
    base: String,
    api_key: String,
    cfg: TestConfig,
}

impl NodeClient {
    pub fn new(cfg: TestConfig) -> Self {
        let agent = AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(5))
            .timeout_read(std::time::Duration::from_secs(30))
            .build();
        let base = cfg.base_url.trim_end_matches('/').to_string();
        let api_key = cfg.api_key.clone();
        Self {
            agent,
            base,
            api_key,
            cfg,
        }
    }

    pub fn cfg(&self) -> &TestConfig {
        &self.cfg
    }

    // ---- low-level helpers ------------------------------------------------

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn handle_response(call: std::result::Result<ureq::Response, ureq::Error>) -> Result<ureq::Response> {
        match call {
            Ok(r) => Ok(r),
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                Err(NodeError::BadStatus { status, body })
            }
            Err(e) => Err(NodeError::Transport(e)),
        }
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let req = self.agent.get(&self.url(path));
        let resp = Self::handle_response(req.call())?;
        Ok(resp.into_json::<T>()?)
    }

    fn get_json_auth<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let req = self
            .agent
            .get(&self.url(path))
            .set(API_KEY_HEADER, &self.api_key);
        let resp = Self::handle_response(req.call())?;
        Ok(resp.into_json::<T>()?)
    }

    /// Authenticated GET returning a raw `serde_json::Value`. Used by
    /// the YoloDAO tests where the indexer response shape is too rich
    /// to model exhaustively; the tests pick the fields they need.
    pub fn raw_get_json_auth(&self, path: &str) -> Result<serde_json::Value> {
        self.get_json_auth(path)
    }

    fn post_json_auth<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let req = self
            .agent
            .post(&self.url(path))
            .set(API_KEY_HEADER, &self.api_key);
        let resp = Self::handle_response(req.send_json(serde_json::to_value(body)?))?;
        Ok(resp.into_json::<T>()?)
    }

    fn post_json_auth_status<B: Serialize>(&self, path: &str, body: &B) -> Result<u16> {
        let req = self
            .agent
            .post(&self.url(path))
            .set(API_KEY_HEADER, &self.api_key);
        let resp = Self::handle_response(req.send_json(serde_json::to_value(body)?))?;
        Ok(resp.status())
    }

    /// Returns `Ok(Some(t))` on 200, `Ok(None)` on 404, `Err` otherwise.
    /// Used for "does this box/tx exist yet" polling. `with_auth`
    /// attaches the `api_key` header; some node routes (the indexer
    /// `/blockchain/*` family in particular) gate on it.
    fn get_json_optional<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        with_auth: bool,
    ) -> Result<Option<T>> {
        let mut req = self.agent.get(&self.url(path));
        if with_auth {
            req = req.set(API_KEY_HEADER, &self.api_key);
        }
        match req.call() {
            Ok(resp) => Ok(Some(resp.into_json::<T>()?)),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                Err(NodeError::BadStatus { status, body })
            }
            Err(e) => Err(NodeError::Transport(e)),
        }
    }

    // ---- /info ------------------------------------------------------------

    /// GET /info — current chain tip.
    pub fn info(&self) -> Result<NodeInfo> {
        self.get_json("/info")
    }

    /// Best-effort fullHeight read; 0 when the field is missing.
    pub fn full_height(&self) -> Result<u32> {
        Ok(self.info()?.full_height.unwrap_or(0))
    }

    /// Block until `full_height >= target`, polling at `cfg.poll_interval`.
    /// Fails with `NodeError::Timeout` after `cfg.block_wait_timeout`.
    pub fn wait_for_height(&self, target: u32) -> Result<u32> {
        let started = Instant::now();
        loop {
            let h = self.full_height()?;
            if h >= target {
                return Ok(h);
            }
            if started.elapsed() > self.cfg.block_wait_timeout {
                return Err(NodeError::Timeout {
                    what: format!("height >= {} (saw {})", target, h),
                    waited_ms: started.elapsed().as_millis() as u64,
                });
            }
            sleep(self.cfg.poll_interval);
        }
    }

    // ---- /wallet ----------------------------------------------------------

    /// GET /wallet/status
    pub fn wallet_status(&self) -> Result<WalletStatus> {
        self.get_json_auth("/wallet/status")
    }

    /// POST /wallet/unlock — body: { "pass": "..." }. 200 on success.
    pub fn wallet_unlock(&self) -> Result<()> {
        let body = UnlockBody {
            pass: self.cfg.wallet_password.clone(),
        };
        let status = self.post_json_auth_status("/wallet/unlock", &body)?;
        if status == 200 {
            Ok(())
        } else {
            Err(NodeError::BadStatus {
                status,
                body: "unexpected status from /wallet/unlock".into(),
            })
        }
    }

    /// GET /wallet/balances — confirmed balance only.
    pub fn wallet_balances(&self) -> Result<WalletBalances> {
        self.get_json_auth("/wallet/balances")
    }

    /// POST /wallet/transaction/send — build + sign + submit a payment.
    /// `requests` is a list of payment targets (address, value, assets).
    pub fn wallet_transaction_send(&self, requests: &[PaymentRequestDto]) -> Result<String> {
        // The route accepts a TransactionSendRequest wrapping a Vec; the
        // node also accepts a bare list (the Scala compat shape). We send
        // the explicit wrapper to be safe.
        let body = TransactionSendRequest {
            requests: requests.to_vec(),
            inputs: None,
            data_inputs: None,
            fee: None,
        };
        let resp: TxIdResponse = self.post_json_auth("/wallet/transaction/send", &body)?;
        Ok(resp.tx_id)
    }

    /// POST /wallet/transaction/send with explicit input override and
    /// optional fee override. Phase 5.4.2 uses this for genesis token
    /// mints: the caller selects a known unspent box as `inputs[0]`,
    /// then references `box_id` as the token id in the payment's
    /// assets list — the wallet bridge allows this through the
    /// override-inputs minting path.
    pub fn wallet_transaction_send_with_inputs(
        &self,
        requests: &[PaymentRequestDto],
        inputs: &[String],
        fee: Option<u64>,
    ) -> Result<String> {
        let body = TransactionSendRequest {
            requests: requests.to_vec(),
            inputs: Some(inputs.to_vec()),
            data_inputs: None,
            fee,
        };
        let resp: TxIdResponse = self.post_json_auth("/wallet/transaction/send", &body)?;
        Ok(resp.tx_id)
    }

    /// GET /wallet/boxes/unspent — paged list of the wallet's unspent
    /// boxes. The default page size is small (50); the bake helper
    /// asks for `limit=200` to land enough fresh coinbase boxes in
    /// one round-trip.
    pub fn wallet_boxes_unspent(&self, offset: u32, limit: u32) -> Result<WalletBoxesPage> {
        self.get_json_auth(&format!(
            "/wallet/boxes/unspent?offset={}&limit={}",
            offset, limit
        ))
    }

    /// POST /wallet/transaction/sign — sign a caller-supplied unsigned
    /// transaction with the wallet's secrets. Used by the deposit /
    /// redeem tests where the tx is constructed in-process (mixing
    /// wallet-owned inputs with contract boxes whose script reduces
    /// to a trivial proof). Returns the signed tx as hex bytes.
    pub fn wallet_transaction_sign(
        &self,
        unsigned_tx_hex: &str,
        inputs: Option<&[String]>,
    ) -> Result<String> {
        let body = serde_json::json!({
            "unsignedTx": { "bytes": unsigned_tx_hex },
            "inputs": inputs,
        });
        let resp: serde_json::Value = self.post_json_auth("/wallet/transaction/sign", &body)?;
        resp.get("transaction")
            .and_then(|t| t.get("bytes"))
            .and_then(|b| b.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| NodeError::Invariant("sign response missing transaction.bytes".into()))
    }

    /// POST /transactions/bytes — submit a hex-encoded signed
    /// transaction directly to the mempool. Returns the assigned tx
    /// id (32-byte hex).
    pub fn submit_signed_tx_bytes(&self, signed_tx_hex: &str) -> Result<String> {
        let body = serde_json::Value::String(signed_tx_hex.to_string());
        let resp: serde_json::Value = self.post_json_auth("/transactions/bytes", &body)?;
        // /transactions/bytes returns the tx id as a bare JSON string
        // (or sometimes wrapped). Handle both.
        if let Some(s) = resp.as_str() {
            return Ok(s.to_string());
        }
        if let Some(s) = resp.get("txId").and_then(|t| t.as_str()) {
            return Ok(s.to_string());
        }
        Err(NodeError::Invariant(format!(
            "/transactions/bytes returned unexpected shape: {}",
            resp
        )))
    }

    // ---- /transactions ----------------------------------------------------

    /// GET /blockchain/transaction/byId/{tx_id} — indexer-backed
    /// confirmed-tx lookup. `Ok(None)` when the tx is not yet mined.
    /// This route is api_key-gated on the SigmaChain node build.
    pub fn transaction_by_id(&self, tx_id: &str) -> Result<Option<serde_json::Value>> {
        self.get_json_optional(&format!("/blockchain/transaction/byId/{}", tx_id), true)
    }

    /// Poll /transactions/byId until the tx appears or `cfg.tx_wait_timeout`
    /// elapses. Returns the tx body on success.
    pub fn wait_for_tx(&self, tx_id: &str) -> Result<serde_json::Value> {
        let started = Instant::now();
        loop {
            if let Some(tx) = self.transaction_by_id(tx_id)? {
                return Ok(tx);
            }
            if started.elapsed() > self.cfg.tx_wait_timeout {
                return Err(NodeError::Timeout {
                    what: format!("tx {} mined", tx_id),
                    waited_ms: started.elapsed().as_millis() as u64,
                });
            }
            sleep(self.cfg.poll_interval);
        }
    }

    // ---- /utxo ------------------------------------------------------------

    /// GET /utxo/byId/{box_id}. `Ok(None)` on 404 (box not in current utxo
    /// set — either never existed or already spent). Unauthenticated.
    pub fn utxo_by_id(&self, box_id: &str) -> Result<Option<serde_json::Value>> {
        self.get_json_optional(&format!("/utxo/byId/{}", box_id), false)
    }
}

// ---- wire DTOs (mirror sigmachain-node ergo-api wallet types) -----------

/// /info response, narrowed to fields Phase 5.4 reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInfo {
    pub full_height: Option<u32>,
    pub headers_height: Option<u32>,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WalletStatus {
    pub is_initialized: bool,
    pub is_unlocked: bool,
    pub change_address: String,
    pub wallet_height: u32,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WalletBalances {
    pub height: u32,
    pub balance: u64,
    #[serde(default)]
    pub assets: Vec<TokenBalance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenBalance {
    pub token_id: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WalletBoxesPage {
    pub total: u32,
    pub items: Vec<WalletBoxEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletBoxEntry {
    pub box_id: String,
    pub value: u64,
    pub creation_height: u32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub provenance: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxIdResponse {
    pub tx_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequestDto {
    pub address: String,
    pub value: u64,
    #[serde(default)]
    pub assets: Vec<AssetDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDto {
    pub token_id: String,
    pub amount: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransactionSendRequest {
    requests: Vec<PaymentRequestDto>,
    inputs: Option<Vec<String>>,
    data_inputs: Option<Vec<String>>,
    fee: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UnlockBody {
    pass: String,
}
