use ethers::types::U256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoolId {
    WethUsdc,
    UsdcDai,
    DaiWeth,
}

impl PoolId {
    pub fn name(&self) -> &'static str {
        match self {
            PoolId::WethUsdc => "WETH/USDC",
            PoolId::UsdcDai => "USDC/DAI",
            PoolId::DaiWeth => "DAI/WETH",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolState {
    pub reserve0: U256,
    pub reserve1: U256,
    pub last_block: u64,
}

#[derive(Debug, Clone)]
pub struct ArbOpportunity {
    /// "WETH→USDC→DAI→WETH" or "WETH→DAI→USDC→WETH"
    pub path: &'static str,
    pub input_weth: U256,
    /// After subtracting estimated gas cost; may be negative (stored as i128 in wei)
    pub estimated_net_after_gas: i128,
    pub roi_pct: f64,
    pub gas_cost_usd: f64,
}

#[derive(Debug, Default)]
pub struct SessionStats {
    pub blocks_scanned: u64,
    pub opps_found: u64,
    pub best_opp: Option<ArbOpportunity>,
}
