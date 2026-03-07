# arb-bot

Scan-only triangular arbitrage scanner for Uniswap V2 on Ethereum mainnet.

Monitors WETH/USDC, USDC/DAI, and DAI/WETH pools via WebSocket `Sync` events,
calculates forward and reverse triangular paths, estimates gas cost, and logs
profitable-looking opportunities. **No execution. No private keys. No flashbots.**

---

## Setup

### 1. Get a free Alchemy WebSocket URL

Sign up at [alchemy.com](https://www.alchemy.com), create an Ethereum mainnet app,
and copy your WebSocket URL (`wss://eth-mainnet.g.alchemy.com/v2/YOUR_KEY`).

### 2. Configure

```bash
cp .env.example .env
# Edit .env and replace YOUR_KEY with your actual Alchemy key
```

### 3. Run

```bash
cargo run
```

---

## What the output means

**On startup:**
```
token ordering verified  pool=WETH/USDC token0=USDC token1=WETH
bootstrapped reserves    pool=WETH/USDC reserve0=... reserve1=...
```
Confirms on-chain token ordering matches our config. Hard-exits on mismatch.

**Every ~12 seconds (per block):**
```
[block 19_000_000] gas_gwei=15.50  WETH/USDC: $3000.42 | USDC/DAI: 1.0002 | DAI/WETH: 2999.88
```

**When a scan finds a theoretical opportunity:**
```
[OPPORTUNITY] path=WETH→USDC→DAI→WETH input_eth=1.0000 roi_pct=0.0012
              estimated_net_after_gas_wei=45000000000000 gas_cost_usd=1.23
```

**On Ctrl+C:**
```
=== Session Summary ===
Blocks scanned:       42
Opportunities found:  3
Best opportunity:
  Path:                 WETH→USDC→DAI→WETH
  Input:                1.0000 ETH
  Estimated net (wei):  45000000000000
  ROI:                  0.0012%
  Gas cost:             $1.23
Runtime:              510.4s
======================
```

---

## Scanned paths

| Direction | Path |
|-----------|------|
| Forward   | WETH → USDC → DAI → WETH |
| Reverse   | WETH → DAI → USDC → WETH |

Each path is scanned at 5 fixed input sizes: **0.1, 0.5, 1, 5, 10 ETH**.

---

## Honest caveats

**Fixed input sizes are a coarse scan heuristic, not an optimizer.**
The bot checks whether any of 5 predetermined sizes shows a positive net after gas.
It does not search for an optimal trade size or account for price impact beyond the
basic constant-product formula.

**`estimated_net_after_gas` is theoretical.**
Actual capturable profit depends on execution speed, MEV competition (searchers
submitting the same arb in the same block), on-chain slippage, and the gas price
at inclusion time. A positive `estimated_net_after_gas` does not mean the trade
would be profitable to execute.

**ethers-rs 2.x is in maintenance mode.**
This project pins `ethers = "2"` as a deliberate MVP choice for a stable,
well-documented API. The successor library is [alloy-rs](https://github.com/alloy-rs/alloy).
Migrating to alloy would be the recommended path for any production continuation.

**No execution path exists.**
This binary has no signing key, no transaction construction, and no flashbots
integration. It is a read-only scanner.

---

## Architecture

```
main.rs      tokio runtime, startup sequence, Ctrl+C, session summary
config.rs    pool address table (WETH/USDC, USDC/DAI, DAI/WETH) with token ordering
types.rs     PoolId, PoolState, ArbOpportunity, SessionStats
pools.rs     PoolRegistry (RwLock), on-chain verify, bootstrap, Sync subscriptions, stale refresh
arb.rs       amm_out, forward/reverse path calculation, freshness guard, gas estimation
```

### Token ordering (verified on-chain at startup)

| Pool | Pair address | token0 | token1 |
|------|-------------|--------|--------|
| WETH/USDC | 0xB4e16d... | USDC (6 dec) | WETH (18 dec) |
| USDC/DAI  | 0xAE461c... | DAI (18 dec) | USDC (6 dec) |
| DAI/WETH  | 0xA478c2... | DAI (18 dec) | WETH (18 dec) |

Reserve math operates in raw token units per hop — no global decimal
normalization. Only display values (USD prices) are normalized.

---

## Running tests

```bash
cargo test
```

12 unit tests cover: AMM formula correctness, zero/overflow guards, freshness guard
(stale pools return None), forward vs reverse path difference, 6-decimal USDC chain,
and a fixture snapshot against hand-calculated values.
