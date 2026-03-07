use ethers::types::Address;

use crate::types::{ArbPath, ChainId, DexType, HopSpec, PoolKey};

pub struct PoolCatalogEntry {
    pub pool_key: PoolKey,
    pub dex_type: DexType,
    pub expected_token0: Address,
    pub expected_token1: Address,
    pub token0_symbol: &'static str,
    pub token1_symbol: &'static str,
    pub name: &'static str,
    pub fee_tier: u32,
}

fn addr(s: &str) -> Address {
    s.parse().expect("invalid address in ethereum config")
}

// ─── Token addresses ──────────────────────────────────────────────────────────

pub fn weth() -> Address {
    addr("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")
}
pub fn usdc() -> Address {
    addr("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
}
pub fn dai() -> Address {
    addr("0x6B175474E89094C44Da98b954EedeAC495271d0F")
}

// ─── Pool keys ────────────────────────────────────────────────────────────────

/// Uniswap V2 — WETH/USDC (token0=USDC, token1=WETH; 0xA0b8 < 0xC02a)
pub fn pool_key_weth_usdc() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc"))
}
/// Uniswap V2 — USDC/DAI (token0=DAI, token1=USDC; 0x6B17 < 0xA0b8)
pub fn pool_key_usdc_dai() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0xAE461cA67B15dc8dc81CE7615e0320dA1A9aB8D5"))
}
/// Uniswap V2 — DAI/WETH (token0=DAI, token1=WETH; 0x6B17 < 0xC02a)
pub fn pool_key_dai_weth() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0xA478c2975Ab1Ea89e8196811F51A7B7Ade33eB11"))
}
/// Uniswap V3 — WETH/USDC 0.05% (token0=USDC, token1=WETH)
pub fn pool_key_weth_usdc_v3_500() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"))
}
/// Uniswap V3 — WETH/USDC 0.3% (token0=USDC, token1=WETH)
pub fn pool_key_weth_usdc_v3_3000() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"))
}

/// Primary WETH/stable pair used for price display (V2; high liquidity).
pub fn pool_key_weth_usdc_info() -> PoolKey {
    pool_key_weth_usdc()
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────
//
// Token address ordering (numerically):
//   DAI  (0x6B17) < USDC (0xA0b8) < WETH (0xC02a)
//
// | Pool              | token0 | token1 | dex    | fee  |
// |-------------------|--------|--------|--------|------|
// | WETH/USDC V2      | USDC   | WETH   | V2     | 0    |
// | USDC/DAI  V2      | DAI    | USDC   | V2     | 0    |
// | DAI/WETH  V2      | DAI    | WETH   | V2     | 0    |
// | WETH/USDC V3 500  | USDC   | WETH   | V3     | 500  |
// | WETH/USDC V3 3000 | USDC   | WETH   | V3     | 3000 |
pub fn ethereum_pools() -> Vec<PoolCatalogEntry> {
    vec![
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdc(),
            dex_type: DexType::UniswapV2,
            expected_token0: usdc(),
            expected_token1: weth(),
            token0_symbol: "USDC",
            token1_symbol: "WETH",
            name: "WETH/USDC V2",
            fee_tier: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_usdc_dai(),
            dex_type: DexType::UniswapV2,
            expected_token0: dai(),
            expected_token1: usdc(),
            token0_symbol: "DAI",
            token1_symbol: "USDC",
            name: "USDC/DAI V2",
            fee_tier: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_dai_weth(),
            dex_type: DexType::UniswapV2,
            expected_token0: dai(),
            expected_token1: weth(),
            token0_symbol: "DAI",
            token1_symbol: "WETH",
            name: "DAI/WETH V2",
            fee_tier: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdc_v3_500(),
            dex_type: DexType::UniswapV3,
            expected_token0: usdc(),
            expected_token1: weth(),
            token0_symbol: "USDC",
            token1_symbol: "WETH",
            name: "WETH/USDC V3 0.05%",
            fee_tier: 500,
        },
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdc_v3_3000(),
            dex_type: DexType::UniswapV3,
            expected_token0: usdc(),
            expected_token1: weth(),
            token0_symbol: "USDC",
            token1_symbol: "WETH",
            name: "WETH/USDC V3 0.3%",
            fee_tier: 3000,
        },
    ]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────

pub fn ethereum_arb_paths() -> Vec<ArbPath> {
    vec![
        // V2-only: WETH → USDC → DAI → WETH
        ArbPath {
            name: "WETH→USDC→DAI→WETH",
            chain: ChainId::Ethereum,
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
                    pool_key: pool_key_dai_weth(),
                    dex_type: DexType::UniswapV2,
                    token_in: dai(),
                    token_out: weth(),
                },
            ],
        },
        // V2-only: WETH → DAI → USDC → WETH
        ArbPath {
            name: "WETH→DAI→USDC→WETH",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_dai_weth(),
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
        // Cross-DEX V3-500: WETH → USDC → DAI → WETH (V3 500 first hop)
        ArbPath {
            name: "WETH→USDC→DAI→WETH V3-500",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdc_v3_500(),
                    dex_type: DexType::UniswapV3,
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
                    pool_key: pool_key_dai_weth(),
                    dex_type: DexType::UniswapV2,
                    token_in: dai(),
                    token_out: weth(),
                },
            ],
        },
        // Cross-DEX V3-500: WETH → DAI → USDC → WETH (V3 500 last hop)
        ArbPath {
            name: "WETH→DAI→USDC→WETH V3-500",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_dai_weth(),
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
                    pool_key: pool_key_weth_usdc_v3_500(),
                    dex_type: DexType::UniswapV3,
                    token_in: usdc(),
                    token_out: weth(),
                },
            ],
        },
        // Cross-DEX V3-3000: WETH → USDC → DAI → WETH (V3 3000 first hop)
        ArbPath {
            name: "WETH→USDC→DAI→WETH V3-3000",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdc_v3_3000(),
                    dex_type: DexType::UniswapV3,
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
                    pool_key: pool_key_dai_weth(),
                    dex_type: DexType::UniswapV2,
                    token_in: dai(),
                    token_out: weth(),
                },
            ],
        },
        // Cross-DEX V3-3000: WETH → DAI → USDC → WETH (V3 3000 last hop)
        ArbPath {
            name: "WETH→DAI→USDC→WETH V3-3000",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_dai_weth(),
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
                    pool_key: pool_key_weth_usdc_v3_3000(),
                    dex_type: DexType::UniswapV3,
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
    fn test_ethereum_catalog_has_five_entries() {
        assert_eq!(ethereum_pools().len(), 5);
    }

    #[test]
    fn test_ethereum_pool_keys_all_use_ethereum_chain() {
        for e in ethereum_pools() {
            assert_eq!(e.pool_key.chain, ChainId::Ethereum);
        }
    }

    #[test]
    fn test_ethereum_arb_paths_have_six_entries() {
        assert_eq!(ethereum_arb_paths().len(), 6);
    }

    #[test]
    fn test_ethereum_arb_paths_each_have_three_hops() {
        for p in ethereum_arb_paths() {
            assert_eq!(p.hops.len(), 3);
        }
    }

    #[test]
    fn test_ethereum_arb_paths_chain_matches() {
        for p in ethereum_arb_paths() {
            assert_eq!(p.chain, ChainId::Ethereum);
        }
    }

    #[test]
    fn test_ethereum_weth_usdc_key_uses_ethereum_chain() {
        assert_eq!(pool_key_weth_usdc().chain, ChainId::Ethereum);
    }

    #[test]
    fn test_ethereum_all_hop_pool_keys_use_ethereum_chain() {
        for path in ethereum_arb_paths() {
            for hop in &path.hops {
                assert_eq!(hop.pool_key.chain, ChainId::Ethereum);
            }
        }
    }

    #[test]
    fn test_ethereum_v3_pools_have_fee_tier() {
        let v3_entries: Vec<_> = ethereum_pools()
            .into_iter()
            .filter(|e| e.dex_type == DexType::UniswapV3)
            .collect();
        assert_eq!(v3_entries.len(), 2);
        for e in &v3_entries {
            assert!(e.fee_tier > 0, "V3 pool {} should have non-zero fee_tier", e.name);
        }
    }

    #[test]
    fn test_ethereum_v2_pools_have_zero_fee_tier() {
        for e in ethereum_pools().into_iter().filter(|e| e.dex_type == DexType::UniswapV2) {
            assert_eq!(e.fee_tier, 0, "V2 pool {} should have fee_tier=0", e.name);
        }
    }
}
