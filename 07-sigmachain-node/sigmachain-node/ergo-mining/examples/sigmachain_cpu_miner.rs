//! Minimal CPU miner for a single-node SigmaChain testnet.
//!
//! Polls `GET /mining/candidate`, runs Autolykos v2 over the returned
//! `msg` with incrementing nonces, posts the first solution that satisfies
//! `hit_for_v2(msg, n, h, calc_n(2, h)) <= target_b` back to
//! `POST /mining/solution`. Then loops.
//!
//! Not production: shells out to `curl` for the HTTP layer (so this crate
//! doesn't gain an HTTP-client dependency for what is otherwise a
//! lib-only crate), single-threaded, blocking. At testnet initial
//! difficulty one nonce sweep is well under a second; the loop is the
//! bottleneck, not the hash. For a real network use Rigel or any
//! Autolykos v2 miner — the wire protocol here is identical to upstream.
//!
//! Run:
//!   cargo run -p ergo-mining --example sigmachain_cpu_miner -- \
//!       --node http://127.0.0.1:9054 \
//!       --api-key hello \
//!       --max-blocks 10
//!
//! Both flags are optional; defaults match `ergo-node/sigmachain-testnet.toml`.

use std::process::{Command, Stdio};
use std::time::Instant;

use ergo_crypto::autolykos::common::calc_n;
use ergo_crypto::autolykos::v2::hit_for_v2;
use num_bigint::BigUint;
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut node = "http://127.0.0.1:9054".to_string();
    let mut api_key = "hello".to_string();
    let mut max_blocks: u32 = 10;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--node" => {
                node = args[i + 1].clone();
                i += 2;
            }
            "--api-key" => {
                api_key = args[i + 1].clone();
                i += 2;
            }
            "--max-blocks" => {
                max_blocks = args[i + 1].parse().expect("--max-blocks: u32");
                i += 2;
            }
            "--help" | "-h" => {
                eprintln!(
                    "usage: sigmachain_cpu_miner [--node URL] [--api-key KEY] [--max-blocks N]"
                );
                return;
            }
            other => {
                eprintln!("unknown flag: {other}");
                std::process::exit(2);
            }
        }
    }

    eprintln!(
        "sigmachain-cpu-miner: node={node} max_blocks={max_blocks}"
    );

    let mut mined = 0u32;
    let mut consecutive_empty = 0u32;
    while mined < max_blocks {
        let candidate = match fetch_candidate(&node, &api_key) {
            Some(c) => c,
            None => {
                consecutive_empty += 1;
                if consecutive_empty % 20 == 0 {
                    eprintln!(
                        "no candidate yet (attempt {consecutive_empty}); node may still be priming"
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
                continue;
            }
        };
        consecutive_empty = 0;

        let msg_hex = candidate["msg"].as_str().expect("msg");
        let msg_bytes = hex::decode(msg_hex).expect("msg hex");
        let msg: [u8; 32] = msg_bytes.as_slice().try_into().expect("msg len 32");
        let target_b = candidate["b"]
            .as_str()
            .expect("b decimal string")
            .parse::<BigUint>()
            .expect("b parse");
        let height = candidate["h"].as_u64().expect("h") as u32;
        // Header version 2 (Autolykos v2): the only thing this node mines.
        let n = calc_n(2, height);

        eprintln!(
            "mining height={height} msg={}… target_b_bits={}",
            &msg_hex[..16],
            target_b.bits()
        );

        let t0 = Instant::now();
        let mut nonce: u64 = rand_seed();
        let mut hashes = 0u64;
        loop {
            let nonce_bytes = nonce.to_be_bytes();
            let hit = hit_for_v2(&msg, &nonce_bytes, height, n);
            hashes += 1;
            if hit <= target_b {
                let elapsed = t0.elapsed();
                eprintln!(
                    "  -> found nonce {} after {} hashes in {:.2}s ({:.0} H/s)",
                    hex::encode(nonce_bytes),
                    hashes,
                    elapsed.as_secs_f64(),
                    hashes as f64 / elapsed.as_secs_f64().max(0.001),
                );
                if submit_solution(&node, &api_key, &nonce_bytes) {
                    mined += 1;
                    eprintln!("  block {mined}/{max_blocks} submitted");
                } else {
                    eprintln!("  solution rejected; refetching candidate");
                }
                break;
            }
            nonce = nonce.wrapping_add(1);
            if hashes % 200_000 == 0 {
                // Re-check candidate periodically in case the tip moved
                // (someone else mined, etc). On a single-node testnet
                // this never triggers but it's free insurance.
                if let Some(fresh) = fetch_candidate(&node, &api_key) {
                    if fresh["msg"].as_str() != Some(msg_hex) {
                        eprintln!("  candidate refreshed mid-search; restarting");
                        break;
                    }
                }
            }
        }
    }
    eprintln!("sigmachain-cpu-miner: done, mined={mined}");
}

fn fetch_candidate(node: &str, api_key: &str) -> Option<Value> {
    let out = Command::new("curl")
        .arg("-s")
        .arg("-H")
        .arg(format!("api_key: {api_key}"))
        .arg(format!("{node}/mining/candidate"))
        .stdout(Stdio::piped())
        .output()
        .expect("curl spawn");
    if !out.status.success() {
        return None;
    }
    let body = String::from_utf8(out.stdout).ok()?;
    let v: Value = serde_json::from_str(&body).ok()?;
    // The node returns a JSON object with at least msg/b/h/pk; if it
    // returns a 4xx body (object without those fields) or no candidate
    // yet (some shapes return null), treat as no-work.
    if v.get("msg").is_none() || v.get("b").is_none() {
        return None;
    }
    Some(v)
}

fn submit_solution(node: &str, api_key: &str, nonce: &[u8; 8]) -> bool {
    let body = format!("{{\"n\":\"{}\"}}", hex::encode(nonce));
    let out = Command::new("curl")
        .arg("-s")
        .arg("-o")
        .arg("/dev/null")
        .arg("-w")
        .arg("%{http_code}")
        .arg("-X")
        .arg("POST")
        .arg("-H")
        .arg(format!("api_key: {api_key}"))
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(&body)
        .arg(format!("{node}/mining/solution"))
        .output()
        .expect("curl spawn");
    let code = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if code == "200" {
        true
    } else {
        eprintln!("  /mining/solution -> HTTP {code}");
        false
    }
}

/// Cheap entropy for the starting nonce. Doesn't need cryptographic
/// quality — any non-overlapping starting point across miner restarts
/// is enough.
fn rand_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // mix with PID so two miner instances started in the same nanosecond
    // still diverge.
    t ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}
