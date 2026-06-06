# Phase 5.4 — Live-node test setup

Operator-side ceremony. None of these steps run from `cargo test`; they
are state-changing actions on the local SigmaChain testnet that you do
once per fresh data dir. After all steps below succeed, the live test
suite can run via `cargo test --features live`.

## Prereqs

- `07-sigmachain-node/sigmachain-node/ergo-node/sigmachain-testnet.toml`
  in place (already committed).
- `b2sum` available (coreutils) for the api_key hash check.
- Empty or known-state node data dir.

## 1. Decide and pin the node API key

The toml ships with a placeholder `api_key_hash = b2sum-256("hello")`.
For a dev testnet that is fine. If you want a different key, generate the
hash and replace the value in the toml before booting:

```sh
printf 'YOUR_KEY' | b2sum -l 256
```

Record the cleartext API key — you will export it as
`YOLO_NODE_API_KEY` later. Do not commit it.

## 2. Boot the node

```sh
cd 07-sigmachain-node/sigmachain-node
cargo run -p ergo-node -- --config ergo-node/sigmachain-testnet.toml
```

Verify it is alive (in another terminal):

```sh
curl -s http://127.0.0.1:9054/info | jq '.fullHeight, .name, .network'
```

`fullHeight` should be the genesis height (0 on a fresh dir) or wherever
the data dir was left.

## 3. Restore the wallet (one-shot)

The mnemonic + password were generated outside this repo. Do this in a
shell where the values are pasted directly into the curl body — they
must not flow through any committed file or this README. After it
completes, the wallet is persisted on disk and you only need the
password for subsequent unlocks.

Use a heredoc, not echo / printf with interpolation, to keep the
mnemonic off the process arg list:

```sh
curl -s -X POST http://127.0.0.1:9054/wallet/restore \
  -H 'api_key: YOUR_API_KEY' \
  -H 'Content-Type: application/json' \
  --data @- <<'JSON'
{
  "mnemonic": "PASTE MNEMONIC HERE",
  "pass": "PASTE PASSWORD HERE",
  "usePre1627KeyDerivation": false
}
JSON
```

Confirm:

```sh
curl -s -H 'api_key: YOUR_API_KEY' http://127.0.0.1:9054/wallet/status | jq
```

`isInitialized: true`. `changeAddress` should match the address the
wallet was generated to produce.

## 4. Unlock the wallet

```sh
curl -s -X POST http://127.0.0.1:9054/wallet/unlock \
  -H 'api_key: YOUR_API_KEY' \
  -H 'Content-Type: application/json' \
  --data @- <<'JSON'
{ "pass": "PASTE PASSWORD HERE" }
JSON
```

`/wallet/status` should now show `isUnlocked: true` and a non-empty
`changeAddress`.

## 5. Start the CPU miner past the reward-delay gate

SigmaChain's `MonetaryParams::sigmachain_testnet::miner_reward_delay` is
4,320 blocks (`ergo-chain-spec/src/lib.rs:422`). Until the chain passes
that height, the wallet's miner reward boxes are not spendable. Mine
4,320+ blocks in one shot:

```sh
cargo run -p ergo-mining --example sigmachain_cpu_miner -- \
  --node http://127.0.0.1:9054 \
  --api-key YOUR_API_KEY \
  --max-blocks 4400
```

Budget ~25 minutes at testnet initial difficulty. The miner uses the
unlocked wallet's miner pubkey for the coinbase reward by default
(via the node's `/mining/candidate` builder).

Watch progress:

```sh
watch -n 5 'curl -s http://127.0.0.1:9054/info | jq ".fullHeight"'
```

When `fullHeight >= 4320`, check the wallet has spendable balance:

```sh
curl -s -H 'api_key: YOUR_API_KEY' http://127.0.0.1:9054/wallet/balances | jq
```

`balance` should be > 0. If it is 0, the wallet was not unlocked when
those blocks were mined, OR `wallet_height` is lagging — wait for the
wallet scanner to catch up.

## 6. Configure local env for the test suite

```sh
cd 06-governance/node-tests
cp .env.example .env
$EDITOR .env   # fill in YOLO_NODE_API_KEY and YOLO_WALLET_PASSWORD
```

`.env` is gitignored at the repo root.

Source and run the smoke test:

```sh
set -a; source .env; set +a
cargo test --features live live_node_single_tx_round_trip -- --nocapture --test-threads=1
```

For the round-trip to mine within the test's timeout window, the CPU
miner must be running in another terminal. The simplest pattern is to
keep `--max-blocks 100000` running for the entire test session.

## When to re-run the setup

- Fresh data dir → all six steps.
- Existing data dir, wallet already restored → steps 2 + 4 + 5 (mining
  resumes from current height; no need to re-cross the maturity gate).
- Just re-running tests in the same shell → step 6.
