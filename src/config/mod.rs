pub mod arbitrum;
pub mod base;
pub mod ethereum;
pub mod polygon;

pub use ethereum::PoolCatalogEntry;

use crate::types::{ArbPath, ChainId, PoolKey};

pub fn pool_catalog(chain: ChainId) -> Vec<PoolCatalogEntry> {
    match chain {
        ChainId::Ethereum => ethereum::ethereum_pools(),
        ChainId::Arbitrum => arbitrum::arbitrum_pools(),
        ChainId::Base => base::base_pools(),
        ChainId::Polygon => polygon::polygon_pools(),
    }
}

pub fn arb_paths(chain: ChainId) -> Vec<ArbPath> {
    match chain {
        ChainId::Ethereum => ethereum::ethereum_arb_paths(),
        ChainId::Arbitrum => arbitrum::arbitrum_arb_paths(),
        ChainId::Base => base::base_arb_paths(),
        ChainId::Polygon => polygon::polygon_arb_paths(),
    }
}

/// Returns the primary WETH/stable pool key and whether USDC is token0.
///
/// Used for WETH/USD price estimation and cross-chain monitoring.
/// `usdc_is_token0=true` means reserve0 is USDC (6 dec), reserve1 is WETH (18 dec).
pub fn weth_usdc_pool_info(chain: ChainId) -> Option<(PoolKey, bool)> {
    match chain {
        // token0=USDC, token1=WETH
        ChainId::Ethereum => Some((ethereum::pool_key_weth_usdc(), true)),
        // token0=WETH, token1=USDC.e
        ChainId::Arbitrum => Some((arbitrum::weth_usdc_key(), false)),
        // token0=WETH, token1=USDC
        ChainId::Base => Some((base::weth_usdc_key(), false)),
        // token0=USDC, token1=WETH
        ChainId::Polygon => Some((polygon::weth_usdc_key(), true)),
    }
}

/// Find the WETH/USDC pool in a runtime-resolved catalog and return (key, usdc_is_token0).
///
/// Used by the price helpers in main.rs after `resolve_pool_catalog` has been called.
pub fn weth_usdc_from_catalog(catalog: &[PoolCatalogEntry]) -> Option<(PoolKey, bool)> {
    catalog
        .iter()
        .find(|e| {
            (e.token0_symbol == "WETH" && e.token1_symbol == "USDC")
                || (e.token0_symbol == "USDC" && e.token1_symbol == "WETH")
        })
        .map(|e| (e.pool_key, e.token0_symbol == "USDC"))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CHAINS: [ChainId; 4] =
        [ChainId::Ethereum, ChainId::Arbitrum, ChainId::Base, ChainId::Polygon];

    #[test]
    fn test_pool_catalog_ethereum_non_empty() {
        // Ethereum, Base, and Polygon all have active pool catalogs. Arbitrum has 1 pool (price monitor only).
        assert!(!pool_catalog(ChainId::Ethereum).is_empty());
    }

    #[test]
    fn test_arb_paths_ethereum_non_empty() {
        // Arbitrum has no triangular paths (only 1 verified pool). Ethereum always should.
        assert!(!arb_paths(ChainId::Ethereum).is_empty());
    }

    #[test]
    fn test_cross_chain_pool_keys_are_unique() {
        let mut all_keys: Vec<PoolKey> = Vec::new();
        for chain in ALL_CHAINS {
            for entry in pool_catalog(chain) {
                all_keys.push(entry.pool_key);
            }
        }
        let total = all_keys.len();
        let unique: std::collections::HashSet<_> = pool_catalog(ChainId::Ethereum)
            .into_iter()
            .map(|e| e.pool_key)
            .chain(pool_catalog(ChainId::Arbitrum).into_iter().map(|e| e.pool_key))
            .chain(pool_catalog(ChainId::Base).into_iter().map(|e| e.pool_key))
            .chain(pool_catalog(ChainId::Polygon).into_iter().map(|e| e.pool_key))
            .collect();
        assert_eq!(
            unique.len(),
            total,
            "pool keys must be unique across all chains"
        );
    }

    #[test]
    fn test_weth_usdc_pool_info_returns_some_for_all_chains() {
        for chain in ALL_CHAINS {
            assert!(
                weth_usdc_pool_info(chain).is_some(),
                "weth_usdc_pool_info({}) returned None",
                chain.name()
            );
        }
    }

    #[test]
    fn test_weth_usdc_pool_info_chain_matches() {
        for chain in ALL_CHAINS {
            let (key, _) = weth_usdc_pool_info(chain).unwrap();
            assert_eq!(key.chain, chain);
        }
    }
}
