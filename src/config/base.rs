use ethers::types::Address;

use crate::config::PoolCatalogEntry;
use crate::types::{ArbPath, ChainId, DexType, PoolKey};

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
// Pool addresses for Base have not been verified on-chain.
// Add them back once correct addresses are confirmed via the BaseSwap factory.
pub fn base_pools() -> Vec<PoolCatalogEntry> {
    vec![]
}

// ─── Arb paths ────────────────────────────────────────────────────────────────

pub fn base_arb_paths() -> Vec<ArbPath> {
    vec![]
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_catalog_empty() {
        assert!(base_pools().is_empty());
    }

    #[test]
    fn test_base_arb_paths_empty() {
        assert!(base_arb_paths().is_empty());
    }

    #[test]
    fn test_base_weth_usdc_key_uses_base_chain() {
        assert_eq!(weth_usdc_key().chain, ChainId::Base);
    }
}
