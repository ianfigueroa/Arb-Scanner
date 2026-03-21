use ethers::types::Address;

use crate::config::PoolCatalogEntry;
use crate::types::{ArbPath, ChainId, DexType, PoolKey};

fn addr(s: &str) -> Address {
    s.parse().expect("invalid address in arbitrum config")
}

// ─── Token addresses ──────────────────────────────────────────────────────────

pub fn weth() -> Address {
    addr("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1")
}
pub fn usdc_e() -> Address {
    addr("0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8")
}

// ─── Pool keys ────────────────────────────────────────────────────────────────

/// SushiSwap V2 — WETH/USDC.e (token0=WETH, token1=USDC.e; 0x82aF < 0xFF97)
pub fn pool_key_weth_usdc_e() -> PoolKey {
    PoolKey::new(ChainId::Arbitrum, addr("0x905dfCD5649217c42684f23958568e533C711Aa3"))
}

/// Primary WETH/stable pair used for price display and cross-chain monitoring.
#[cfg_attr(not(test), allow(dead_code))]
pub fn weth_usdc_key() -> PoolKey {
    pool_key_weth_usdc_e()
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────
//
// Only WETH/USDC.e is included — it is the only SushiSwap V2 pair on Arbitrum
// with a verified on-chain address. Additional pairs (USDT, etc.) had bad
// addresses and were removed. Add them back once correct addresses are confirmed.
pub fn arbitrum_pools() -> Vec<PoolCatalogEntry> {
    vec![
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdc_e(),
            dex_type: DexType::UniswapV2,
            expected_token0: weth(),
            expected_token1: usdc_e(),
            token0_symbol: "WETH",
            token1_symbol: "USDC.e",
            name: "WETH/USDC.e SushiV2",
            fee_tier: 0,
            n_coins: 0,
        },
    ]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────
//
// No triangular paths configured — triangular arb requires 3 pools minimum.
// Arbitrum is used for price monitoring only (WETH/USDC.e cross-chain spread).

pub fn arbitrum_arb_paths() -> Vec<ArbPath> {
    vec![]
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arbitrum_catalog_has_one_entry() {
        assert_eq!(arbitrum_pools().len(), 1);
    }

    #[test]
    fn test_arbitrum_pool_keys_all_use_arbitrum_chain() {
        for e in arbitrum_pools() {
            assert_eq!(e.pool_key.chain, ChainId::Arbitrum);
        }
    }

    #[test]
    fn test_arbitrum_arb_paths_empty() {
        assert!(arbitrum_arb_paths().is_empty());
    }

    #[test]
    fn test_arbitrum_weth_usdc_key_uses_arbitrum_chain() {
        assert_eq!(weth_usdc_key().chain, ChainId::Arbitrum);
    }
}
