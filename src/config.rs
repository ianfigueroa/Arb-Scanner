use ethers::types::Address;
use crate::types::PoolId;

pub struct PoolConfig {
    pub pool_id: PoolId,
    pub pair_address: Address,
    pub expected_token0: Address,
    pub expected_token1: Address,
    pub token0_symbol: &'static str,
    pub token1_symbol: &'static str,
}

/// Parse a checksummed Ethereum address string at runtime.
fn addr(s: &str) -> Address {
    s.parse().expect("invalid address in POOL_CONFIGS")
}

/// Verified token ordering from on-chain state.
/// token0/token1 must be confirmed via on-chain call at startup.
///
/// | Pool      | token0                  | token1                 |
/// |-----------|-------------------------|------------------------|
/// | WETH/USDC | USDC (6 dec)            | WETH (18 dec)          |
/// | USDC/DAI  | DAI  (18 dec)           | USDC (6 dec)           |
/// | DAI/WETH  | DAI  (18 dec)           | WETH (18 dec)          |
pub fn pool_configs() -> [PoolConfig; 3] {
    [
        // WETH/USDC: token0=USDC (6 dec), token1=WETH (18 dec)
        PoolConfig {
            pool_id: PoolId::WethUsdc,
            pair_address:    addr("0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc"),
            expected_token0: addr("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"), // USDC
            expected_token1: addr("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"), // WETH
            token0_symbol: "USDC",
            token1_symbol: "WETH",
        },
        // USDC/DAI: token0=DAI (18 dec), token1=USDC (6 dec)
        PoolConfig {
            pool_id: PoolId::UsdcDai,
            pair_address:    addr("0xAE461cA67B15dc8dc81CE7615e0320dA1A9aB8D5"),
            expected_token0: addr("0x6B175474E89094C44Da98b954EedeAC495271d0F"), // DAI
            expected_token1: addr("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"), // USDC
            token0_symbol: "DAI",
            token1_symbol: "USDC",
        },
        // DAI/WETH: token0=DAI (18 dec), token1=WETH (18 dec)
        PoolConfig {
            pool_id: PoolId::DaiWeth,
            pair_address:    addr("0xA478c2975Ab1Ea89e8196811F51A7B7Ade33eB11"),
            expected_token0: addr("0x6B175474E89094C44Da98b954EedeAC495271d0F"), // DAI
            expected_token1: addr("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"), // WETH
            token0_symbol: "DAI",
            token1_symbol: "WETH",
        },
    ]
}
