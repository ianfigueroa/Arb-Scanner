use ethers::types::{Address, U256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainId {
    Ethereum,
    Arbitrum,
    Base,
    Polygon,
}

impl ChainId {
    pub fn name(&self) -> &'static str {
        match self {
            ChainId::Ethereum => "ethereum",
            ChainId::Arbitrum => "arbitrum",
            ChainId::Base => "base",
            ChainId::Polygon => "polygon",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexType {
    UniswapV2,
    UniswapV3,
    CurveStableswap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolKey {
    pub chain: ChainId,
    pub address: Address,
}

impl PoolKey {
    pub fn new(chain: ChainId, address: Address) -> Self {
        Self { chain, address }
    }
}

#[derive(Debug, Clone)]
pub enum PoolState {
    V2 {
        reserve0: U256,
        reserve1: U256,
        /// Stored so quote dispatch can determine reserve direction from token_in/token_out.
        token0: Address,
        token1: Address,
        last_block: u64,
    },
    V3 {
        sqrt_price_x96: U256,
        fee_tier: u32,
        token0: Address,
        token1: Address,
        last_block: u64,
    },
    Curve {
        balances: Vec<U256>,
        last_block: u64,
    },
}

impl PoolState {
    pub fn last_block(&self) -> u64 {
        match self {
            PoolState::V2 { last_block, .. } => *last_block,
            PoolState::V3 { last_block, .. } => *last_block,
            PoolState::Curve { last_block, .. } => *last_block,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HopSpec {
    pub pool_key: PoolKey,
    pub dex_type: DexType,
    pub token_in: Address,
    pub token_out: Address,
}

#[derive(Debug, Clone)]
pub struct ArbPath {
    pub name: &'static str,
    pub chain: ChainId,
    pub hops: Vec<HopSpec>,
}

#[derive(Debug, Clone)]
pub struct ArbOpportunity {
    /// "WETH→USDC→DAI→WETH" or similar.
    pub path: &'static str,
    pub chain: ChainId,
    pub input_weth: U256,
    /// After subtracting estimated gas cost; may be negative (stored as i128 in wei).
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
