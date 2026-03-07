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

pub fn pool_key_weth_usdc() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc"))
}
pub fn pool_key_usdc_dai() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0xAE461cA67B15dc8dc81CE7615e0320dA1A9aB8D5"))
}
pub fn pool_key_dai_weth() -> PoolKey {
    PoolKey::new(ChainId::Ethereum, addr("0xA478c2975Ab1Ea89e8196811F51A7B7Ade33eB11"))
}

// ─── Pool catalog ─────────────────────────────────────────────────────────────

/// Verified token ordering from on-chain state.
///
/// | Pool      | token0                  | token1                 |
/// |-----------|-------------------------|------------------------|
/// | WETH/USDC | USDC (6 dec)            | WETH (18 dec)          |
/// | USDC/DAI  | DAI  (18 dec)           | USDC (6 dec)           |
/// | DAI/WETH  | DAI  (18 dec)           | WETH (18 dec)          |
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
        },
        PoolCatalogEntry {
            pool_key: pool_key_usdc_dai(),
            dex_type: DexType::UniswapV2,
            expected_token0: dai(),
            expected_token1: usdc(),
            token0_symbol: "DAI",
            token1_symbol: "USDC",
            name: "USDC/DAI V2",
        },
        PoolCatalogEntry {
            pool_key: pool_key_dai_weth(),
            dex_type: DexType::UniswapV2,
            expected_token0: dai(),
            expected_token1: weth(),
            token0_symbol: "DAI",
            token1_symbol: "WETH",
            name: "DAI/WETH V2",
        },
    ]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────

pub fn ethereum_arb_paths() -> Vec<ArbPath> {
    vec![
        // Forward: WETH → USDC → DAI → WETH
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
        // Reverse: WETH → DAI → USDC → WETH
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
    ]
}
