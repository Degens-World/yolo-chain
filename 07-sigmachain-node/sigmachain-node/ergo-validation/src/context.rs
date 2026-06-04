use ergo_primitives::digest::Digest32;
use ergo_ser::ergo_box::ErgoBox;

/// Snapshot of protocol parameters at a given epoch.
/// These change at epoch boundaries via soft-fork voting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolParams {
    /// Minimum nanoErg per byte of serialized box. Default: 360.
    pub min_value_per_byte: u64,
    /// Maximum cumulative cost for all scripts in a block. Default: 1,000,000.
    pub max_block_cost: u64,
    /// Maximum serialized `BlockTransactions` section size in bytes
    /// (voted parameter — Scala `maxBlockSize`). Default: 524_288.
    /// Used by rule 306 (`bsBlockTransactionsSize`).
    pub max_block_size: u32,
    /// Maximum serialized box size in bytes. Protocol-level: 4096.
    pub max_box_size: u32,
    /// Maximum tokens per box. Protocol-level: 122.
    pub max_tokens_per_box: u8,
    /// Cost per transaction input (votable). Default: 2,000.
    pub input_cost: u64,
    /// Cost per data input (votable). Default: 100.
    pub data_input_cost: u64,
    /// Cost per output candidate (votable). Default: 100.
    pub output_cost: u64,
    /// Cost per token access entry (votable). Default: 100.
    pub token_access_cost: u64,
    /// Storage fee factor: nanoErg per byte per storage period. Default: 1,250,000.
    /// Votable parameter. Used in storage rent calculation.
    pub storage_fee_factor: i32,
    /// Storage period in blocks (4 years). Default: 1,051,200.
    pub storage_period: u32,
}

impl ProtocolParams {
    /// Mainnet defaults — frozen snapshot of the votable parameters at
    /// the time these were captured. The authoritative path is to derive
    /// `ProtocolParams` from a per-epoch [`crate::ActiveProtocolParameters`] via
    /// [`Self::from_active`]; this constructor is a fallback when the
    /// per-epoch active set is not yet available.
    pub fn mainnet_default() -> Self {
        Self {
            min_value_per_byte: 360,
            // Mainnet value from blockchain parameters (adjusted via voting).
            // The authoritative source is the extension section of the
            // first block of each epoch — see `from_active` below.
            max_block_cost: 8_001_091,
            max_block_size: 524_288,
            max_box_size: 4096,
            max_tokens_per_box: 122,
            input_cost: 2_000,
            data_input_cost: 100,
            output_cost: 100,
            token_access_cost: 100,
            storage_fee_factor: 1_250_000,
            storage_period: 1_051_200,
        }
    }

    /// SigmaChain testnet defaults. Differences from `mainnet_default`:
    ///
    /// - `storage_period = 1_577_880` blocks (1 year at 20 s blocks),
    ///   down from Ergo's 1_051_200 (4 years at 120 s). Per the YOLO
    ///   storage rent design — boxes become rent-eligible after 1
    ///   year of idleness rather than 4.
    /// - `storage_fee_factor = 312_500` nanoYOLO per byte per
    ///   storage_period, exactly 1/4 of Ergo's 1_250_000. Combined
    ///   with the 1/4 shorter cycle, the *annualized* storage cost
    ///   per byte is identical to Ergo's — only the collection
    ///   cadence speeds up.
    ///
    /// The miner-only consensus rule for storage rent claims (every
    /// non-coinbase rent claim must be rejected at block validation)
    /// is NOT yet wired here — that requires a block-level check
    /// outside per-tx validation. Phase 4 lands it together with the
    /// 4-output coinbase builder.
    pub fn sigmachain_testnet_default() -> Self {
        Self {
            min_value_per_byte: 360,
            max_block_cost: 8_001_091,
            max_block_size: 524_288,
            max_box_size: 4096,
            max_tokens_per_box: 122,
            input_cost: 2_000,
            data_input_cost: 100,
            output_cost: 100,
            token_access_cost: 100,
            storage_fee_factor: 312_500,
            storage_period: 1_577_880,
        }
    }

    /// Convert from the per-epoch active set persisted in `voted_params`.
    /// Total and infallible: the parser / persisted-codec rejects
    /// out-of-range values up front, so by the time we reach
    /// `from_active` every cost-bearing field is already guaranteed
    /// `>= 0`. Widening `i32 → u64` is a pure type-level cast.
    ///
    /// The non-votable protocol constants (`max_box_size`,
    /// `max_tokens_per_box`, `storage_period`) are pulled from
    /// `mainnet_default`.
    pub fn from_active(active: &crate::active_params::ActiveProtocolParameters) -> Self {
        // Negativity guard lives at the parse / persisted-codec boundary
        // (`active_params::parse_active_params` and
        // `ActiveProtocolParameters::deserialize`); see
        // `ActiveParamsError::NegativeCostBearingParam`. Any negative
        // value reaching this constructor is a violated parser
        // invariant — `debug_assert!` surfaces the bug in tests without
        // taking down a production node, where the downstream
        // cost-parity test would still detect the resulting drift.
        debug_assert!(
            active.min_value_per_byte >= 0,
            "negative min_value_per_byte leaked past parse boundary"
        );
        debug_assert!(
            active.max_block_cost >= 0,
            "negative max_block_cost leaked past parse boundary"
        );
        debug_assert!(
            active.max_block_size >= 0,
            "negative max_block_size leaked past parse boundary"
        );
        debug_assert!(
            active.input_cost >= 0,
            "negative input_cost leaked past parse boundary"
        );
        debug_assert!(
            active.data_input_cost >= 0,
            "negative data_input_cost leaked past parse boundary"
        );
        debug_assert!(
            active.output_cost >= 0,
            "negative output_cost leaked past parse boundary"
        );
        debug_assert!(
            active.token_access_cost >= 0,
            "negative token_access_cost leaked past parse boundary"
        );
        Self {
            min_value_per_byte: active.min_value_per_byte as u64,
            max_block_cost: active.max_block_cost as u64,
            max_block_size: active.max_block_size as u32,
            max_box_size: 4096,
            max_tokens_per_box: 122,
            input_cost: active.input_cost as u64,
            data_input_cost: active.data_input_cost as u64,
            output_cost: active.output_cost as u64,
            token_access_cost: active.token_access_cost as u64,
            storage_fee_factor: active.storage_fee_factor,
            storage_period: 1_051_200,
        }
    }

    /// Network-aware variant of [`Self::from_active`]. Differs from the
    /// vanilla `from_active` only in `storage_period`, which is the
    /// non-votable protocol constant that diverges between Ergo and
    /// SigmaChain (4 years vs. 1 year of block-count). Production
    /// block-validation paths that know which chain they're on should
    /// prefer this entry point.
    pub fn from_active_for_network(
        active: &crate::active_params::ActiveProtocolParameters,
        network: ergo_chain_spec::Network,
    ) -> Self {
        let mut p = Self::from_active(active);
        p.storage_period = match network {
            ergo_chain_spec::Network::Mainnet | ergo_chain_spec::Network::Testnet => 1_051_200,
            ergo_chain_spec::Network::SigmaChainTestnet => 1_577_880,
        };
        p
    }
}

/// Local node policy limits (not consensus rules).
/// A transaction violating these is rejected by this node but may be
/// valid on the network.
pub struct LocalPolicy {
    /// Maximum transaction byte size this node will accept.
    pub max_transaction_size: usize,
}

impl LocalPolicy {
    /// Default policy — 512 KiB max transaction size (matches the
    /// default max block size). Tightening below this is safe; raising
    /// it accepts transactions other nodes will silently drop.
    pub fn default_policy() -> Self {
        Self {
            max_transaction_size: 524_288, // 512KB — same as default max block size
        }
    }
}

/// Minimal trait for UTXO lookup during transaction validation.
///
/// Returns owned `ErgoBox` to avoid leaking storage lifetimes into the
/// validation layer. The extra clone cost is acceptable.
pub trait UtxoView {
    /// Look up an unspent box by id. Returns `None` when the box is
    /// not present in the active UTXO set.
    fn get_box(&self, box_id: &Digest32) -> Option<ErgoBox>;
}

/// Block/chain context needed for transaction validation.
///
/// Per-input context extension values come from `Input.spending_proof.extension`,
/// not from this struct — they are threaded per-input in script validation.
pub struct TransactionContext {
    /// Current block height being validated.
    pub height: u32,
    /// Miner public key (33-byte compressed SEC1) from the block being validated.
    pub miner_pubkey: [u8; 33],
    /// Block timestamp (milliseconds since epoch) from the pre-header.
    pub pre_header_timestamp: u64,
    /// Activated script version = header.version - 1.
    /// Controls consensus-preserving behavior (e.g., selfBoxIndex bug in v4.x).
    pub activated_script_version: u8,
    /// Block version byte (from header).
    pub pre_header_version: u8,
    /// Parent block header ID (32 bytes).
    pub pre_header_parent_id: [u8; 32],
    /// Encoded difficulty (nBits from header).
    pub pre_header_n_bits: u64,
    /// Miner votes (3 bytes from header).
    pub pre_header_votes: [u8; 3],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::active_params::ActiveProtocolParameters;

    fn baseline_active() -> ActiveProtocolParameters {
        ActiveProtocolParameters {
            epoch_start_height: 1024,
            block_version: 1,
            storage_fee_factor: 1_250_000,
            min_value_per_byte: 360,
            max_block_size: 524_288,
            max_block_cost: 1_000_000,
            token_access_cost: 100,
            input_cost: 2_000,
            data_input_cost: 100,
            output_cost: 100,
            subblocks_per_block: None,
            extra: vec![],
            proposed_update:
                crate::voting::validation_settings::ErgoValidationSettingsUpdate::empty(),
            activated_update:
                crate::voting::validation_settings::ErgoValidationSettingsUpdate::empty(),
        }
    }

    // ----- happy path -----

    #[test]
    fn from_active_accepts_baseline_voted_params() {
        let p = ProtocolParams::from_active(&baseline_active());
        assert_eq!(p.input_cost, 2_000);
        assert_eq!(p.max_block_cost, 1_000_000);
        assert_eq!(p.min_value_per_byte, 360);
    }

    // ----- SigmaChain storage rent (Phase 3.4) -----

    #[test]
    fn sigmachain_testnet_default_storage_period_is_one_year_at_20s() {
        // 1_577_880 blocks × 20 s = 31_557_600 s = exactly 365.25 days
        // (Julian year), matching emission.es:37 "blocksPerHalving:
        // Int = 1577880 ~1 year at 20s blocks". The storage rent
        // cycle locks to the same cadence as halvings.
        let p = ProtocolParams::sigmachain_testnet_default();
        assert_eq!(p.storage_period, 1_577_880);
        let seconds_per_period = (p.storage_period as u64) * 20;
        let seconds_per_julian_year = 365 * 86_400 + 86_400 / 4; // 31_557_600
        assert_eq!(seconds_per_period, seconds_per_julian_year);
    }

    #[test]
    fn sigmachain_testnet_default_storage_fee_factor_matches_ergo_annualized() {
        // Ergo: 1_250_000 nano / byte per 4-year cycle = 312_500
        // nano / byte / year. SigmaChain: 312_500 nano / byte per
        // 1-year cycle. Annualized per-byte cost is identical; only
        // the collection cadence speeds up.
        let p = ProtocolParams::sigmachain_testnet_default();
        let ergo = ProtocolParams::mainnet_default();
        assert_eq!(p.storage_fee_factor, 312_500);
        assert_eq!(ergo.storage_fee_factor, 1_250_000);
        let ergo_annualized = ergo.storage_fee_factor / 4;
        assert_eq!(p.storage_fee_factor, ergo_annualized);
    }

    #[test]
    fn sigmachain_testnet_default_keeps_protocol_constants() {
        // Non-storage-rent fields should match mainnet defaults — this
        // chunk only changes storage_period + storage_fee_factor.
        let p = ProtocolParams::sigmachain_testnet_default();
        let m = ProtocolParams::mainnet_default();
        assert_eq!(p.min_value_per_byte, m.min_value_per_byte);
        assert_eq!(p.max_block_cost, m.max_block_cost);
        assert_eq!(p.max_block_size, m.max_block_size);
        assert_eq!(p.max_box_size, m.max_box_size);
        assert_eq!(p.max_tokens_per_box, m.max_tokens_per_box);
        assert_eq!(p.input_cost, m.input_cost);
        assert_eq!(p.data_input_cost, m.data_input_cost);
        assert_eq!(p.output_cost, m.output_cost);
        assert_eq!(p.token_access_cost, m.token_access_cost);
    }

    #[test]
    fn from_active_for_network_picks_right_storage_period() {
        use ergo_chain_spec::Network;
        let active = baseline_active();
        // Mainnet / testnet use Ergo's 4-year period.
        let m = ProtocolParams::from_active_for_network(&active, Network::Mainnet);
        let t = ProtocolParams::from_active_for_network(&active, Network::Testnet);
        assert_eq!(m.storage_period, 1_051_200);
        assert_eq!(t.storage_period, 1_051_200);
        // SigmaChain swaps to the 1-year period.
        let s = ProtocolParams::from_active_for_network(&active, Network::SigmaChainTestnet);
        assert_eq!(s.storage_period, 1_577_880);
    }

    #[test]
    fn from_active_for_network_preserves_voted_storage_fee_factor() {
        use ergo_chain_spec::Network;
        // storage_fee_factor is a voted parameter — from_active reads
        // it from the active set, not from the per-network default.
        // For SigmaChain, the initial active set will carry 312_500
        // (see Phase 4 genesis active params); for the test we pass a
        // baseline with the Ergo value to confirm the voted value
        // wins.
        let active = baseline_active();
        assert_eq!(active.storage_fee_factor, 1_250_000);
        let s = ProtocolParams::from_active_for_network(&active, Network::SigmaChainTestnet);
        assert_eq!(s.storage_fee_factor, 1_250_000);
    }
}
