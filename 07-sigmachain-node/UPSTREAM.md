# Upstream tracking — `sigmachain-node`

`sigmachain-node/` is a fork of [arkadianet/ergo](https://github.com/arkadianet/ergo). Its files are tracked directly inside this `yolo-chain` repo (no submodule, no subtree), so the inner `.git` of the original clone has been removed.

## Fork point

| | |
|---|---|
| Upstream repo | https://github.com/arkadianet/ergo |
| Upstream HEAD at fork | `1a68cd9b530756f1aacaa512e9e537577982c91c` |
| Nearest tag | `v0.3.0` (4 small commits earlier — CI fixes + a p2p `DeliveryTracker` shadow-type fix) |
| Fork date | 2026-06-04 |

## How to pull upstream fixes

There is no `git remote upstream` configured on this fork. To bring in an upstream fix:

```bash
# 1. Get a fresh upstream snapshot somewhere out-of-tree
cd /tmp
git clone https://github.com/arkadianet/ergo upstream-fresh
cd upstream-fresh
git log --oneline 1a68cd9b...HEAD   # see what's new since fork point

# 2. Diff a single file or path against our fork
diff -u \
  /tmp/upstream-fresh/<path-to-file> \
  /home/cq/working-files/yolo-chain/07-sigmachain-node/sigmachain-node/<path-to-file>

# 3. Apply manually, or cherry-pick via patch:
cd /tmp/upstream-fresh
git format-patch -1 <upstream-commit-sha>
cd /home/cq/working-files/yolo-chain
git apply --3way /tmp/upstream-fresh/<patch>.patch
```

## Local diff reference (optional)

If you want to keep a pristine arkadianet/ergo clone around for fast diffs without re-cloning each time, drop it at:

```
07-sigmachain-node/arkadianet-ergo-reference/
```

That path is `.gitignored` — it's local-only and won't be committed. Re-create it with:

```bash
cd 07-sigmachain-node
git clone https://github.com/arkadianet/ergo arkadianet-ergo-reference
```

## SigmaChain-specific divergences

See [overall-documents/sigmachain-parameter-inventory.md](../overall-documents/sigmachain-parameter-inventory.md) for the per-parameter inventory of where we diverge from upstream.
