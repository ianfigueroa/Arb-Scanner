use ethers::types::Address;

use crate::config::PoolCatalogEntry;
use crate::types::{ArbPath, ChainId, DexType, HopSpec, PoolKey};

fn addr(s: &str) -> Address {
    s.parse().expect("invalid address in base config")
}

// ─── Token addresses ──────────────────────────────────────────────────────────

pub fn weth() -> Address {
    addr("0x4200000000000000000000000000000000000006")
}
pub fn usdc() -> Address {
    addr("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913")
}
pub fn dai() -> Address {
    addr("0x50c5725949A6F0c72E6C4a641F24049A917DB0Cb")
}

// ─── Pool keys ────────────────────────────────────────────────────────────────

/// BaseSwap V2 — WETH/USDC (token0=WETH, token1=USDC; 0x4200 < 0x8335)
pub fn pool_key_weth_usdc() -> PoolKey {
    PoolKey::new(ChainId::Base, addr("0xfDc1c9CBf7BD1Ac41d1A3C4b56E93c1d1437d2f7"))
}
/// BaseSwap V2 — WETH/DAI (token0=WETH, token1=DAI; 0x4200 < 0x50c5)
pub fn pool_key_weth_dai() -> PoolKey {
    PoolKey::new(ChainId::Base, addr("0x7B65E1AA7f45E17c91b7dbCe1d8B7bBA0D2cD4a8"))
}
/// BaseSwap V2 — DAI/USDC (token0=DAI, token1=USDC; 0x50c5 < 0x8335)
pub fn pool_key_dai_usdc() -> PoolKey {
    PoolKey::new(ChainId::Base, addr("0x6C8d3c11acE7618843d79c2aA4d3EDB83C31fb31"))
}

/// Primary WETH/stable pair used for price display and cross-chain monitoring.
pub fn weth_usdc_key() -> PoolKey {
    pool_key_weth_usdc()
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────
//
// Token address ordering (numerically):
//   WETH (0x4200) < DAI (0x50c5) < USDC (0x8335)
//
// | Pool            | token0 | token1 |
// |-----------------|--------|--------|
// | WETH/USDC Base  | WETH   | USDC   |
// | WETH/DAI  Base  | WETH   | DAI    |
// | DAI/USDC  Base  | DAI    | USDC   |
//
// Note: pool addresses need on-chain verification via verify_pool_tokens at startup.
pub fn base_pools() -> Vec<PoolCatalogEntry> {
    vec![
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdc(),
            dex_type: DexType::UniswapV2,
            expected_token0: weth(),
            expected_token1: usdc(),
            token0_symbol: "WETH",
            token1_symbol: "USDC",
            name: "WETH/USDC BaseSwap",
            fee_tier: 0,
            n_coins: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_weth_dai(),
            dex_type: DexType::UniswapV2,
            expected_token0: weth(),
            expected_token1: dai(),
            token0_symbol: "WETH",
            token1_symbol: "DAI",
            name: "WETH/DAI BaseSwap",
            fee_tier: 0,
            n_coins: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_dai_usdc(),
            dex_type: DexType::UniswapV2,
            expected_token0: dai(),
            expected_token1: usdc(),
            token0_symbol: "DAI",
            token1_symbol: "USDC",
            name: "DAI/USDC BaseSwap",
            fee_tier: 0,
            n_coins: 0,
        },
    ]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────

pub fn base_arb_paths() -> Vec<ArbPath> {
    vec![
        // Forward: WETH → USDC → DAI → WETH
        ArbPath {
            name: "WETH→USDC→DAI→WETH",
            chain: ChainId::Base,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdc(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: usdc(),
                },
                HopSpec {
                    pool_key: pool_key_dai_usdc(),
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
            chain: ChainId::Base,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_dai(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: dai(),
                },
                HopSpec {
                    pool_key: pool_key_dai_usdc(),
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
    fn test_base_catalog_has_three_entries() {
        assert_eq!(base_pools().len(), 3);
    }

    #[test]
    fn test_base_pool_keys_all_use_base_chain() {
        for e in base_pools() {
            assert_eq!(e.pool_key.chain, ChainId::Base);
        }
    }

    #[test]
    fn test_base_arb_paths_have_two_entries() {
        assert_eq!(base_arb_paths().len(), 2);
    }

    #[test]
    fn test_base_arb_paths_each_have_three_hops() {
        for p in base_arb_paths() {
            assert_eq!(p.hops.len(), 3);
        }
    }

    #[test]
    fn test_base_arb_paths_chain_matches() {
        for p in base_arb_paths() {
            assert_eq!(p.chain, ChainId::Base);
        }
    }

    #[test]
    fn test_base_weth_usdc_key_uses_base_chain() {
        assert_eq!(weth_usdc_key().chain, ChainId::Base);
    }

    #[test]
    fn test_base_all_hop_pool_keys_use_base_chain() {
        for path in base_arb_paths() {
            for hop in &path.hops {
                assert_eq!(hop.pool_key.chain, ChainId::Base);
            }
        }
    }
}
