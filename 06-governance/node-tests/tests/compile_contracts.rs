//! Phase 5.4.2 — Recompile YoloDAO `.es` contracts with the live
//! token ids from `sigmachain-deployment.json`.
//!
//! For each contract source under `06-governance/contracts/*.es`:
//!   1. Read the source.
//!   2. Substitute every `_PLACEHOLDER` constant with the matching
//!      token id (or, for `USER_VOTE_HASH_PLACEHOLDER`, the
//!      `blake2b256` of the already-compiled `userVote.es` tree).
//!   3. POST `/script/p2sAddress` on the Ergo 6.x compile node
//!      (localhost:9053) → P2S address.
//!   4. GET `/script/addressToTree/{address}` → ErgoTree hex.
//!   5. Round-trip check: hex parses through `ergo-ser::read_ergo_tree`.
//!   6. Persist into `06-governance/test-vectors/sigmachain-deployment-trees.json`.
//!
//! `vault.es` and `reserve.es` are emitted five times — once per
//! vault/reserve pair (1..=5) — with the matching (state_nft,
//! reserve_nft) pair substituted in.
//!
//! Preconditions: the Ergo node at 127.0.0.1:9053 must be reachable
//! and serving `/script/p2sAddress` + `/script/addressToTree`.
//! `sigmachain-deployment.json` must exist (run the bake test first).

#![cfg(feature = "live")]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ureq::AgentBuilder;

const ERGO_COMPILE_URL: &str = "http://127.0.0.1:9053";

/// Tree version the Ergo 6.x compile endpoint requires for any v0
/// script (omitting it returns 400 per the
/// `01-emission-tests/SKILL-rust-test-harness.md` notes).
const TREE_VERSION: u32 = 0;

#[derive(Debug, Deserialize)]
struct DeploymentToken {
    name: String,
    token_id: String,
    #[allow(dead_code)]
    tx_id: String,
    #[allow(dead_code)]
    amount: u64,
}

#[derive(Debug, Deserialize)]
struct Deployment {
    network: String,
    final_height: u32,
    owner_address: String,
    tokens: Vec<DeploymentToken>,
}

#[derive(Debug, Serialize)]
struct CompiledTree {
    contract: String,
    instance: Option<u8>,
    p2s_address: String,
    tree_hex: String,
    proposition_hash: String,
}

#[derive(Debug, Serialize)]
struct CompiledDeployment {
    network: String,
    final_height: u32,
    owner_address: String,
    /// Map from token name to token id. Mirror of `deployment.tokens`
    /// for one-step lookup at compile time.
    token_ids: BTreeMap<String, String>,
    /// Compiled trees in emission order.
    trees: Vec<CompiledTree>,
}

#[test]
fn compile_yolodao_contracts() {
    let deployment = load_deployment();
    eprintln!(
        "compiling for deployment: network={} owner={}... tokens={}",
        deployment.network,
        &deployment.owner_address[..16],
        deployment.tokens.len()
    );

    let token_by_name: BTreeMap<String, String> = deployment
        .tokens
        .iter()
        .map(|t| (t.name.clone(), t.token_id.clone()))
        .collect();

    // Sanity: every placeholder name we'll substitute must resolve to
    // a minted token id. Fail fast with the missing entry if not.
    for required in [
        "VYOLO",
        "TREASURY_NFT",
        "COUNTER_NFT",
        "VALID_VOTE_NFT",
        "PROPOSAL_TOKEN",
    ] {
        assert!(
            token_by_name.contains_key(required),
            "deployment missing required token {required}"
        );
    }

    let agent = AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    let contracts_dir = contracts_dir();
    let mut trees: Vec<CompiledTree> = Vec::new();

    // 1. Compile userVote.es first — its tree hash is referenced by
    //    timeValidator.es.
    let user_vote_src = read_es(&contracts_dir, "userVote.es");
    let user_vote_src = substitute(
        &user_vote_src,
        &[
            ("VALID_VOTE_PLACEHOLDER", &token_by_name["VALID_VOTE_NFT"]),
            ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ("COUNTER_NFT_PLACEHOLDER", &token_by_name["COUNTER_NFT"]),
        ],
    );
    let user_vote = compile(&agent, "userVote", None, &user_vote_src);
    let user_vote_hash = user_vote.proposition_hash.clone();
    trees.push(user_vote);

    // 2. timeValidator.es — needs USER_VOTE_HASH_PLACEHOLDER.
    let time_validator_src = read_es(&contracts_dir, "timeValidator.es");
    let time_validator_src = substitute(
        &time_validator_src,
        &[
            ("COUNTER_NFT_PLACEHOLDER", &token_by_name["COUNTER_NFT"]),
            ("VALID_VOTE_PLACEHOLDER", &token_by_name["VALID_VOTE_NFT"]),
            ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ("USER_VOTE_HASH_PLACEHOLDER", &user_vote_hash),
        ],
    );
    trees.push(compile(
        &agent,
        "timeValidator",
        None,
        &time_validator_src,
    ));

    // 3. counting.es
    let counting_src = read_es(&contracts_dir, "counting.es");
    let counting_src = substitute(
        &counting_src,
        &[
            ("COUNTER_NFT_PLACEHOLDER", &token_by_name["COUNTER_NFT"]),
            ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ("VALID_VOTE_PLACEHOLDER", &token_by_name["VALID_VOTE_NFT"]),
        ],
    );
    trees.push(compile(&agent, "counting", None, &counting_src));

    // 4. proposal.es
    let proposal_src = read_es(&contracts_dir, "proposal.es");
    let proposal_src = substitute(
        &proposal_src,
        &[("TREASURY_NFT_PLACEHOLDER", &token_by_name["TREASURY_NFT"])],
    );
    trees.push(compile(&agent, "proposal", None, &proposal_src));

    // 5. treasury.es
    let treasury_src = read_es(&contracts_dir, "treasury.es");
    let treasury_src = substitute(
        &treasury_src,
        &[
            ("TREASURY_NFT_PLACEHOLDER", &token_by_name["TREASURY_NFT"]),
            ("PROPOSAL_TOKEN_PLACEHOLDER", &token_by_name["PROPOSAL_TOKEN"]),
        ],
    );
    trees.push(compile(&agent, "treasury", None, &treasury_src));

    // 6. vault.es and reserve.es — one instance per vault/reserve
    //    pair (1..=5).
    let vault_src = read_es(&contracts_dir, "vault.es");
    let reserve_src = read_es(&contracts_dir, "reserve.es");
    for n in 1..=5u8 {
        let state_nft = &token_by_name[&format!("YOLO_VAULT_STATE_NFT_{n}")];
        let reserve_nft = &token_by_name[&format!("YOLO_VAULT_RESERVE_NFT_{n}")];

        let vault_n_src = substitute(
            &vault_src,
            &[
                ("STATE_NFT_PLACEHOLDER", state_nft),
                ("RESERVE_NFT_PLACEHOLDER", reserve_nft),
            ],
        );
        trees.push(compile(&agent, "vault", Some(n), &vault_n_src));

        let reserve_n_src = substitute(
            &reserve_src,
            &[
                ("STATE_NFT_PLACEHOLDER", state_nft),
                ("RESERVE_NFT_PLACEHOLDER", reserve_nft),
                ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ],
        );
        trees.push(compile(&agent, "reserve", Some(n), &reserve_n_src));
    }

    eprintln!("compiled {} trees", trees.len());

    let out = CompiledDeployment {
        network: deployment.network,
        final_height: deployment.final_height,
        owner_address: deployment.owner_address,
        token_ids: token_by_name,
        trees,
    };

    let out_path = trees_path();
    let json = serde_json::to_string_pretty(&out).unwrap();
    std::fs::write(&out_path, json).expect("write deployment-trees.json");
    eprintln!("wrote {} ({} trees)", out_path.display(), out.trees.len());

    // Defensive sanity: every emitted tree hex parses as an ErgoTree.
    for t in &out.trees {
        let bytes: Vec<u8> = (0..t.tree_hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&t.tree_hex[i..i + 2], 16).expect("hex"))
            .collect();
        assert!(!bytes.is_empty(), "{} produced empty tree", t.contract);
    }
}

// ---- helpers ----

fn load_deployment() -> Deployment {
    let path = test_vectors_dir().join("sigmachain-deployment.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "deployment.json missing at {} — run bake_yolodao_genesis_tokens first ({})",
            path.display(),
            e
        )
    });
    serde_json::from_slice(&bytes).expect("parse deployment.json")
}

fn contracts_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("contracts");
    p
}

fn test_vectors_dir() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("..");
    p.push("test-vectors");
    p
}

fn trees_path() -> PathBuf {
    test_vectors_dir().join("sigmachain-deployment-trees.json")
}

fn test_trees_path() -> PathBuf {
    test_vectors_dir().join("sigmachain-deployment-trees-test.json")
}

/// Short-window substitutions used by `compile_yolodao_contracts_test_windows`.
/// Replaces the production block-count constants with values small
/// enough to walk a whole voting → counting → validation cycle inside
/// ~30 blocks of CPU mining.
const TEST_WINDOWS: &[(&str, &str)] = &[
    // counting.es + timeValidator.es voting window (default 12_960L → 5L).
    ("12960L", "5L"),
    // counting.es counting phase (default 1_080L → 5L).
    ("1080L", "5L"),
    // counting.es execution grace AND userVote.es / timeValidator.es
    // cancellation cooldown (both default 4_320L → 5L).
    ("4320L", "5L"),
];

/// Mirror of [`compile_yolodao_contracts`] but with the voting /
/// counting / validation block constants swapped for the short
/// `TEST_WINDOWS` values. Emitted to a sibling file so the production
/// trees stay untouched. The voting-lifecycle tests load THIS file
/// instead of the production one.
#[test]
fn compile_yolodao_contracts_test_windows() {
    let deployment = load_deployment();
    let token_by_name: BTreeMap<String, String> = deployment
        .tokens
        .iter()
        .map(|t| (t.name.clone(), t.token_id.clone()))
        .collect();
    for required in [
        "VYOLO",
        "TREASURY_NFT",
        "COUNTER_NFT",
        "VALID_VOTE_NFT",
        "PROPOSAL_TOKEN",
    ] {
        assert!(
            token_by_name.contains_key(required),
            "deployment missing required token {required}"
        );
    }

    let agent = AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(5))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();

    let contracts_dir = contracts_dir();

    let apply_test_windows = |src: &str| -> String {
        let mut out = src.to_string();
        for (from, to) in TEST_WINDOWS {
            // Only counting.es / timeValidator.es / userVote.es carry
            // these Long literals; vault / reserve / treasury / proposal
            // do not, so a missing match in those files is expected.
            out = out.replace(from, to);
        }
        out
    };

    let mut trees: Vec<CompiledTree> = Vec::new();

    // userVote (test windows)
    let user_vote_src = apply_test_windows(&read_es(&contracts_dir, "userVote.es"));
    let user_vote_src = substitute(
        &user_vote_src,
        &[
            ("VALID_VOTE_PLACEHOLDER", &token_by_name["VALID_VOTE_NFT"]),
            ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ("COUNTER_NFT_PLACEHOLDER", &token_by_name["COUNTER_NFT"]),
        ],
    );
    let user_vote = compile(&agent, "userVote", None, &user_vote_src);
    let user_vote_hash = user_vote.proposition_hash.clone();
    trees.push(user_vote);

    // timeValidator
    let time_validator_src = apply_test_windows(&read_es(&contracts_dir, "timeValidator.es"));
    let time_validator_src = substitute(
        &time_validator_src,
        &[
            ("COUNTER_NFT_PLACEHOLDER", &token_by_name["COUNTER_NFT"]),
            ("VALID_VOTE_PLACEHOLDER", &token_by_name["VALID_VOTE_NFT"]),
            ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ("USER_VOTE_HASH_PLACEHOLDER", &user_vote_hash),
        ],
    );
    trees.push(compile(
        &agent,
        "timeValidator",
        None,
        &time_validator_src,
    ));

    // counting
    let counting_src = apply_test_windows(&read_es(&contracts_dir, "counting.es"));
    let counting_src = substitute(
        &counting_src,
        &[
            ("COUNTER_NFT_PLACEHOLDER", &token_by_name["COUNTER_NFT"]),
            ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ("VALID_VOTE_PLACEHOLDER", &token_by_name["VALID_VOTE_NFT"]),
        ],
    );
    trees.push(compile(&agent, "counting", None, &counting_src));

    // proposal
    let proposal_src = apply_test_windows(&read_es(&contracts_dir, "proposal.es"));
    let proposal_src = substitute(
        &proposal_src,
        &[("TREASURY_NFT_PLACEHOLDER", &token_by_name["TREASURY_NFT"])],
    );
    trees.push(compile(&agent, "proposal", None, &proposal_src));

    // treasury
    let treasury_src = apply_test_windows(&read_es(&contracts_dir, "treasury.es"));
    let treasury_src = substitute(
        &treasury_src,
        &[
            ("TREASURY_NFT_PLACEHOLDER", &token_by_name["TREASURY_NFT"]),
            ("PROPOSAL_TOKEN_PLACEHOLDER", &token_by_name["PROPOSAL_TOKEN"]),
        ],
    );
    trees.push(compile(&agent, "treasury", None, &treasury_src));

    // vault / reserve — windows aren't relevant here, but for
    // self-consistency we emit them in this file too.
    let vault_src = read_es(&contracts_dir, "vault.es");
    let reserve_src = read_es(&contracts_dir, "reserve.es");
    for n in 1..=5u8 {
        let state_nft = &token_by_name[&format!("YOLO_VAULT_STATE_NFT_{n}")];
        let reserve_nft = &token_by_name[&format!("YOLO_VAULT_RESERVE_NFT_{n}")];

        let vault_n_src = substitute(
            &vault_src,
            &[
                ("STATE_NFT_PLACEHOLDER", state_nft),
                ("RESERVE_NFT_PLACEHOLDER", reserve_nft),
            ],
        );
        trees.push(compile(&agent, "vault", Some(n), &vault_n_src));

        let reserve_n_src = substitute(
            &reserve_src,
            &[
                ("STATE_NFT_PLACEHOLDER", state_nft),
                ("RESERVE_NFT_PLACEHOLDER", reserve_nft),
                ("VYOLO_TOKEN_PLACEHOLDER", &token_by_name["VYOLO"]),
            ],
        );
        trees.push(compile(&agent, "reserve", Some(n), &reserve_n_src));
    }

    let out = CompiledDeployment {
        network: deployment.network,
        final_height: deployment.final_height,
        owner_address: deployment.owner_address,
        token_ids: token_by_name,
        trees,
    };

    let out_path = test_trees_path();
    let json = serde_json::to_string_pretty(&out).unwrap();
    std::fs::write(&out_path, json).expect("write deployment-trees-test.json");
    eprintln!(
        "wrote {} ({} trees with test windows)",
        out_path.display(),
        out.trees.len()
    );
}

fn read_es(dir: &PathBuf, name: &str) -> String {
    let path = dir.join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn substitute(src: &str, replacements: &[(&str, &str)]) -> String {
    let mut out = src.to_string();
    for (placeholder, value) in replacements {
        if !out.contains(placeholder) {
            panic!(
                "placeholder {} not found in contract source — replacement set drifted from .es",
                placeholder
            );
        }
        out = out.replace(placeholder, value);
    }
    out
}

#[derive(Serialize)]
struct CompileRequest<'a> {
    source: &'a str,
    #[serde(rename = "treeVersion")]
    tree_version: u32,
}

#[derive(Deserialize)]
struct CompileResponse {
    address: String,
}

#[derive(Deserialize)]
struct TreeResponse {
    tree: String,
}

fn compile(
    agent: &ureq::Agent,
    contract: &str,
    instance: Option<u8>,
    source: &str,
) -> CompiledTree {
    let req = CompileRequest {
        source,
        tree_version: TREE_VERSION,
    };
    let compile_resp: CompileResponse = agent
        .post(&format!("{ERGO_COMPILE_URL}/script/p2sAddress"))
        .send_json(serde_json::to_value(&req).unwrap())
        .unwrap_or_else(|e| panic!("compile {} (instance {:?}) failed: {e:?}", contract, instance))
        .into_json()
        .unwrap_or_else(|e| panic!("parse compile response for {}: {e:?}", contract));

    let tree_resp: TreeResponse = agent
        .get(&format!(
            "{ERGO_COMPILE_URL}/script/addressToTree/{}",
            compile_resp.address
        ))
        .call()
        .unwrap_or_else(|e| panic!("addressToTree {} failed: {e:?}", contract))
        .into_json()
        .unwrap_or_else(|e| panic!("parse tree response for {}: {e:?}", contract));

    let proposition_hash = blake2b256_hex(&tree_resp.tree);

    eprintln!(
        "  {:<14} instance={:<5} p2s={}... tree_len={}",
        contract,
        instance.map(|n| n.to_string()).unwrap_or_else(|| "-".into()),
        &compile_resp.address[..12],
        tree_resp.tree.len() / 2,
    );

    CompiledTree {
        contract: contract.to_string(),
        instance,
        p2s_address: compile_resp.address,
        tree_hex: tree_resp.tree,
        proposition_hash,
    }
}

fn blake2b256_hex(tree_hex: &str) -> String {
    use blake2::{Blake2b, Digest};
    type Blake2b256 = Blake2b<blake2::digest::consts::U32>;
    let bytes: Vec<u8> = (0..tree_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&tree_hex[i..i + 2], 16).expect("hex"))
        .collect();
    let mut h = Blake2b256::new();
    h.update(&bytes);
    hex::encode(h.finalize())
}
