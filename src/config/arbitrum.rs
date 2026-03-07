use ethers::types::Address;

use crate::config::PoolCatalogEntry;
use crate::types::{ArbPath, ChainId, DexType, HopSpec, PoolKey};

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
pub fn usdt() -> Address {
    addr("0xFd086bC7CD5C481DCC9C85ebE478A1C0b69FCbb9")
}

// ─── Pool keys ────────────────────────────────────────────────────────────────

/// SushiSwap V2 — WETH/USDC.e (token0=WETH, token1=USDC.e; 0x82aF < 0xFF97)
pub fn pool_key_weth_usdc_e() -> PoolKey {
    PoolKey::new(ChainId::Arbitrum, addr("0x905dfCD5649217c42684f23958568e533C711Aa3"))
}
/// SushiSwap V2 — WETH/USDT (token0=WETH, token1=USDT; 0x82aF < 0xFd08)
pub fn pool_key_weth_usdt() -> PoolKey {
    PoolKey::new(ChainId::Arbitrum, addr("0x1f56a7cc1abD5Df78aFE4E819AaB68eFf5cDB65b"))
}
/// SushiSwap V2 — USDT/USDC.e (token0=USDT, token1=USDC.e; 0xFd08 < 0xFF97)
pub fn pool_key_usdt_usdc_e() -> PoolKey {
    PoolKey::new(ChainId::Arbitrum, addr("0xCB0E5bFa72bBb4d16AB5aA0c60601c438F04b4ad"))
}

/// Primary WETH/stable pair used for price display and cross-chain monitoring.
pub fn weth_usdc_key() -> PoolKey {
    pool_key_weth_usdc_e()
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────
//
// Token address ordering (numerically):
//   WETH (0x82aF) < USDT (0xFd08) < USDC.e (0xFF97)
//
// | Pool              | token0 | token1  |
// |-------------------|--------|---------|
// | WETH/USDC.e Sushi | WETH   | USDC.e  |
// | WETH/USDT Sushi   | WETH   | USDT    |
// | USDT/USDC.e Sushi | USDT   | USDC.e  |
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
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdt(),
            dex_type: DexType::UniswapV2,
            expected_token0: weth(),
            expected_token1: usdt(),
            token0_symbol: "WETH",
            token1_symbol: "USDT",
            name: "WETH/USDT SushiV2",
            fee_tier: 0,
            n_coins: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_usdt_usdc_e(),
            dex_type: DexType::UniswapV2,
            expected_token0: usdt(),
            expected_token1: usdc_e(),
            token0_symbol: "USDT",
            token1_symbol: "USDC.e",
            name: "USDT/USDC.e SushiV2",
            fee_tier: 0,
            n_coins: 0,
        },
    ]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────

pub fn arbitrum_arb_paths() -> Vec<ArbPath> {
    vec![
        // Forward: WETH → USDC.e → USDT → WETH
        ArbPath {
            name: "WETH→USDC.e→USDT→WETH",
            chain: ChainId::Arbitrum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdc_e(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: usdc_e(),
                },
                HopSpec {
                    pool_key: pool_key_usdt_usdc_e(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdc_e(),
                    token_out: usdt(),
                },
                HopSpec {
                    pool_key: pool_key_weth_usdt(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdt(),
                    token_out: weth(),
                },
            ],
        },
        // Reverse: WETH → USDT → USDC.e → WETH
        ArbPath {
            name: "WETH→USDT→USDC.e→WETH",
            chain: ChainId::Arbitrum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdt(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: usdt(),
                },
                HopSpec {
                    pool_key: pool_key_usdt_usdc_e(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdt(),
                    token_out: usdc_e(),
                },
                HopSpec {
                    pool_key: pool_key_weth_usdc_e(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdc_e(),
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
    fn test_arbitrum_catalog_has_three_entries() {
        assert_eq!(arbitrum_pools().len(), 3);
    }

    #[test]
    fn test_arbitrum_pool_keys_all_use_arbitrum_chain() {
        for e in arbitrum_pools() {
            assert_eq!(e.pool_key.chain, ChainId::Arbitrum);
        }
    }

    #[test]
    fn test_arbitrum_arb_paths_have_two_entries() {
        assert_eq!(arbitrum_arb_paths().len(), 2);
    }

    #[test]
    fn test_arbitrum_arb_paths_each_have_three_hops() {
        for p in arbitrum_arb_paths() {
            assert_eq!(p.hops.len(), 3);
        }
    }

    #[test]
    fn test_arbitrum_arb_paths_chain_matches() {
        for p in arbitrum_arb_paths() {
            assert_eq!(p.chain, ChainId::Arbitrum);
        }
    }

    #[test]
    fn test_arbitrum_weth_usdc_key_uses_arbitrum_chain() {
        assert_eq!(weth_usdc_key().chain, ChainId::Arbitrum);
    }

    #[test]
    fn test_arbitrum_all_hop_pool_keys_use_arbitrum_chain() {
        for path in arbitrum_arb_paths() {
            for hop in &path.hops {
                assert_eq!(hop.pool_key.chain, ChainId::Arbitrum);
            }
        }
    }
}
