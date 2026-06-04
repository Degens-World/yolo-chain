//! Network identity and chain parameters for Ergo.
//!
//! Single source of truth for "what does network X look like" — magic
//! bytes, address prefix, difficulty schedule, voting epoch lengths,
//! monetary curve, reemission constants, genesis state. The top-level
//! [`ChainSpec`] aggregates narrow parameter types and is built from a
//! [`Network`] via `ChainSpec::for_network`.
//!
//! Consumers downstream borrow narrow views (`&DifficultyParams`,
//! `&VotingParams`, etc.) rather than the whole spec. The
//! `Network::Mainnet`/`Network::Testnet` discrimination only lives in
//! the constructor site.
//!
//! Charter: types, constants, constructors only. No validation logic,
//! no I/O, no runtime services, no broad node state.
//!
//! Scala provenance: mainnet values reference `ergoplatform/ergo` at
//! commit `2cdbb8c` (v6.0.2, "Merge pull request #2236"). Testnet
//! values track upstream `v6.0.3` after PR #2252 ("New public testnet
//! parameters", merged 2026-02-26), which retired the previous public
//! testnet and re-pinned magic bytes, P2P port, checkpoint, voting
//! state, and EIP-27 reemission. Whenever either reference moves,
//! re-extract vectors under `test-vectors/` and re-verify the
//! byte-equality tests at the bottom of this file.

use std::net::SocketAddr;

use ergo_primitives::digest::Digest32;
use ergo_ser::address::NetworkPrefix;

fn parse_id_hex(s: &str) -> Digest32 {
    Digest32::from_bytes(parse_bytes32_hex(s))
}

fn parse_bytes32_hex(s: &str) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    hex::decode_to_slice(s, &mut bytes).expect("hardcoded hex must be valid");
    bytes
}

fn parse_digest33_hex(s: &str) -> [u8; 33] {
    let mut bytes = [0u8; 33];
    hex::decode_to_slice(s, &mut bytes).expect("hardcoded hex must be valid");
    bytes
}

/// Network selector. Discriminates only at construction time (via the
/// constructors on parameter types below); downstream consumers take
/// narrow params and never branch on this.
///
/// The first two variants are upstream Ergo. `SigmaChainTestnet` is
/// the YOLO/SigmaChain fork's network: see the SigmaChain parameter
/// inventory in `overall-documents/sigmachain-parameter-inventory.md`
/// for the per-variant divergences from Ergo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Network {
    /// Ergo public mainnet.
    Mainnet,
    /// Ergo public testnet.
    Testnet,
    /// SigmaChain testnet — fork of arkadianet/ergo with YOLO emission,
    /// 20 s blocks, miner-only storage rent. Magic `[0x59, 0x4F, 0x4C,
    /// 0x4F]` ("YOLO" ASCII), address prefix `0x20`.
    SigmaChainTestnet,
}

impl Network {
    /// Lowercase canonical name, matching the Scala `ergo.networkType`
    /// HOCON field value (plus `"sigmachain-testnet"` for the YOLO fork).
    pub const fn as_str(self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::SigmaChainTestnet => "sigmachain-testnet",
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Network {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            "sigmachain-testnet" => Ok(Network::SigmaChainTestnet),
            other => Err(format!("unknown network: {other}")),
        }
    }
}

/// Wire-level identity for a network: P2P handshake magic and the
/// high-nibble byte folded into base58 address encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkParams {
    /// P2P handshake magic. Scala `scorex.network.magicBytes`.
    pub magic: [u8; 4],
    /// High-nibble byte applied to base58 addresses (mainnet 0x00,
    /// testnet 0x10). Scala `ergo.chain.addressPrefix`.
    pub address_prefix: NetworkPrefix,
}

impl NetworkParams {
    /// Mainnet identity. Source: `reference/ergo/src/main/resources/mainnet.conf`
    /// (`magicBytes = [1, 0, 2, 4]` at line 125; `addressPrefix = 0` at line 12).
    pub const MAINNET: NetworkParams = NetworkParams {
        magic: [1, 0, 2, 4],
        address_prefix: NetworkPrefix::Mainnet,
    };

    /// Testnet identity at v6.0.3+. Source: upstream `testnet.conf`
    /// after PR #2252 (`scorex.network.magicBytes = [2, 3, 2, 3]`
    /// on `:9023`; `chain.addressPrefix = 16`). The previous public
    /// testnet (PaiNet, magicBytes `[2, 0, 2, 3]` on `:9022`) was
    /// retired by that PR.
    pub const TESTNET: NetworkParams = NetworkParams {
        magic: [2, 3, 2, 3],
        address_prefix: NetworkPrefix::Testnet,
    };

    /// SigmaChain testnet identity. Magic bytes are the ASCII for
    /// "YOLO" (`Y=0x59, O=0x4F, L=0x4C, O=0x4F`), chosen so any node
    /// debugging a p2p capture instantly recognizes the network from
    /// the first 4 bytes. Address prefix `0x20` is distinct from
    /// Ergo's `0x00` mainnet and `0x10` testnet, so wallets reject
    /// cross-network addresses at decode time.
    pub const SIGMACHAIN_TESTNET: NetworkParams = NetworkParams {
        magic: [0x59, 0x4F, 0x4C, 0x4F],
        address_prefix: NetworkPrefix::SigmaChainTestnet,
    };

    /// Identity for the given [`Network`].
    pub const fn for_network(net: Network) -> NetworkParams {
        match net {
            Network::Mainnet => Self::MAINNET,
            Network::Testnet => Self::TESTNET,
            Network::SigmaChainTestnet => Self::SIGMACHAIN_TESTNET,
        }
    }
}

/// Difficulty-recalculation parameters: epoch length(s), EIP-37 switch
/// boundary, Autolykos v2 activation, initial difficulty seeds, and the
/// network's target block interval. Consumed by `ergo-crypto::difficulty`
/// and `ergo-crypto::pow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DifficultyParams {
    /// Pre-EIP-37 epoch length in blocks. Mainnet 1024; testnet 128.
    pub epoch_length: u32,
    /// Post-EIP-37 epoch length in blocks. `None` if EIP-37 never
    /// activates on this network. Mainnet `Some(128)`; testnet `None`.
    pub eip37_epoch_length: Option<u32>,
    /// Height at which EIP-37 activates. `None` if EIP-37 never
    /// activates on this network. Mainnet `Some(844_673)`; testnet `None`.
    pub eip37_activation_height: Option<u32>,
    /// Autolykos v1 → v2 hard-fork descriptor. `None` on networks
    /// whose genesis block already carries a v2-or-later block
    /// version, so the difficulty interpolator has no v1 epoch to
    /// transition out of.
    pub v2_activation: Option<V2Activation>,
    /// Initial difficulty at genesis (height 1), big-endian bytes.
    /// Scala `chain.initialDifficultyHex`.
    pub initial_difficulty: Vec<u8>,
    /// Target block interval, milliseconds. Mainnet 120_000 (2 min);
    /// testnet 45_000. Used inside the difficulty interpolator.
    pub desired_interval_ms: u64,
}

/// Network-specific v1 → v2 hard-fork descriptor. Carried as
/// `Option<V2Activation>` on [`DifficultyParams`] so a network that
/// starts at v2-or-later block version from genesis can encode the
/// absence of a transition rather than smuggling in a sentinel
/// height.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct V2Activation {
    /// Block height at which v2 activates. The difficulty
    /// recalculator returns [`Self::initial_difficulty`] when
    /// `parent_height == height` or `parent_height + 1 == height`.
    pub height: u32,
    /// Initial difficulty at the v2 activation block, big-endian
    /// bytes. Scala `chain.voting.version2ActivationDifficultyHex`.
    pub initial_difficulty: Vec<u8>,
}

impl DifficultyParams {
    /// Mainnet defaults. Source: `reference/ergo/src/main/resources/mainnet.conf`
    /// (`chain.eip37EpochLength = 128`, `chain.initialDifficultyHex = "011765000000"`,
    /// `chain.voting.version2ActivationHeight = 417792`,
    /// `chain.voting.version2ActivationDifficultyHex = "6f98d5000000"`,
    /// pre-EIP-37 epoch length 1024 per Scala `LaunchParameters`; EIP-37
    /// activation at 844_673 per `Parameters.scala`). Target block
    /// interval 2 minutes.
    pub fn mainnet() -> Self {
        Self {
            epoch_length: 1024,
            eip37_epoch_length: Some(128),
            eip37_activation_height: Some(844_673),
            v2_activation: Some(V2Activation {
                height: 417_792,
                initial_difficulty: vec![0x6f, 0x98, 0xd5, 0x00, 0x00, 0x00],
            }),
            initial_difficulty: vec![0x01, 0x17, 0x65, 0x00, 0x00, 0x00],
            desired_interval_ms: 120_000,
        }
    }

    /// Testnet defaults. Source: `reference/ergo/src/main/resources/testnet.conf`
    /// (`chain.initialDifficultyHex = "01"`, `chain.epochLength = 128`,
    /// `chain.blockInterval = 45s`). Testnet has no EIP-37 boundary —
    /// its `epoch_length` is already 128 throughout — and no v1 → v2
    /// transition because `TestnetLaunchParameters.scala` sets
    /// `BlockVersion = Interpreter60Version` (= 4) at genesis.
    pub fn testnet() -> Self {
        Self {
            epoch_length: 128,
            eip37_epoch_length: None,
            eip37_activation_height: None,
            v2_activation: None,
            initial_difficulty: vec![0x01],
            desired_interval_ms: 45_000,
        }
    }

    /// SigmaChain testnet difficulty schedule. Mirrors testnet shape
    /// (no EIP-37, no v1 → v2 transition — SigmaChain launches at
    /// `Interpreter60Version` directly), with two divergences:
    ///
    /// - `desired_interval_ms = 20_000` (20 s blocks per the YOLO
    ///   emission contract; see `01-emission-tests/emission.es` line 37
    ///   "blocksPerHalving: Int = 1577880 ~1 year at 20s blocks").
    /// - `epoch_length = 6_144` (= Ergo mainnet's 1024 × 120/20). Scales
    ///   to preserve roughly 34 hours of wall-clock per difficulty
    ///   epoch, matching mainnet's adjustment cadence.
    ///
    /// Initial difficulty stays at `0x01` (testnet bootstrap level —
    /// genesis miner can start mining instantly on a fresh chain).
    pub fn sigmachain_testnet() -> Self {
        Self {
            epoch_length: 6_144,
            eip37_epoch_length: None,
            eip37_activation_height: None,
            v2_activation: None,
            initial_difficulty: vec![0x01],
            desired_interval_ms: 20_000,
        }
    }

    /// Schedule for the given [`Network`].
    pub fn for_network(net: Network) -> Self {
        match net {
            Network::Mainnet => Self::mainnet(),
            Network::Testnet => Self::testnet(),
            Network::SigmaChainTestnet => Self::sigmachain_testnet(),
        }
    }
}

/// Voting epoch parameters: soft-fork tally window length, fork-vote
/// thresholds, and the non-voted v2 hard-fork height. Consumed by
/// `ergo-validation::voting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VotingParams {
    /// Length of a voting epoch in blocks. Scala
    /// `chain.voting.votingLength`. Drives both `soft_fork_approved`
    /// and `change_approved` thresholds.
    pub voting_length: u32,
    /// Number of voting epochs needed to approve a soft fork. Scala
    /// `chain.voting.softForkEpochs`.
    pub soft_fork_epochs: u32,
    /// Number of voting epochs the network waits between approval and
    /// activation. Scala `chain.voting.activationEpochs`.
    pub activation_epochs: u32,
    /// Non-voted hard-fork height where protocol block version flips
    /// from 1 to 2 (Autolykos v2 + tx witness commitments). Scala
    /// `chain.voting.version2ActivationHeight`. `None` on networks
    /// whose genesis already carries a v2-or-later block version, so
    /// the voting-driven version bump never fires.
    pub version2_activation: Option<u32>,
}

impl VotingParams {
    /// Mainnet defaults. Source: `mainnet.conf` (`voting.votingLength`
    /// derived from `application.conf`, `voting.softForkEpochs = 32`,
    /// `voting.activationEpochs = 32`, `voting.version2ActivationHeight
    /// = 417792`).
    pub const fn mainnet() -> Self {
        Self {
            voting_length: 1024,
            soft_fork_epochs: 32,
            activation_epochs: 32,
            version2_activation: Some(417_792),
        }
    }

    /// Testnet defaults. Source: `testnet.conf` at v6.0.3 —
    /// `votingLength = 128`, `softForkEpochs = 32`,
    /// `activationEpochs = 32`. The shorter epoch length matches
    /// testnet's 45 s block interval so the wall-clock voting window
    /// is comparable to mainnet's. No `version2ActivationHeight`
    /// because `TestnetLaunchParameters.scala` sets
    /// `BlockVersion = Interpreter60Version` (= 4) at genesis — there
    /// is no v1 → v2 transition for the voting state machine to
    /// trigger.
    pub const fn testnet() -> Self {
        Self {
            voting_length: 128,
            soft_fork_epochs: 32,
            activation_epochs: 32,
            version2_activation: None,
        }
    }

    /// SigmaChain testnet voting params. `voting_length = 6_144`
    /// matches the difficulty epoch so a single epoch boundary serves
    /// both adjustment and vote-counting (Ergo mainnet uses the same
    /// alignment: `chain.voting.votingLength == chain.epochLength`).
    /// At 20 s blocks, 6_144 blocks = ~34 h wall-clock — comparable to
    /// mainnet's ~34 h (1024 × 120 s). `soft_fork_epochs` and
    /// `activation_epochs` stay at 32. No `version2_activation`:
    /// SigmaChain launches at `Interpreter60Version` and never carries
    /// a v1 → v2 transition.
    pub const fn sigmachain_testnet() -> Self {
        Self {
            voting_length: 6_144,
            soft_fork_epochs: 32,
            activation_epochs: 32,
            version2_activation: None,
        }
    }

    /// `softForkApproved(votes) = votes > votingLength * softForkEpochs
    /// * 9 / 10`. Mainnet threshold: `> 29_491`
    /// (`reference/ergo/.../settings/VotingSettings.scala:9`).
    pub fn soft_fork_approved(&self, votes: i32) -> bool {
        let threshold = (self.voting_length * self.soft_fork_epochs * 9 / 10) as i32;
        votes > threshold
    }

    /// `changeApproved(count) = count > votingLength / 2`. Mainnet
    /// threshold: `> 512`
    /// (`reference/ergo/.../settings/VotingSettings.scala:11`).
    pub fn change_approved(&self, count: i32) -> bool {
        count > (self.voting_length / 2) as i32
    }
}

/// Emission curve parameters: fixed-rate window, per-epoch reduction,
/// founder split, and the miner reward maturity delay. Same values on
/// mainnet and testnet — only the `desired_interval_ms` in
/// [`DifficultyParams`] differs, which means real-time emission rates
/// diverge while block counts agree. Consumed by `ergo-mining`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonetaryParams {
    /// Per-block reward during the fixed-rate window, nanoERG. Scala
    /// `monetary.fixedRate = 75 * 10^9`.
    pub fixed_rate: u64,
    /// Length of the fixed-rate window in blocks. Scala
    /// `monetary.fixedRatePeriod = 525_600`.
    pub fixed_rate_period: u32,
    /// Length of one post-fixed-rate epoch in blocks. Scala
    /// `monetary.epochLength = 64_800`.
    pub epoch_length: u32,
    /// Per-epoch reward reduction after the fixed-rate window,
    /// nanoERG. Scala `monetary.oneEpochReduction = 3 * 10^9`.
    pub one_epoch_reduction: u64,
    /// Founders' share of the fixed-rate reward, nanoERG. Scala
    /// `monetary.foundersInitialReward = 7_500_000_000` (10 % of
    /// 75 ERG).
    pub founders_initial_reward: u64,
    /// Block-height delay between mining a reward and being able to
    /// spend it. Scala `monetary.minerRewardDelay = 720`.
    pub miner_reward_delay: u32,
}

impl MonetaryParams {
    /// Mainnet defaults. Source: `application.conf:174-187` (mainnet
    /// `monetary` section inherits the defaults). Same as Scala
    /// `MonetarySettings.scala:13-18`.
    pub const fn mainnet() -> Self {
        Self {
            fixed_rate: 75 * 1_000_000_000,
            fixed_rate_period: 525_600,
            epoch_length: 64_800,
            one_epoch_reduction: 3 * 1_000_000_000,
            founders_initial_reward: 75 * 1_000_000_000 / 10,
            miner_reward_delay: 720,
        }
    }

    /// Testnet defaults. Same field values as [`Self::mainnet`] per
    /// `testnet.conf:46-49` — testnet inherits the `application.conf`
    /// monetary section unchanged; only `minerRewardDelay = 720` is
    /// echoed explicitly. The shorter testnet block interval changes
    /// real-time emission rate but not the block-count schedule.
    pub const fn testnet() -> Self {
        Self::mainnet()
    }

    /// SigmaChain testnet monetary params. Phase 3.1 stub — clones
    /// `testnet()` (Ergo curve). Phase 3.3 will replace this with the
    /// YOLO geometric-halving emission (50 → 25 → … → 1 tail at
    /// 1_577_880-block halvings, 85/10/5 split). The existing field
    /// shape can't express that curve; chunk 3.3 will introduce an
    /// `EmissionCurve` enum or per-network function.
    pub const fn sigmachain_testnet() -> Self {
        Self::testnet()
    }
}

/// Re-emission (EIP-27) parameters: activation height, distribution
/// schedule, and the token / NFT identities that drive the injection.
/// All values differ between mainnet and testnet; testnet's
/// `activation_height = 100_000_001` keeps reemission effectively
/// disabled on the public testnet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReemissionParams {
    /// Height at which reemission activates (the injection tx is
    /// mined at this height). Scala `reemission.activationHeight`.
    pub activation_height: u32,
    /// Height at which the per-block reemission distribution stops.
    /// Scala `reemission.reemissionStartHeight`.
    pub reemission_start_height: u32,
    /// 32-byte id of the NFT that lives in the emission box and is
    /// transferred to the post-activation emission box. Scala
    /// `reemission.emissionNftId`.
    pub emission_nft_id: Digest32,
    /// 32-byte id of the reemission NFT that lives in the injection
    /// box. Scala `reemission.reemissionNftId`.
    pub reemission_nft_id: Digest32,
    /// 32-byte id of the reemission token unlocked per block.
    /// Scala `reemission.reemissionTokenId`.
    pub reemission_token_id: Digest32,
}

impl ReemissionParams {
    /// Mainnet defaults. Source: `mainnet.conf:43-57` plus the running
    /// Scala node at activation height 777_217.
    pub fn mainnet() -> Self {
        Self {
            activation_height: 777_217,
            reemission_start_height: 2_080_800,
            emission_nft_id: parse_id_hex(
                "20fa2bf23962cdf51b07722d6237c0c7b8a44f78856c0f7ec308dc1ef1a92a51",
            ),
            reemission_nft_id: parse_id_hex(
                "d3feeffa87f2df63a7a15b4905e618ae3ce4c69a7975f171bd314d0b877927b8",
            ),
            reemission_token_id: parse_id_hex(
                "d9a2cc8a09abfaed87afacfbb7daee79a6b26f10c6613fc13d3f3953e5521d1a",
            ),
        }
    }
}

/// Genesis identity for a network: the AVL state root at height 0,
/// the canonical first mined header (height 1) id, and the embedded
/// genesis-boxes JSON used to seed the state tree on a cold start.
#[derive(Debug, Clone, Copy)]
pub struct GenesisParams {
    /// AVL state root at height 0, 33 bytes (32-byte digest + 1-byte
    /// tree-height marker). Scala `chain.genesisStateDigestHex`.
    pub state_digest: [u8; 33],
    /// Header id of the first mined block (height 1). The NiPoPoW
    /// proof verifier rejects any proof whose first header's id does
    /// not match this. `None` for networks where the value isn't
    /// embedded yet (open verification — testnet during bootstrap).
    /// Scala `chain.genesisId`.
    pub header_id: Option<[u8; 32]>,
    /// Genesis boxes as raw JSON, embedded at compile time. `None`
    /// until vectors are extracted from a Scala node of that network
    /// (see `test-vectors/<network>/PROVISIONING.md`). Parsed by
    /// `ergo-node::genesis::parse_genesis_boxes` when present.
    pub boxes_json: Option<&'static str>,
}

impl GenesisParams {
    /// Mainnet genesis. Source: `mainnet.conf:21,33` for the ids;
    /// `test-vectors/mainnet/genesis_boxes.json` for the embedded
    /// boxes (Scala-extracted).
    pub fn mainnet() -> Self {
        Self {
            state_digest: parse_digest33_hex(
                "a5df145d41ab15a01e0cd3ffbab046f0d029e5412293072ad0f5827428589b9302",
            ),
            header_id: Some(parse_bytes32_hex(
                "b0244dfc267baca974a4caee06120321562784303a8a688976ae56170e4d175b",
            )),
            boxes_json: Some(include_str!(
                "../../test-vectors/mainnet/genesis_boxes.json"
            )),
        }
    }

    /// Testnet genesis. Sources: `testnet.conf:84` for the state
    /// digest; `/blocks/at/1` on a v6.0.3+ Scala testnet node for the
    /// height-1 header id; `/utxo/genesis` on the same node for the
    /// embedded boxes (see `test-vectors/testnet/PROVISIONING.md`).
    pub fn testnet() -> Self {
        Self {
            state_digest: parse_digest33_hex(
                "cb63aa99a3060f341781d8662b58bf18b9ad258db4fe88d09f8f71cb668cad4502",
            ),
            header_id: Some(parse_bytes32_hex(
                "5b1827ca092b599eafbaf339d2acf2445bc5216ec2e022d9c001a6fff660cad9",
            )),
            boxes_json: Some(include_str!(
                "../../test-vectors/testnet/genesis_boxes.json"
            )),
        }
    }

    /// SigmaChain testnet genesis. Phase 3.1 stub: zero state digest,
    /// no header id, no embedded boxes — node will refuse to start
    /// until Phase 4 builds the real genesis state. Tests can still
    /// construct a `ChainSpec::sigmachain_testnet()` for parity/identity
    /// checks without exercising the genesis loader.
    pub fn sigmachain_testnet() -> Self {
        Self {
            state_digest: [0u8; 33],
            header_id: None,
            boxes_json: None,
        }
    }

    /// Dispatch to the per-network genesis parameters. Mirrors the
    /// `for_network` accessor on the sibling chain-spec params types so
    /// boot can resolve the genesis state digest without a manual match.
    pub fn for_network(net: Network) -> Self {
        match net {
            Network::Mainnet => Self::mainnet(),
            Network::Testnet => Self::testnet(),
            Network::SigmaChainTestnet => Self::sigmachain_testnet(),
        }
    }
}

/// Sync-tip and tip-readiness parameters derived from the network's
/// target block interval. The same `desired_interval_ms` value also
/// lives on [`DifficultyParams`] — both consumers (difficulty
/// interpolator and sync-tip detector) get a narrow view of just what
/// they need without holding the other crate's params type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockTimingParams {
    /// Target block interval, milliseconds. Mainnet 120_000 (2 min);
    /// testnet 45_000 (45 s).
    pub desired_interval_ms: u64,
    /// Maximum block-count gap between local best-header tip and the
    /// network's current height before the node still considers
    /// itself "synced". Scala `node.headerChainDiff`. Mainnet 100,
    /// testnet 800.
    pub header_chain_diff: u32,
}

impl BlockTimingParams {
    /// Mainnet defaults. Source: `application.conf` —
    /// `chain.blockInterval = 2m` and `node.headerChainDiff = 100`.
    pub const fn mainnet() -> Self {
        Self {
            desired_interval_ms: 120_000,
            header_chain_diff: 100,
        }
    }

    /// Testnet defaults. Source: `testnet.conf:43` —
    /// `chain.blockInterval = 45s`; `testnet.conf:10` —
    /// `node.headerChainDiff = 800`. The longer chain-diff window
    /// matches testnet's shorter blocks so the freshness threshold
    /// (45_000 × 800 = 36_000_000 ms = 600 minutes) tolerates the
    /// network's lighter mining activity.
    pub const fn testnet() -> Self {
        Self {
            desired_interval_ms: 45_000,
            header_chain_diff: 800,
        }
    }

    /// SigmaChain testnet timing. `desired_interval_ms = 20_000` per
    /// the YOLO emission contract; `header_chain_diff = 600` so the
    /// freshness threshold is `20_000 × 600 = 12_000_000 ms = 200
    /// minutes` — comparable to Ergo mainnet's 200 min (`120_000 × 100`)
    /// rather than Ergo testnet's much more permissive 600 min. We
    /// pick mainnet-equivalent freshness because SigmaChain testnet's
    /// primary purpose is integration testing of the YoloDAO contracts
    /// at production parity, not validator-friendly sparse uptime.
    pub const fn sigmachain_testnet() -> Self {
        Self {
            desired_interval_ms: 20_000,
            header_chain_diff: 600,
        }
    }

    /// Sync-tip threshold the headers-chain-synced gate compares
    /// against: `desired_interval_ms * header_chain_diff`. A header
    /// whose timestamp is within this many milliseconds of `now` is
    /// considered fresh enough that the chain is "synced".
    pub const fn header_freshness_threshold_ms(&self) -> u64 {
        self.desired_interval_ms * self.header_chain_diff as u64
    }
}

/// Network bootstrap inputs: hardcoded seed peers and an optional
/// historical "trust this block id at this height" checkpoint that
/// lets the node skip per-input script validation up to that height
/// during initial sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapParams {
    /// Seed peers to dial on first start, before any user-supplied
    /// `known_peers` overrides. Scala `scorex.network.knownPeers`.
    pub seed_peers: Vec<SocketAddr>,
    /// Optional script-validation checkpoint: `(height, block_id)`.
    /// Blocks at or below `height` skip per-input ErgoScript
    /// evaluation but still verify per-block AVL state roots; the
    /// configured `block_id` is asserted on apply. Scala
    /// `node.checkpoint`.
    pub checkpoint: Option<(u32, [u8; 32])>,
}

/// Aggregate of every network-specific parameter group: the single
/// source of truth for "what does this network look like." Constructed
/// once at node startup via [`ChainSpec::for_network`] and threaded as
/// `Arc<ChainSpec>` into long-lived services; pure protocol functions
/// downstream borrow narrow views (`&DifficultyParams`, `&VotingParams`,
/// …) and never see [`Network`] discrimination directly.
#[derive(Debug, Clone)]
pub struct ChainSpec {
    /// The network this spec describes. Useful for telemetry, REST
    /// `/info`, and the rare consumer that genuinely needs to know;
    /// chain rules read the narrow params instead.
    pub network: Network,
    /// Magic bytes + address prefix.
    pub network_params: NetworkParams,
    /// Difficulty recalc schedule + block interval.
    pub difficulty: DifficultyParams,
    /// Voting epoch length + soft-fork thresholds + v2 hard-fork height.
    pub voting: VotingParams,
    /// Emission curve.
    pub monetary: MonetaryParams,
    /// Re-emission (EIP-27) schedule + token / NFT identities. `None`
    /// on networks that don't enable the EIP-27 reemission protocol
    /// (new public testnet, post PR #2252).
    pub reemission: Option<ReemissionParams>,
    /// Genesis state digest, header id, and embedded boxes.
    pub genesis: GenesisParams,
    /// Sync-tip tolerances and other block-interval-derived thresholds.
    pub block_timing: BlockTimingParams,
    /// Seed peers and the optional script-validation checkpoint.
    pub bootstrap: BootstrapParams,
}

impl ChainSpec {
    /// Mainnet spec: every parameter group set to its canonical
    /// mainnet value.
    pub fn mainnet() -> Self {
        Self {
            network: Network::Mainnet,
            network_params: NetworkParams::MAINNET,
            difficulty: DifficultyParams::mainnet(),
            voting: VotingParams::mainnet(),
            monetary: MonetaryParams::mainnet(),
            reemission: Some(ReemissionParams::mainnet()),
            genesis: GenesisParams::mainnet(),
            block_timing: BlockTimingParams::mainnet(),
            bootstrap: BootstrapParams::mainnet(),
        }
    }

    /// Testnet spec, populated from `testnet.conf`. Every narrow param
    /// carries the canonical testnet values; the genesis header id and
    /// genesis box set are extracted from a v6.0.3+ Scala testnet node
    /// (see `test-vectors/testnet/PROVISIONING.md`).
    pub fn testnet() -> Self {
        Self {
            network: Network::Testnet,
            network_params: NetworkParams::TESTNET,
            difficulty: DifficultyParams::testnet(),
            voting: VotingParams::testnet(),
            monetary: MonetaryParams::testnet(),
            reemission: None,
            genesis: GenesisParams::testnet(),
            block_timing: BlockTimingParams::testnet(),
            bootstrap: BootstrapParams::testnet(),
        }
    }

    /// SigmaChain testnet spec. Phase 3.1: identity-only fork — only
    /// `network_params` (magic + address prefix) differs from upstream
    /// `Ergo` testnet; every other narrow param is the testnet clone.
    /// Phases 3.2–3.4 will diverge block timing, voting, monetary,
    /// genesis, and storage rent to YOLO targets. `reemission` is
    /// `None` (SigmaChain has no EIP-27).
    pub fn sigmachain_testnet() -> Self {
        Self {
            network: Network::SigmaChainTestnet,
            network_params: NetworkParams::SIGMACHAIN_TESTNET,
            difficulty: DifficultyParams::sigmachain_testnet(),
            voting: VotingParams::sigmachain_testnet(),
            monetary: MonetaryParams::sigmachain_testnet(),
            reemission: None,
            genesis: GenesisParams::sigmachain_testnet(),
            block_timing: BlockTimingParams::sigmachain_testnet(),
            bootstrap: BootstrapParams::sigmachain_testnet(),
        }
    }

    /// Spec for the given [`Network`]. This is the only place that
    /// branches on `Network` variants; downstream code takes narrow
    /// views.
    pub fn for_network(net: Network) -> Self {
        match net {
            Network::Mainnet => Self::mainnet(),
            Network::Testnet => Self::testnet(),
            Network::SigmaChainTestnet => Self::sigmachain_testnet(),
        }
    }
}

impl BootstrapParams {
    /// Mainnet defaults. Seed peers from `mainnet.conf:129-143`;
    /// checkpoint from `mainnet.conf:73-76` (height 1_231_454,
    /// `ca5aa96a…`).
    pub fn mainnet() -> Self {
        let seed_strs = [
            "213.239.193.208:9030",
            "159.65.11.55:9030",
            "165.227.26.175:9030",
            "159.89.116.15:9030",
            "136.244.110.145:9030",
            "94.130.108.35:9030",
            "51.75.147.1:9020",
            "221.165.214.185:9030",
            "217.182.197.196:9030",
            "173.212.220.9:9030",
            "176.9.65.58:9130",
            "213.152.106.56:9030",
            "[2001:41d0:700:6662::]:29031",
        ];
        Self {
            seed_peers: seed_strs
                .iter()
                .filter_map(|s| s.parse::<SocketAddr>().ok())
                .collect(),
            checkpoint: Some((
                1_231_454,
                parse_bytes32_hex(
                    "ca5aa96a2d560f49cd5652eae4b9e16bbf410ee32365313dc16544ee5fda1e6d",
                ),
            )),
        }
    }

    /// Testnet defaults at v6.0.3+. Seed peers from upstream
    /// `testnet.conf` after PR #2252 — three public testnet nodes
    /// on `:9023`. Checkpoint is `None`: the previous PaiNet
    /// checkpoint at `h = 91_320` was removed alongside the
    /// network reset, and the new chain hasn't published one yet.
    pub fn testnet() -> Self {
        let seed_strs = [
            "213.239.193.208:9023",
            "168.138.185.215:9023",
            "192.234.196.165:9023",
        ];
        Self {
            seed_peers: seed_strs
                .iter()
                .filter_map(|s| s.parse::<SocketAddr>().ok())
                .collect(),
            checkpoint: None,
        }
    }

    /// SigmaChain testnet bootstrap. No public seed peers yet — the
    /// network bootstraps from a single local node. Operators add
    /// peers via the `[peers] known_peers` TOML field. No checkpoint
    /// (fresh chain).
    pub fn sigmachain_testnet() -> Self {
        Self {
            seed_peers: Vec::new(),
            checkpoint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ----- happy path -----

    #[test]
    fn network_roundtrips_through_str() {
        for net in [
            Network::Mainnet,
            Network::Testnet,
            Network::SigmaChainTestnet,
        ] {
            assert_eq!(net.as_str().parse::<Network>().unwrap(), net);
        }
    }

    #[test]
    fn network_display_matches_as_str() {
        assert_eq!(format!("{}", Network::Mainnet), "mainnet");
        assert_eq!(format!("{}", Network::Testnet), "testnet");
        assert_eq!(
            format!("{}", Network::SigmaChainTestnet),
            "sigmachain-testnet"
        );
    }

    #[test]
    fn for_network_picks_right_params() {
        assert_eq!(
            NetworkParams::for_network(Network::Mainnet),
            NetworkParams::MAINNET
        );
        assert_eq!(
            NetworkParams::for_network(Network::Testnet),
            NetworkParams::TESTNET
        );
        assert_eq!(
            NetworkParams::for_network(Network::SigmaChainTestnet),
            NetworkParams::SIGMACHAIN_TESTNET
        );
    }

    // ----- oracle parity -----

    #[test]
    fn mainnet_magic_matches_scala_conf() {
        // mainnet.conf line 125: scorex.network.magicBytes = [1, 0, 2, 4]
        assert_eq!(NetworkParams::MAINNET.magic, [1, 0, 2, 4]);
    }

    #[test]
    fn testnet_magic_matches_scala_conf() {
        // v6.0.3 testnet.conf: scorex.network.magicBytes = [2, 3, 2, 3]
        // (the new public testnet from PR #2252; the previous PaiNet
        // used [2, 0, 2, 3]).
        assert_eq!(NetworkParams::TESTNET.magic, [2, 3, 2, 3]);
    }

    #[test]
    fn mainnet_address_prefix_is_zero() {
        // mainnet.conf line 12: ergo.chain.addressPrefix = 0
        assert_eq!(
            NetworkParams::MAINNET.address_prefix,
            NetworkPrefix::Mainnet
        );
        assert_eq!(NetworkParams::MAINNET.address_prefix as u8, 0x00);
    }

    #[test]
    fn testnet_address_prefix_is_sixteen() {
        // testnet.conf line 36: ergo.chain.addressPrefix = 16
        assert_eq!(
            NetworkParams::TESTNET.address_prefix,
            NetworkPrefix::Testnet
        );
        assert_eq!(NetworkParams::TESTNET.address_prefix as u8, 0x10);
    }

    // ----- error paths -----

    #[test]
    fn network_from_str_unknown_errors() {
        assert!("devnet".parse::<Network>().is_err());
        assert!("".parse::<Network>().is_err());
        assert!("MAINNET\0".parse::<Network>().is_err());
    }

    // ----- difficulty -----

    #[test]
    fn difficulty_mainnet_matches_scala_conf() {
        // mainnet.conf:9-37 — protocolVersion 4, initialDifficultyHex "011765000000",
        // version2ActivationHeight 417792, version2ActivationDifficultyHex "6f98d5000000",
        // eip37EpochLength 128, with pre-EIP-37 epochLength 1024.
        let p = DifficultyParams::mainnet();
        assert_eq!(p.epoch_length, 1024);
        assert_eq!(p.eip37_epoch_length, Some(128));
        assert_eq!(p.eip37_activation_height, Some(844_673));
        let v2 = p.v2_activation.expect("mainnet has v1 → v2 hardfork");
        assert_eq!(v2.height, 417_792);
        assert_eq!(
            v2.initial_difficulty,
            vec![0x6f, 0x98, 0xd5, 0x00, 0x00, 0x00]
        );
        assert_eq!(
            p.initial_difficulty,
            vec![0x01, 0x17, 0x65, 0x00, 0x00, 0x00]
        );
        assert_eq!(p.desired_interval_ms, 120_000);
    }

    #[test]
    fn difficulty_testnet_matches_scala_conf() {
        // testnet.conf at v6.0.3 — initialDifficultyHex "01", epochLength 128,
        // blockInterval 45s. No version2ActivationHeight (TestnetLaunchParameters
        // sets BlockVersion = Interpreter60Version = 4 at genesis, so there is
        // no v1 → v2 transition).
        let p = DifficultyParams::testnet();
        assert_eq!(p.epoch_length, 128);
        assert_eq!(p.eip37_epoch_length, None);
        assert_eq!(p.eip37_activation_height, None);
        assert!(p.v2_activation.is_none());
        assert_eq!(p.initial_difficulty, vec![0x01]);
        assert_eq!(p.desired_interval_ms, 45_000);
    }

    // ----- voting / monetary / reemission -----

    #[test]
    fn voting_mainnet_matches_scala_conf() {
        // mainnet.conf:35-41 + application.conf voting defaults.
        let p = VotingParams::mainnet();
        assert_eq!(p.voting_length, 1024);
        assert_eq!(p.soft_fork_epochs, 32);
        assert_eq!(p.activation_epochs, 32);
        assert_eq!(p.version2_activation, Some(417_792));
    }

    #[test]
    fn monetary_mainnet_matches_scala_defaults() {
        // application.conf:174-187 (MonetarySettings.scala:13-18).
        let p = MonetaryParams::mainnet();
        assert_eq!(p.fixed_rate, 75 * 1_000_000_000);
        assert_eq!(p.fixed_rate_period, 525_600);
        assert_eq!(p.epoch_length, 64_800);
        assert_eq!(p.one_epoch_reduction, 3 * 1_000_000_000);
        assert_eq!(p.founders_initial_reward, 7_500_000_000);
        assert_eq!(p.miner_reward_delay, 720);
    }

    #[test]
    fn reemission_mainnet_matches_scala_conf() {
        // mainnet.conf:43-57.
        let p = ReemissionParams::mainnet();
        assert_eq!(p.activation_height, 777_217);
        assert_eq!(p.reemission_start_height, 2_080_800);
        assert_eq!(
            hex::encode(p.emission_nft_id.as_bytes()),
            "20fa2bf23962cdf51b07722d6237c0c7b8a44f78856c0f7ec308dc1ef1a92a51"
        );
        assert_eq!(
            hex::encode(p.reemission_nft_id.as_bytes()),
            "d3feeffa87f2df63a7a15b4905e618ae3ce4c69a7975f171bd314d0b877927b8"
        );
        assert_eq!(
            hex::encode(p.reemission_token_id.as_bytes()),
            "d9a2cc8a09abfaed87afacfbb7daee79a6b26f10c6613fc13d3f3953e5521d1a"
        );
    }

    // ----- genesis / timing / bootstrap -----

    #[test]
    fn genesis_mainnet_matches_scala_conf() {
        // mainnet.conf:21,33 — genesisId and genesisStateDigestHex.
        let p = GenesisParams::mainnet();
        assert_eq!(
            hex::encode(p.state_digest),
            "a5df145d41ab15a01e0cd3ffbab046f0d029e5412293072ad0f5827428589b9302"
        );
        assert_eq!(
            p.header_id.map(hex::encode).unwrap(),
            "b0244dfc267baca974a4caee06120321562784303a8a688976ae56170e4d175b"
        );
        // Embedded JSON is non-empty and parses as a JSON array of box objects.
        let boxes = p.boxes_json.expect("mainnet embeds genesis_boxes.json");
        assert!(!boxes.is_empty());
        assert!(boxes.starts_with('['));
    }

    // ----- testnet oracle parity -----

    #[test]
    fn voting_testnet_matches_scala_conf() {
        // testnet.conf at v6.0.3 — votingLength = 128,
        // softForkEpochs = 32, activationEpochs = 32. No
        // version2ActivationHeight (TestnetLaunchParameters sets
        // BlockVersion = Interpreter60Version at genesis, so the
        // version bump never fires).
        let p = VotingParams::testnet();
        assert_eq!(p.voting_length, 128);
        assert_eq!(p.soft_fork_epochs, 32);
        assert_eq!(p.activation_epochs, 32);
        assert!(p.version2_activation.is_none());
    }

    #[test]
    fn monetary_testnet_matches_mainnet() {
        // testnet.conf:46-49 — monetary section inherits application.conf
        // defaults; only minerRewardDelay = 720 is echoed explicitly.
        assert_eq!(MonetaryParams::testnet(), MonetaryParams::mainnet());
    }

    #[test]
    fn chain_spec_testnet_has_no_reemission() {
        // testnet.conf at v6.0.3 comments out all reemission token /
        // NFT ids plus injectionBoxBytesEncoded and adds an explicit
        // "no re-emission" marker. ChainSpec.reemission is therefore
        // None on testnet — there is no EIP-27 protocol to assemble.
        assert!(ChainSpec::testnet().reemission.is_none());
    }

    #[test]
    fn genesis_testnet_matches_scala_conf() {
        // testnet.conf:84 — genesisStateDigestHex. The header id and
        // genesis box set come from a v6.0.3+ Scala testnet node via
        // /blocks/at/1 and /utxo/genesis respectively.
        let p = GenesisParams::testnet();
        assert_eq!(
            hex::encode(p.state_digest),
            "cb63aa99a3060f341781d8662b58bf18b9ad258db4fe88d09f8f71cb668cad4502"
        );
        assert_eq!(
            p.header_id.map(hex::encode).unwrap(),
            "5b1827ca092b599eafbaf339d2acf2445bc5216ec2e022d9c001a6fff660cad9"
        );
        let boxes = p.boxes_json.expect("testnet embeds genesis_boxes.json");
        assert!(!boxes.is_empty());
        assert!(boxes.starts_with('['));
        assert_eq!(boxes.matches("\"boxId\"").count(), 3);
    }

    #[test]
    fn block_timing_testnet_matches_scala_conf() {
        // testnet.conf:43 (blockInterval = 45s) and testnet.conf:10
        // (headerChainDiff = 800).
        let p = BlockTimingParams::testnet();
        assert_eq!(p.desired_interval_ms, 45_000);
        assert_eq!(p.header_chain_diff, 800);
        // 45_000 * 800 = 36_000_000 ms = 600 minutes (~10 h).
        assert_eq!(p.header_freshness_threshold_ms(), 36_000_000);
    }

    #[test]
    fn bootstrap_testnet_has_three_seeds_and_no_checkpoint() {
        // v6.0.3 testnet.conf — three known peers on :9023, no
        // node.checkpoint section (the PaiNet h=91_320 checkpoint
        // was retired by PR #2252).
        let p = BootstrapParams::testnet();
        assert_eq!(p.seed_peers.len(), 3);
        assert!(p.seed_peers.iter().all(|a| a.port() == 9023));
        assert!(p.checkpoint.is_none());
    }

    #[test]
    fn block_timing_mainnet_matches_scala_conf() {
        // application.conf — chain.blockInterval = 2m (120_000 ms),
        // node.headerChainDiff (mainnet inherits default 100).
        let p = BlockTimingParams::mainnet();
        assert_eq!(p.desired_interval_ms, 120_000);
        assert_eq!(p.header_chain_diff, 100);
        // 120_000 * 100 = 12_000_000 ms = 200 minutes (~3.3 h) of
        // header staleness tolerated before "synced" flips false.
        assert_eq!(p.header_freshness_threshold_ms(), 12_000_000);
    }

    // ----- ChainSpec aggregate -----

    #[test]
    fn chain_spec_for_network_dispatches_correctly() {
        let m = ChainSpec::for_network(Network::Mainnet);
        assert_eq!(m.network, Network::Mainnet);
        assert_eq!(m.network_params, NetworkParams::MAINNET);

        let t = ChainSpec::for_network(Network::Testnet);
        assert_eq!(t.network, Network::Testnet);
        assert_eq!(t.network_params, NetworkParams::TESTNET);

        let s = ChainSpec::for_network(Network::SigmaChainTestnet);
        assert_eq!(s.network, Network::SigmaChainTestnet);
        assert_eq!(s.network_params, NetworkParams::SIGMACHAIN_TESTNET);
    }

    // ----- SigmaChain testnet identity -----

    #[test]
    fn sigmachain_testnet_magic_is_yolo_ascii() {
        // [0x59, 0x4F, 0x4C, 0x4F] = ASCII "YOLO". Chosen so a p2p
        // packet capture identifies the network at a glance.
        assert_eq!(
            NetworkParams::SIGMACHAIN_TESTNET.magic,
            [0x59, 0x4F, 0x4C, 0x4F]
        );
        assert_eq!(
            std::str::from_utf8(&NetworkParams::SIGMACHAIN_TESTNET.magic).unwrap(),
            "YOLO"
        );
    }

    #[test]
    fn sigmachain_testnet_magic_differs_from_ergo() {
        // Different magic from Ergo mainnet/testnet means a SigmaChain
        // node and an Ergo node drop each other's packets at the p2p
        // framing layer (Phase 1 inventory: chain-isolation requirement).
        assert_ne!(
            NetworkParams::SIGMACHAIN_TESTNET.magic,
            NetworkParams::MAINNET.magic
        );
        assert_ne!(
            NetworkParams::SIGMACHAIN_TESTNET.magic,
            NetworkParams::TESTNET.magic
        );
    }

    #[test]
    fn sigmachain_testnet_address_prefix_is_0x20() {
        // Distinct high nibble from Ergo mainnet (0x00) and Ergo
        // testnet (0x10) so wallets reject cross-network addresses
        // at decode time.
        assert_eq!(
            NetworkParams::SIGMACHAIN_TESTNET.address_prefix,
            NetworkPrefix::SigmaChainTestnet
        );
        assert_eq!(
            NetworkParams::SIGMACHAIN_TESTNET.address_prefix as u8,
            0x20
        );
    }

    #[test]
    fn sigmachain_testnet_has_no_reemission() {
        // SigmaChain doesn't implement EIP-27 — the emission curve
        // is the YoloDAO emission.es contract (Phase 3.3 will wire
        // the curve into ergo-mining).
        assert!(ChainSpec::sigmachain_testnet().reemission.is_none());
    }

    #[test]
    fn sigmachain_testnet_genesis_is_unbuilt_stub() {
        // Phase 3.1: genesis is a zero-digest placeholder so the
        // chain-spec compiles; Phase 4 will populate it with the
        // real YOLO emission/treasury/LP boxes.
        let g = GenesisParams::sigmachain_testnet();
        assert_eq!(g.state_digest, [0u8; 33]);
        assert!(g.header_id.is_none());
        assert!(g.boxes_json.is_none());
    }

    #[test]
    fn sigmachain_testnet_bootstrap_has_no_seeds_or_checkpoint() {
        // Brand-new network: no public seed peers and no historical
        // checkpoint yet.
        let p = BootstrapParams::sigmachain_testnet();
        assert!(p.seed_peers.is_empty());
        assert!(p.checkpoint.is_none());
    }

    #[test]
    fn sigmachain_testnet_parses_from_str() {
        assert_eq!(
            "sigmachain-testnet".parse::<Network>().unwrap(),
            Network::SigmaChainTestnet
        );
        assert_eq!(
            "SIGMACHAIN-TESTNET".parse::<Network>().unwrap(),
            Network::SigmaChainTestnet
        );
    }

    // ----- SigmaChain testnet block timing + voting (Phase 3.2) -----

    #[test]
    fn sigmachain_testnet_block_interval_is_20s() {
        // The YOLO emission contract pins blocksPerHalving = 1_577_880
        // = 365.25 × 24 × 3600 / 20, which only holds at 20 s blocks.
        let t = BlockTimingParams::sigmachain_testnet();
        assert_eq!(t.desired_interval_ms, 20_000);
    }

    #[test]
    fn sigmachain_testnet_header_freshness_matches_mainnet_minutes() {
        // header_chain_diff = 600 at 20 s blocks gives
        // 20_000 × 600 = 12_000_000 ms = 200 minutes, equal to Ergo
        // mainnet's freshness window. Same wall-clock as mainnet, but
        // for a faster chain.
        let t = BlockTimingParams::sigmachain_testnet();
        assert_eq!(t.header_chain_diff, 600);
        assert_eq!(t.header_freshness_threshold_ms(), 12_000_000);
        assert_eq!(
            t.header_freshness_threshold_ms(),
            BlockTimingParams::mainnet().header_freshness_threshold_ms()
        );
    }

    #[test]
    fn sigmachain_testnet_difficulty_skips_v1_to_v2_and_eip37() {
        // SigmaChain launches at Interpreter60Version (= 4) at genesis,
        // mirroring Ergo testnet's launch — no v1 → v2 transition and
        // no EIP-37 boundary. Difficulty interpolator stays on a
        // single epoch_length throughout.
        let d = DifficultyParams::sigmachain_testnet();
        assert!(d.v2_activation.is_none());
        assert_eq!(d.eip37_epoch_length, None);
        assert_eq!(d.eip37_activation_height, None);
    }

    #[test]
    fn sigmachain_testnet_difficulty_epoch_preserves_mainnet_wall_clock() {
        // Ergo mainnet difficulty epoch: 1024 blocks × 120 s = ~34 h.
        // SigmaChain at 20 s blocks needs 6144 blocks for the same
        // wall-clock: 6144 × 20 = 122_880 s = ~34.13 h.
        let d = DifficultyParams::sigmachain_testnet();
        assert_eq!(d.epoch_length, 6_144);
        let wall_clock_s = u64::from(d.epoch_length) * d.desired_interval_ms / 1000;
        // Within 1% of mainnet's 1024 × 120 = 122_880 s.
        let mainnet_wall_clock_s: u64 = 1024 * 120;
        assert_eq!(wall_clock_s, mainnet_wall_clock_s);
    }

    #[test]
    fn sigmachain_testnet_initial_difficulty_is_one() {
        // Testnet-style genesis bootstrap: any miner can produce the
        // first block instantly. Real difficulty climbs from epoch 1.
        let d = DifficultyParams::sigmachain_testnet();
        assert_eq!(d.initial_difficulty, vec![0x01]);
    }

    #[test]
    fn sigmachain_testnet_voting_length_matches_difficulty_epoch() {
        // Aligning the voting epoch with the difficulty epoch mirrors
        // Ergo mainnet (1024 / 1024) — vote-counting boundaries land
        // on difficulty-recalc boundaries.
        let d = DifficultyParams::sigmachain_testnet();
        let v = VotingParams::sigmachain_testnet();
        assert_eq!(v.voting_length, d.epoch_length);
        assert_eq!(v.voting_length, 6_144);
    }

    #[test]
    fn sigmachain_testnet_voting_thresholds_track_voting_length() {
        let v = VotingParams::sigmachain_testnet();
        // softForkApproved boundary: votes > 6144 × 32 × 9 / 10 = 176_947.
        assert!(!v.soft_fork_approved(176_947));
        assert!(v.soft_fork_approved(176_948));
        // changeApproved boundary: count > 6144 / 2 = 3072.
        assert!(!v.change_approved(3072));
        assert!(v.change_approved(3073));
    }

    #[test]
    fn chain_spec_sigmachain_testnet_uses_20s_interval_everywhere() {
        // BlockTimingParams and DifficultyParams must agree on the
        // block interval — they are independent narrow views of the
        // same physical parameter.
        let s = ChainSpec::sigmachain_testnet();
        assert_eq!(s.block_timing.desired_interval_ms, 20_000);
        assert_eq!(s.difficulty.desired_interval_ms, 20_000);
    }

    #[test]
    fn chain_spec_testnet_difficulty_skips_v2_transition_and_uses_45s_interval() {
        // Testnet at v6.0.3+ launches with BlockVersion = Interpreter60Version
        // (= 4) at genesis, so there is no v1 → v2 hard fork; the
        // difficulty interpolator has no special-case branch to take.
        let t = ChainSpec::testnet();
        assert!(t.difficulty.v2_activation.is_none());
        assert_eq!(t.difficulty.desired_interval_ms, 45_000);
    }

    #[test]
    fn bootstrap_mainnet_has_thirteen_seeds_and_checkpoint() {
        // mainnet.conf:129-143 — 13 known peers.
        let p = BootstrapParams::mainnet();
        assert_eq!(p.seed_peers.len(), 13);
        // mainnet.conf:73-76 — checkpoint at 1_231_454 / ca5aa96a…
        let (h, id) = p.checkpoint.expect("mainnet checkpoint is set");
        assert_eq!(h, 1_231_454);
        assert_eq!(
            hex::encode(id),
            "ca5aa96a2d560f49cd5652eae4b9e16bbf410ee32365313dc16544ee5fda1e6d"
        );
    }
}
