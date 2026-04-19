# $YOLO — A New GPU-Mineable Chain with Real DeFi from Day One

## What is this?

A new standalone blockchain that miners can point their GPUs at and start earning coins immediately. Unlike most new chains that launch with nothing but a token and a promise, this one launches with a full suite of decentralized finance tools — trading, lending, stablecoins, options — ready to use from the first block.

The chain uses ErgoScript, the same smart contract language that powers the Ergo blockchain. That means every financial application that works on Ergo today can be deployed on this chain with minimal changes. Years of battle-tested DeFi infrastructure, available at genesis.

## Why does this matter?

GPU miners are running out of places to mine. Ethereum moved to proof of stake. The remaining GPU chains either have no smart contracts (so there's nothing to do with the coins except sell them) or they're EVM clones competing for the same shrinking pool of users.

This chain offers something different: mine a coin that has actual utility from day one. Decentralized exchanges, lending markets, options trading, stablecoins — all on-chain, all permissionless, all inherited from Ergo's proven contract library.

And through existing bridge infrastructure, the mined token can reach Ethereum, Cosmos, and other major chains almost immediately. Miners aren't stuck on an island waiting for exchange listings. Liquidity exists from the start.

## What makes it different?

**No premine. No VC allocation. No team tokens.** Every coin comes from mining. 85% of block rewards go to miners, 10% to a development treasury governed by on-chain multisig with timelocks, and 5% to a liquidity fund that seeds trading pools. All of this is enforced by smart contracts, not promises.

**Aggressive storage rent.** Coins sitting untouched in a wallet for 12 months start paying a small fee that goes directly to miners. This does two things: it gives miners a permanent income stream beyond block rewards, and it pushes people to actually use the DeFi instead of just hoarding. The cost is negligible for active users — their boxes reset every time they transact. It only affects truly dormant holdings, and even then a 1-coin balance survives over 12 years before being fully consumed.

**Miner-only rent collection.** Unlike Ergo, where anyone can claim storage rent from dormant wallets, this chain restricts rent collection to the miner who produces the block. This is a consensus rule, not a suggestion. Miners do the work of securing the chain; miners get the rent. No bots front-running them.

**Fast blocks.** 20-second block times mean transactions confirm in under a minute. Six confirmations in two minutes. That's fast enough to watch a swap execute in real time.

## What's been built so far?

The monetary system is complete and tested:

- **Emission contract** — controls how every coin enters existence. 50 coins per block, halving annually, with a 1-coin tail emission that ensures miners always have incentive. 19 tests passing against the actual Rust interpreter the chain will run on.

- **Treasury contract** — receives 10% of block rewards. Spending requires a proposal, multisig approval from multiple trusted community members, and a mandatory waiting period before execution. Designed to prevent any single person from raiding the development fund.

- **LP fund contract** — receives 5% of block rewards. Can only be spent on liquidity pools and bridge liquidity — this restriction is enforced by the contract itself, not by governance norms. The fund exists solely to make the token tradeable.

- **Integration testing** — all three contracts proven working together in a simulated multi-block sequence, including across the first halving boundary where the reward drops from 50 to 25 coins. The full pipeline from genesis through ongoing emission is verified.

- **Storage rent economic model** — 274-line simulation covering 560 parameter combinations. The recommended parameters match Ergo's proven annual cost rate but with 4x faster cleanup. Seven sensitivity charts produced. Plain-language holder impact guide completed.

All contract work was done in Rust against sigma-rust 0.28, the same contract interpreter the chain will use in production. No untested code, no Scala dependencies, no "it should work" assumptions.

## What's next?

**Conversations, not code.** The core economic contracts are done. The next milestones are:

1. **Bridge integration scoping** — coordinating with the team building AEther, a bridge protocol connecting Ergo to Cosmos and EVM chains. This is what enables the token to reach Ethereum DEXes and Cosmos liquidity from early on. AEther has already been tested moving assets between Ergo mainnet and Cosmos/Ethereum testnets.

2. **Rust node developer engagement** — two independent developers are currently building Ergo nodes in Rust and are close to stable releases. Forking one of those nodes, swapping the mining algorithm, and applying the new chain's parameters is the engineering work that produces the actual blockchain. The contracts are ready and waiting for the node.

3. **Mining infrastructure research** — documenting the stratum protocol so miners can connect their existing GPU mining software to the new chain without custom tools.

4. **Mining profitability calculator** — a web tool where miners can plug in their GPU model and see estimated earnings. Launches alongside the chain.

5. **DeFi deployment planning** — auditing existing Ergo DeFi contracts (AMM, lending, options) for any parameter changes needed at the new chain's faster block time. The contracts themselves are identical; only time-based constants need recalculation.

## The honest assessment

This is an experiment. The contracts are real, the economics are modeled, the bridge infrastructure exists, and the node technology is nearly ready. But no community has formed yet, no miners are committed, and the chain doesn't exist. If it catches, it would be the first ErgoScript fork — a GPU-mineable chain with more functional DeFi at launch than most chains have after years of development. If it doesn't, every line of code is open source and nothing was lost.

No promises. No hype. Just working code and a fair launch.
