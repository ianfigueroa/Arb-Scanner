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
    /// Coin count for Curve pools (0 for V2/V3, 3 for Curve 3pool).
    pub n_coins: u8,
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
pub fn usdt() -> Address {
    addr("0xdAC17F958D2ee523a2206206994597C13D831ec7")
}

// ─── Pool keys ────────────────────────────────────────────────────────────────

/// Uniswap V2 — WETH/USDC (token0=USDC, token1=WETH; 0xA0b8 < 0xC02a)
pub fn pool_key_weth_usdc() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc"),
    )
}
/// Uniswap V2 — USDC/DAI (token0=DAI, token1=USDC; 0x6B17 < 0xA0b8)
pub fn pool_key_usdc_dai() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0xAE461cA67B15dc8dc81CE7615e0320dA1A9aB8D5"),
    )
}
/// Uniswap V2 — DAI/WETH (token0=DAI, token1=WETH; 0x6B17 < 0xC02a)
pub fn pool_key_dai_weth() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0xA478c2975Ab1Ea89e8196811F51A7B7Ade33eB11"),
    )
}
/// Uniswap V3 — WETH/USDC 0.05% (token0=USDC, token1=WETH)
pub fn pool_key_weth_usdc_v3_500() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0x88e6A0c2dDD26FEEb64F039a2c41296FcB3f5640"),
    )
}
/// Uniswap V3 — WETH/USDC 0.3% (token0=USDC, token1=WETH)
pub fn pool_key_weth_usdc_v3_3000() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0x8ad599c3A0ff1De082011EFDDc58f1908eb6e6D8"),
    )
}
/// Curve 3pool — DAI/USDC/USDT (coins: DAI=0, USDC=1, USDT=2)
pub fn pool_key_curve_3pool() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0xbEbc44782C7dB0a1A60Cb6fe97d0b483032FF1C7"),
    )
}
/// Uniswap V2 — WETH/USDT (token0=WETH, token1=USDT; 0xC02a < 0xdAC1)
pub fn pool_key_weth_usdt() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0x0d4a11d5EEaaC28EC3F61d100daF4d40471f1852"),
    )
}
/// Uniswap V2 — DAI/USDT (token0=DAI, token1=USDT; 0x6B17 < 0xdAC1)
pub fn pool_key_dai_usdt() -> PoolKey {
    PoolKey::new(
        ChainId::Ethereum,
        addr("0xB20bd5D04BE54f870D5C0d3ca85d82b34B836405"),
    )
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────
//
// Token address ordering (numerically):
//   DAI  (0x6B17) < USDC (0xA0b8) < WETH (0xC02a) < USDT (0xdAC1)
//
// | Pool              | token0 | token1 | dex   | fee  | n_coins |
// |-------------------|--------|--------|-------|------|---------|
// | WETH/USDC V2      | USDC   | WETH   | V2    | 0    | 0       |
// | USDC/DAI  V2      | DAI    | USDC   | V2    | 0    | 0       |
// | DAI/WETH  V2      | DAI    | WETH   | V2    | 0    | 0       |
// | WETH/USDC V3 500  | USDC   | WETH   | V3    | 500  | 0       |
// | WETH/USDC V3 3000 | USDC   | WETH   | V3    | 3000 | 0       |
// | Curve 3pool       | DAI    | USDC   | Curve | 0    | 3       |
// | WETH/USDT V2      | WETH   | USDT   | V2    | 0    | 0       |
// | DAI/USDT  V2      | DAI    | USDT   | V2    | 0    | 0       |
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
            n_coins: 0,
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
            n_coins: 0,
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
            n_coins: 0,
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
            n_coins: 0,
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
            n_coins: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_curve_3pool(),
            dex_type: DexType::CurveStableswap,
            expected_token0: dai(),
            expected_token1: usdc(),
            token0_symbol: "DAI",
            token1_symbol: "USDC",
            name: "Curve 3pool",
            fee_tier: 0,
            n_coins: 3,
        },
        PoolCatalogEntry {
            pool_key: pool_key_weth_usdt(),
            dex_type: DexType::UniswapV2,
            expected_token0: weth(),
            expected_token1: usdt(),
            token0_symbol: "WETH",
            token1_symbol: "USDT",
            name: "WETH/USDT V2",
            fee_tier: 0,
            n_coins: 0,
        },
        PoolCatalogEntry {
            pool_key: pool_key_dai_usdt(),
            dex_type: DexType::UniswapV2,
            expected_token0: dai(),
            expected_token1: usdt(),
            token0_symbol: "DAI",
            token1_symbol: "USDT",
            name: "DAI/USDT V2",
            fee_tier: 0,
            n_coins: 0,
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
        // V2-only: WETH → USDT → DAI → WETH
        ArbPath {
            name: "WETH→USDT→DAI→WETH",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_weth_usdt(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: usdt(),
                },
                HopSpec {
                    pool_key: pool_key_dai_usdt(),
                    dex_type: DexType::UniswapV2,
                    token_in: usdt(),
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
        // V2-only: WETH → DAI → USDT → WETH
        ArbPath {
            name: "WETH→DAI→USDT→WETH",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: pool_key_dai_weth(),
                    dex_type: DexType::UniswapV2,
                    token_in: weth(),
                    token_out: dai(),
                },
                HopSpec {
                    pool_key: pool_key_dai_usdt(),
                    dex_type: DexType::UniswapV2,
                    token_in: dai(),
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
    ]
    // NOTE: Curve arb paths are excluded. The synchronous scan loop cannot call
    // curve_quote (async get_dy RPC call). Curve hops always return None from
    // dex::quote(), so any path containing them would be silently skipped.
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ethereum_catalog_has_eight_entries() {
        assert_eq!(ethereum_pools().len(), 8);
    }

    #[test]
    fn test_ethereum_catalog_has_curve_pool() {
        let curve: Vec<_> = ethereum_pools()
            .into_iter()
            .filter(|e| e.dex_type == DexType::CurveStableswap)
            .collect();
        assert_eq!(curve.len(), 1);
        assert_eq!(curve[0].n_coins, 3);
        assert_eq!(curve[0].name, "Curve 3pool");
    }

    #[test]
    fn test_ethereum_pool_keys_all_use_ethereum_chain() {
        for e in ethereum_pools() {
            assert_eq!(e.pool_key.chain, ChainId::Ethereum);
        }
    }

    #[test]
    fn test_ethereum_arb_paths_have_eight_entries() {
        assert_eq!(ethereum_arb_paths().len(), 8);
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
            assert!(
                e.fee_tier > 0,
                "V3 pool {} should have non-zero fee_tier",
                e.name
            );
        }
    }

    #[test]
    fn test_ethereum_v2_pools_have_zero_fee_tier() {
        for e in ethereum_pools()
            .into_iter()
            .filter(|e| e.dex_type == DexType::UniswapV2 || e.dex_type == DexType::CurveStableswap)
        {
            assert_eq!(e.fee_tier, 0, "pool {} should have fee_tier=0", e.name);
        }
    }
}
