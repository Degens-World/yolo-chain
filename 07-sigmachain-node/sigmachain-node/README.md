# SigmaChain Node

A Rust full node for **SigmaChain**, the YoloDAO-governed chain that runs ErgoScript contracts with a custom emission curve, miner-only storage rent, and 20-second blocks.

This node is a fork of [arkadianet/ergo](https://github.com/arkadianet/ergo) v0.3.0+. The contract layer, sigma interpreter, box model, and consensus framework are inherited unchanged from upstream; SigmaChain only diverges where it has to.

> ⚠️ **Pre-1.0 / pre-mainnet.** This node is for testnet and contract integration work only. Do not use it for production funds.

## What's different from Ergo

| Area | Ergo | SigmaChain |
|---|---|---|
| Block time | 120 s mainnet / 45 s testnet | **20 s** |
| Emission curve | 75 ERG linear reduction | **50 → 25 → 12.5 → … → 1 YOLO geometric halving** with tail emission floor |
| Per-block split | 90% miner + 10% founders (initial window) | **85% miner + 10% treasury + 5% LP fund** |
| Storage rent cycle | 4 years | **1 year** |
| Storage rent collection | anyone | **miner-only (consensus rule)** |
| Network magic | `[1, 0, 2, 4]` mainnet / `[2, 3, 2, 3]` testnet | `[0x59, 0x4F, 0x4C, 0x4F]` (ASCII "YOLO") testnet |
| Address prefix | `0x00` mainnet / `0x10` testnet | `0x20` testnet |
| Default P2P port | 9030 mainnet / 9023 testnet | **9120** testnet |
| Default REST port | 9053 (Scala) / 9099 (arkadianet) | **9153** testnet |

Full parameter inventory: [overall-documents/sigmachain-parameter-inventory.md](../../overall-documents/sigmachain-parameter-inventory.md).

What stays identical to Ergo: every consensus structure that matters for ErgoScript — box model, sigma proofs, ErgoTree interpreter, AVL+ state, transaction format, NiPoPoW. The YoloDAO governance contracts, treasury contracts, LP contracts, and emission contract written for [yolo-chain](https://github.com/Degens-World/yolo-chain) deploy on this node without modification.

## Build

Requires Rust 1.95.0 (pinned via `rust-toolchain.toml` — `rustup` installs automatically on first build).

```bash
cargo build --release -p ergo-node -p ergo-wallet
```

The binary is `target/release/sigmachain-node` (the crate is still named `ergo-node` internally so upstream merges from arkadianet stay clean).

## Run

(Configuration is still in progress while parameter changes from the inventory get applied. See parent project [yolo-chain README](../../README.md) for the broader picture.)

## Repository structure

- `07-sigmachain-node/arkadianet-ergo-reference/` (sibling dir) — pristine clone of upstream at the commit we forked from, kept for diff/reference.
- `07-sigmachain-node/sigmachain-node/` (this dir) — the SigmaChain fork. `upstream` remote tracks `arkadianet/ergo`; `sigmachain-main` is the working branch.

To pull upstream consensus fixes:

```bash
git fetch upstream
git merge upstream/main   # resolve conflicts in chain-spec / mining / validation
```

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), matching upstream. SigmaChain modifications: see [NOTICE](NOTICE).

## See also

- [arkadianet/ergo](https://github.com/arkadianet/ergo) — upstream Rust Ergo node
- [ergoplatform/ergo](https://github.com/ergoplatform/ergo) — Scala reference node (consensus oracle)
- [Degens-World/yolo-chain](https://github.com/Degens-World/yolo-chain) — YOLO contracts, emission/treasury/LP/governance specs
- [overall-documents/RUST-NODE-FORK-HANDOFF.md](../../overall-documents/RUST-NODE-FORK-HANDOFF.md) — the fork plan
- [overall-documents/sigmachain-parameter-inventory.md](../../overall-documents/sigmachain-parameter-inventory.md) — every changed parameter
