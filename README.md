# Arb Scanner

[![CI](https://github.com/ianfigueroa/Arb-Scanner/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/ianfigueroa/Arb-Scanner/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A read-only arbitrage scanner that watches DEX pools across multiple chains and logs price discrepancies worth studying. It does not execute trades: no keys, no bundles, no transaction submission. This is a research and monitoring tool for on-chain pricing.

## What It Does

Every block, the scanner evaluates curated triangular paths such as `WETH -> USDC -> DAI -> WETH` on each enabled chain. When the estimated round trip is positive after gas, it logs the event and stores it in SQLite. It also records per-block WETH/USD reference prices and raises cross-chain spread alerts when prices drift beyond a configured threshold.

Supported chains:
- Ethereum
- Arbitrum
- Base
- Polygon

Supported venues:
- Ethereum: Uniswap V2, Uniswap V3 0.05%, Uniswap V3 0.3%, Curve 3pool for pricing context
- Arbitrum: SushiSwap V2
- Base: BaseSwap V2
- Polygon: QuickSwap V2

Input sizes scanned per path:
- `0.1`
- `0.5`
- `1`
- `5`
- `10` ETH

Pool addressing:
- Ethereum and Arbitrum use verified static pool addresses.
- Base and Polygon resolve V2 pair addresses from their factory contracts at startup so the scanner uses canonical live pool addresses instead of stale hardcoded values.

## Quick Start

### 1. Copy the environment file

```bash
cp .env.example .env
```

### 2. Add at least one WebSocket RPC URL

Example:

```dotenv
ETH_WS_URL=wss://eth-mainnet.g.alchemy.com/v2/your_key
ARBITRUM_WS_URL=wss://arb-mainnet.g.alchemy.com/v2/your_key
BASE_WS_URL=wss://base-mainnet.g.alchemy.com/v2/your_key
POLYGON_WS_URL=wss://polygon-mainnet.g.alchemy.com/v2/your_key

CROSS_CHAIN_THRESHOLD_PCT=0.1
```

Any chain without a URL is skipped automatically.

### 3. Run the scanner

```bash
cargo run
```

Stop it with `Ctrl+C`.

### 4. Analyze the latest session

```bash
pip install -r analysis/requirements.txt
python analysis/analyze.py arb_opportunities.db
```

By default, the analysis script selects the latest recorded session and writes charts into `analysis/output/<session_id>/`.

## Reading The Logs

Startup:

```text
INFO arb_bot: database opened path="arb_opportunities.db"
INFO arb_bot: session started session_id="session-1761332485123" active_chains="ethereum,arbitrum,base,polygon"
INFO arb_bot: pool catalog resolved chain="ethereum" pools=8
INFO arb_bot: reserves bootstrapped chain="ethereum"
INFO arb_bot::pools: subscribed to pool events chain="ethereum"
```

Per block:

```text
INFO arb_bot: WETH: $2150.50 chain="ethereum" block=24708245 gas_gwei="0.04"
INFO arb_bot: WETH: $2149.38 chain="arbitrum" block=444231730 gas_gwei="0.02"
```

Cross-chain alert:

```text
WARN arb_bot::cross_chain: [CROSS-CHAIN ALERT] ethereum=$2150.50 arbitrum=$2149.38 base=$2154.28 spread="0.2281%"
```

Opportunity log:

```text
INFO arb_bot: [OPPORTUNITY] chain="ethereum" path="WETH→USDC→DAI→WETH" roi_pct="0.0023" gas_cost_usd="1.40"
```

Shutdown summary:

```text
=== Session Summary ===
Session ID:           session-1761332485123
Blocks scanned:       312
Opportunities found:  7
Best opportunity:
  Chain:                ethereum
  Path:                 WETH→USDC→DAI→WETH
  Input:                1.0000 ETH
  Estimated net (wei):  1234567890000000
  ROI:                  0.0031%
  Gas cost:             $1.52
Runtime:              3721.4s
======================
```

All runs write into `arb_opportunities.db`. The database is now session-aware:
- each scanner start creates a `sessions` row
- `opportunities` rows are tagged with `session_id`
- `price_snapshots` rows are tagged with `session_id`

## Session-Based Analysis

Analyze the latest session:

```bash
python analysis/analyze.py arb_opportunities.db
```

Analyze one specific session:

```bash
python analysis/analyze.py arb_opportunities.db --session session-1761332485123
```

List available sessions:

```bash
python analysis/analyze.py arb_opportunities.db --list-sessions
```

Analyze the full historical database instead of one session:

```bash
python analysis/analyze.py arb_opportunities.db --all
```

Output locations:
- latest or explicit session: `analysis/output/<session_id>/`
- aggregate mode: `analysis/output/all/`

Charts produced:
- `roi_distribution.png`
- `opportunities_timeline.png`
- `path_frequency.png`
- `gas_vs_profit.png`
- `weth_price.png`

`weth_price.png` is the chart that should correlate most directly with the live block logs, because both are based on `price_snapshots`.

Sample chart from example data:

![ROI distribution example](docs/example-roi-distribution.png)

This sample image is documentation-only. It is not proof of live profitability from your current database.

## Architecture

High-level component notes live in [docs/architecture.md](docs/architecture.md).

Code layout:

```text
src/
├── main.rs
├── types.rs
├── pools.rs
├── arb.rs
├── cross_chain.rs
├── db.rs
├── config/
└── dex/

analysis/
├── analyze.py
├── requirements.txt
└── test_analyze.py
```

## Design Tradeoffs

- The scanner is read-only by design. That keeps scope honest and removes private-key and execution risk.
- SQLite is used because it is simple to inspect locally and works well for iterative research runs.
- Pool coverage is curated rather than exhaustive. That keeps startup and maintenance manageable, but it is not a full DEX search surface.
- Uniswap V3 quoting uses marginal price from `sqrtPriceX96`, which is fast and useful for monitoring but not a full execution simulation.
- The Python analysis layer is intentionally separate from the Rust runtime so post-run reporting stays easy to modify without touching the scanner core.

## Known Limitations

- Positive ROI in logs is an estimate, not executable profit.
- The scanner does not submit transactions or compete with real MEV searchers.
- V3 quotes do not model full price impact across liquidity ranges.
- Historical sessions are preserved in one SQLite file; this is convenient, but not the right long-term storage format for high-volume research.
- Observability is log-and-SQLite based. There is no metrics backend, dashboard service, or alert delivery outside the terminal yet.

## Roadmap

- Add explicit session filtering and comparison views to the Python charts.
- Add better observability, including structured metrics and richer stale-pool diagnostics.
- Expand path coverage and improve quote accuracy for deeper research runs.
- Evaluate an `alloy-rs` migration if the runtime grows beyond the current `ethers-rs` footprint.
- Cut formal GitHub releases starting with `v0.1.0`.

## Tests

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
python -m unittest analysis.test_analyze
```

Verified locally:
- `cargo test`: 85 passing tests
- `python -m unittest analysis.test_analyze`: 7 passing tests
