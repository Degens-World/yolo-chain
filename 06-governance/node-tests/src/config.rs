use std::env;
use std::fmt;
use std::time::Duration;

/// Connection + secret material for the live SigmaChain node under test.
///
/// Secrets (`api_key`, `wallet_password`) are read from environment
/// variables. They are NEVER printed via Debug or Display — the manual
/// Debug impl below redacts them.
pub struct TestConfig {
    pub base_url: String,
    pub api_key: String,
    pub wallet_password: String,
    pub poll_interval: Duration,
    pub block_wait_timeout: Duration,
    pub tx_wait_timeout: Duration,
}

impl TestConfig {
    /// Build a config from environment variables. Required env vars:
    ///   YOLO_NODE_URL            (default: http://127.0.0.1:9054)
    ///   YOLO_NODE_API_KEY        (required)
    ///   YOLO_WALLET_PASSWORD     (required)
    /// Optional:
    ///   YOLO_POLL_MS             (default: 500)
    ///   YOLO_BLOCK_TIMEOUT_MS    (default: 60_000)
    ///   YOLO_TX_TIMEOUT_MS       (default: 30_000)
    pub fn from_env() -> Result<Self, String> {
        let base_url = env::var("YOLO_NODE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:9054".to_string());
        let api_key = env::var("YOLO_NODE_API_KEY")
            .map_err(|_| "YOLO_NODE_API_KEY env var is required".to_string())?;
        let wallet_password = env::var("YOLO_WALLET_PASSWORD")
            .map_err(|_| "YOLO_WALLET_PASSWORD env var is required".to_string())?;
        let poll_ms: u64 = env::var("YOLO_POLL_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(500);
        let block_timeout_ms: u64 = env::var("YOLO_BLOCK_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60_000);
        let tx_timeout_ms: u64 = env::var("YOLO_TX_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30_000);

        Ok(Self {
            base_url,
            api_key,
            wallet_password,
            poll_interval: Duration::from_millis(poll_ms),
            block_wait_timeout: Duration::from_millis(block_timeout_ms),
            tx_wait_timeout: Duration::from_millis(tx_timeout_ms),
        })
    }
}

impl fmt::Debug for TestConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("wallet_password", &"<redacted>")
            .field("poll_interval", &self.poll_interval)
            .field("block_wait_timeout", &self.block_wait_timeout)
            .field("tx_wait_timeout", &self.tx_wait_timeout)
            .finish()
    }
}
