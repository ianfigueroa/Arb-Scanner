use ethers::types::Address;

use crate::config::PoolCatalogEntry;
use crate::types::{ArbPath, ChainId, DexType, HopSpec, PoolKey};

fn addr(s: &str) -> Address {
    s.parse().expect("invalid address in polygon config")
}

// ─── Token addresses ──────────────────────────────────────────────────────────

pub fn weth() -> Address {
    addr("0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619")
}
pub fn usdc() -> Address {
    addr("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174")
}
pub fn dai() -> Address {
    addr("0x8f3Cf7ad23Cd3CaDbD9735AFf958023239c6A063")
}

// ─── Pool keys ────────────────────────────────────────────────────────────────

/// QuickSwap V2 — WETH/USDC (token0=USDC, token1=WETH; 0x2791 < 0x7ceB)
pub fn pool_key_weth_usdc() -> PoolKey {
    PoolKey::new(ChainId::Polygon, addr("0x853Ee4b2A13f8a742d64C8F088bE7bA2131f670d"))
}
/// QuickSwap V2 — USDC/DAI (token0=USDC, token1=DAI; 0x2791 < 0x8f3C)
pub fn pool_key_usdc_dai() -> PoolKey {
    PoolKey::new(ChainId::Polygon, addr("0xf04adBF75cDFc5eD26eeA4bbbb991DB002036Bdd"))
}
/// QuickSwap V2 — WETH/DAI (token0=WETH, token1=DAI; 0x7ceB < 0x8f3C)
pub fn pool_key_weth_dai() -> PoolKey {
    PoolKey::new(ChainId::Polygon, addr("0x4A35582a710E1F4b2030A3F826DA20BfB6703C09"))
}

/// Primary WETH/stable pair used for price display and cross-chain monitoring.
pub fn weth_usdc_key() -> PoolKey {
    pool_key_weth_usdc()
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────
//
// Token address ordering (numerically):
//   USDC (0x2791) < WETH (0x7ceB) < DAI (0x8f3C)
//
// | Pool           | token0 | token1 |
// |----------------|--------|--------|
// | WETH/USDC QS   | USDC   | WETH   |
// | USDC/DAI  QS   | USDC   | DAI    |
// | WETH/DAI  QS   | WETH   | DAI    |
pub fn polygon_pools() -> Vec<PoolCatalogEntry> {
    vec![
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdc(),
            dex_type: DexType::UniswapV2,
            expected_token0: usdc(),
            expected_token1: weth(),
            token0_symbol: "USDC",
            token1_symbol: "WETH",
            name: "WETH/USDC QuickSwap",
        },
        PoolCatalogEntry {
            pool_key: pool_key_usdc_dai(),
            dex_type: DexType::UniswapV2,
            expected_token0: usdc(),
            expected_token1: dai(),
            token0_symbol: "USDC",
            token1_symbol: "DAI",
            name: "USDC/DAI QuickSwap",
        },
        PoolCatalogEntry {
            pool_key: pool_key_weth_dai(),
            dex_type: DexType::UniswapV2,
            expected_token0: weth(),
            expected_token1: dai(),
            token0_symbol: "WETH",
            token1_symbol: "DAI",
            name: "WETH/DAI QuickSwap",
        },
    ]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────

pub fn polygon_arb_paths() -> Vec<ArbPath> {
    vec![
        // Forward: WETH → USDC → DAI → WETH
        ArbPath {
            name: "WETH→USDC→DAI→WETH",
            chain: ChainId::Polygon,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdc(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: usdc(),
                },
                HopSpec {
                    pool_key: pool_key_usdc_dai(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdc(),
                    token_out: dai(),
                },
                HopSpec {
                    pool_key: pool_key_weth_dai(),
                    dex_type: DexType::UniswapV2,
                    token_in: dai(),
                    token_out: weth(),
                },
            ],
        },
        // Reverse: WETH → DAI → USDC → WETH
        ArbPath {
            name: "WETH→DAI→USDC→WETH",
            chain: ChainId::Polygon,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_dai(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: dai(),
                },
                HopSpec {
                    pool_key: pool_key_usdc_dai(),
                    dex_type: DexType::UniswapV2,
                    token_in: dai(),
                    token_out: usdc(),
                },
                HopSpec {
                    pool_key: pool_key_weth_usdc(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdc(),
                    token_out: weth(),
                },
            ],
        },
    ]
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polygon_catalog_has_three_entries() {
        assert_eq!(polygon_pools().len(), 3);
    }

    #[test]
    fn test_polygon_pool_keys_all_use_polygon_chain() {
        for e in polygon_pools() {
            assert_eq!(e.pool_key.chain, ChainId::Polygon);
        }
    }

    #[test]
    fn test_polygon_arb_paths_have_two_entries() {
        assert_eq!(polygon_arb_paths().len(), 2);
    }

    #[test]
    fn test_polygon_arb_paths_each_have_three_hops() {
        for p in polygon_arb_paths() {
            assert_eq!(p.hops.len(), 3);
        }
    }

    #[test]
    fn test_polygon_arb_paths_chain_matches() {
        for p in polygon_arb_paths() {
            assert_eq!(p.chain, ChainId::Polygon);
        }
    }

    #[test]
    fn test_polygon_weth_usdc_key_uses_polygon_chain() {
        assert_eq!(weth_usdc_key().chain, ChainId::Polygon);
    }

    #[test]
    fn test_polygon_all_hop_pool_keys_use_polygon_chain() {
        for path in polygon_arb_paths() {
            for hop in &path.hops {
                assert_eq!(hop.pool_key.chain, ChainId::Polygon);
            }
        }
    }
}
