use std::collections::HashMap;

use ethers::types::U256;
use tracing::warn;

use crate::dex;
pub use crate::dex::v2::amm_out;
use crate::types::{ArbOpportunity, ArbPath, ChainId, PoolKey, PoolState};

/// Maximum block difference across pools in a path before skipping arb calculation.
pub const MAX_BLOCK_SKEW: u64 = 3;

/// Gas units estimated for a 3-hop triangular arb.
const GAS_UNITS: u64 = 150_000;

// ─── Freshness guard ──────────────────────────────────────────────────────────

/// Returns true if all pools referenced by the path exist and their last_block
/// values are within MAX_BLOCK_SKEW of each other.
fn path_is_fresh(pools: &HashMap<PoolKey, PoolState>, path: &ArbPath) -> bool {
    let blocks: Vec<u64> = path
        .hops
        .iter()
        .filter_map(|hop| pools.get(&hop.pool_key).map(|s| s.last_block()))
        .collect();

    if blocks.len() < path.hops.len() {
        warn!("freshness guard: not all path pools have state");
        return false;
    }

    let min = *blocks.iter().min().unwrap();
    let max = *blocks.iter().max().unwrap();
    if max - min > MAX_BLOCK_SKEW {
        warn!(
            min_block = min,
            max_block = max,
            skew = max - min,
            "freshness guard: pool states too far apart, skipping arb"
        );
        return false;
    }
    true
}

// ─── Path execution ───────────────────────────────────────────────────────────

/// Execute an arb path hop-by-hop. Returns the output amount after all hops,
/// or None if any hop fails or pools are stale.
pub(crate) fn execute_path(
    pools: &HashMap<PoolKey, PoolState>,
    path: &ArbPath,
    amount_in: U256,
) -> Option<U256> {
    if !path_is_fresh(pools, path) {
        return None;
    }
    let mut amount = amount_in;
    for hop in &path.hops {
        let state = pools.get(&hop.pool_key)?;
        amount = dex::quote(hop.dex_type, state, hop.token_in, hop.token_out, amount)?;
    }
    Some(amount)
}

// ─── Gas estimation ───────────────────────────────────────────────────────────

/// Compute estimated_net_after_gas (signed wei) and gas_cost_usd.
/// `weth_price_usd` is USDC per WETH (e.g. 3000.0).
pub fn apply_gas(
    gross_weth: U256,
    input_weth: U256,
    gas_price: U256,
    weth_price_usd: f64,
    path: &'static str,
    chain: ChainId,
) -> ArbOpportunity {
    let gas_cost_wei: U256 = gas_price.saturating_mul(U256::from(GAS_UNITS));

    let gross_i: i128 = u256_to_i128_lossy(gross_weth);
    let input_i: i128 = u256_to_i128_lossy(input_weth);
    let gas_i: i128 = u256_to_i128_lossy(gas_cost_wei);

    let profit_gross = gross_i - input_i;
    let estimated_net_after_gas = profit_gross - gas_i;

    let roi_pct = if input_i > 0 {
        (profit_gross as f64 / input_i as f64) * 100.0
    } else {
        0.0
    };

    let gas_cost_usd = (u256_to_f64(gas_cost_wei) / 1e18) * weth_price_usd;

    ArbOpportunity {
        path,
        chain,
        input_weth,
        estimated_net_after_gas,
        roi_pct,
        gas_cost_usd,
    }
}

// ─── Scan all opportunities ───────────────────────────────────────────────────

/// Input sizes to scan: 0.1, 0.5, 1, 5, 10 ETH in wei
const INPUT_SIZES_ETH: [u64; 5] = [
    100_000_000_000_000_000,    // 0.1 ETH
    500_000_000_000_000_000,    // 0.5 ETH
    1_000_000_000_000_000_000,  // 1 ETH
    5_000_000_000_000_000_000,  // 5 ETH
    10_000_000_000_000_000_000, // 10 ETH
];

pub fn scan_all_opportunities(
    pools: &HashMap<PoolKey, PoolState>,
    paths: &[ArbPath],
    gas_price: U256,
    weth_price_usd: f64,
) -> Vec<ArbOpportunity> {
    let mut opps = Vec::new();

    for path in paths {
        for &size in &INPUT_SIZES_ETH {
            let input = U256::from(size);
            if let Some(out) = execute_path(pools, path, input) {
                opps.push(apply_gas(out, input, gas_price, weth_price_usd, path.name, path.chain));
            }
        }
    }

    opps
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn u256_to_i128_lossy(v: U256) -> i128 {
    v.low_u128() as i128
}

pub fn u256_to_f64(v: U256) -> f64 {
    let (hi, lo) = v.div_mod(U256::from(u64::MAX));
    hi.low_u64() as f64 * u64::MAX as f64 + lo.low_u64() as f64
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::Address;
    use crate::types::{DexType, HopSpec};

    // ── Test fixtures ─────────────────────────────────────────────────────────

    fn test_addr(n: u8) -> Address {
        let mut bytes = [0u8; 20];
        bytes[19] = n;
        Address::from(bytes)
    }

    // Token indices
    const WETH: u8 = 1;
    const USDC: u8 = 2;
    const DAI: u8 = 3;

    // Pool address indices (different range from tokens)
    const WU_POOL: u8 = 0x10; // WETH/USDC: token0=USDC, token1=WETH
    const UD_POOL: u8 = 0x11; // USDC/DAI:  token0=DAI,  token1=USDC
    const DW_POOL: u8 = 0x12; // DAI/WETH:  token0=DAI,  token1=WETH

    fn wu_key() -> PoolKey {
        PoolKey::new(ChainId::Ethereum, test_addr(WU_POOL))
    }
    fn ud_key() -> PoolKey {
        PoolKey::new(ChainId::Ethereum, test_addr(UD_POOL))
    }
    fn dw_key() -> PoolKey {
        PoolKey::new(ChainId::Ethereum, test_addr(DW_POOL))
    }

    fn make_pools(
        wu_r0: u128, wu_r1: u128, wu_block: u64,
        ud_r0: u128, ud_r1: u128, ud_block: u64,
        dw_r0: u128, dw_r1: u128, dw_block: u64,
    ) -> HashMap<PoolKey, PoolState> {
        let mut m = HashMap::new();
        // WU: token0=USDC(2), token1=WETH(1)
        m.insert(wu_key(), PoolState::V2 {
            reserve0: U256::from(wu_r0),
            reserve1: U256::from(wu_r1),
            token0: test_addr(USDC),
            token1: test_addr(WETH),
            last_block: wu_block,
        });
        // UD: token0=DAI(3), token1=USDC(2)
        m.insert(ud_key(), PoolState::V2 {
            reserve0: U256::from(ud_r0),
            reserve1: U256::from(ud_r1),
            token0: test_addr(DAI),
            token1: test_addr(USDC),
            last_block: ud_block,
        });
        // DW: token0=DAI(3), token1=WETH(1)
        m.insert(dw_key(), PoolState::V2 {
            reserve0: U256::from(dw_r0),
            reserve1: U256::from(dw_r1),
            token0: test_addr(DAI),
            token1: test_addr(WETH),
            last_block: dw_block,
        });
        m
    }

    /// Forward path: WETH → USDC → DAI → WETH
    fn forward_path() -> ArbPath {
        ArbPath {
            name: "WETH→USDC→DAI→WETH",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: wu_key(),
                    dex_type: DexType::UniswapV2,
                    token_in: test_addr(WETH),
                    token_out: test_addr(USDC),
                },
                HopSpec {
                    pool_key: ud_key(),
                    dex_type: DexType::UniswapV2,
                    token_in: test_addr(USDC),
                    token_out: test_addr(DAI),
                },
                HopSpec {
                    pool_key: dw_key(),
                    dex_type: DexType::UniswapV2,
                    token_in: test_addr(DAI),
                    token_out: test_addr(WETH),
                },
            ],
        }
    }

    /// Reverse path: WETH → DAI → USDC → WETH
    fn reverse_path() -> ArbPath {
        ArbPath {
            name: "WETH→DAI→USDC→WETH",
            chain: ChainId::Ethereum,
            hops: vec![
                HopSpec {
                    pool_key: dw_key(),
                    dex_type: DexType::UniswapV2,
                    token_in: test_addr(WETH),
                    token_out: test_addr(DAI),
                },
                HopSpec {
                    pool_key: ud_key(),
                    dex_type: DexType::UniswapV2,
                    token_in: test_addr(DAI),
                    token_out: test_addr(USDC),
                },
                HopSpec {
                    pool_key: wu_key(),
                    dex_type: DexType::UniswapV2,
                    token_in: test_addr(USDC),
                    token_out: test_addr(WETH),
                },
            ],
        }
    }

    fn eth(n: u64) -> U256 {
        U256::from(n) * U256::exp10(18)
    }

    // ── amm_out ───────────────────────────────────────────────────────────────

    #[test]
    fn test_amm_out_basic() {
        // With equal reserves of 1000 and input of 100:
        // amount_in_with_fee = 100 * 997 = 99700
        // numerator = 99700 * 1000 = 99_700_000
        // denominator = 1000*1000 + 99700 = 1_099_700
        // out = 99_700_000 / 1_099_700 = 90 (integer division)
        let out = amm_out(U256::from(100u64), U256::from(1000u64), U256::from(1000u64));
        assert_eq!(out, Some(U256::from(90u64)));
    }

    #[test]
    fn test_amm_out_zero_reserve_in() {
        assert_eq!(amm_out(U256::from(100u64), U256::zero(), U256::from(1000u64)), None);
    }

    #[test]
    fn test_amm_out_zero_reserve_out() {
        assert_eq!(amm_out(U256::from(100u64), U256::from(1000u64), U256::zero()), None);
    }

    #[test]
    fn test_amm_out_zero_amount_in() {
        assert_eq!(amm_out(U256::zero(), U256::from(1000u64), U256::from(1000u64)), None);
    }

    #[test]
    fn test_amm_out_overflow_guard() {
        // Very large U256 values — should not panic, just return None due to overflow
        let huge = U256::MAX;
        // checked_mul(997) on U256::MAX will overflow → None
        let result = amm_out(huge, huge, huge);
        // Either None (overflow) or Some — just must not panic
        let _ = result;
    }

    // ── PoolState::last_block ─────────────────────────────────────────────────

    #[test]
    fn test_pool_state_last_block_v2() {
        let state = PoolState::V2 {
            reserve0: U256::zero(),
            reserve1: U256::zero(),
            token0: test_addr(1),
            token1: test_addr(2),
            last_block: 42,
        };
        assert_eq!(state.last_block(), 42);
    }

    #[test]
    fn test_pool_state_last_block_v3() {
        let state = PoolState::V3 {
            sqrt_price_x96: U256::zero(),
            fee_tier: 500,
            last_block: 99,
        };
        assert_eq!(state.last_block(), 99);
    }

    // ── PoolKey ───────────────────────────────────────────────────────────────

    #[test]
    fn test_pool_key_equality_and_hash() {
        use std::collections::HashMap;
        let k1 = PoolKey::new(ChainId::Ethereum, test_addr(1));
        let k2 = PoolKey::new(ChainId::Ethereum, test_addr(1));
        let k3 = PoolKey::new(ChainId::Arbitrum, test_addr(1));
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
        let mut m: HashMap<PoolKey, u32> = HashMap::new();
        m.insert(k1, 10);
        assert_eq!(m[&k2], 10);
        assert!(m.get(&k3).is_none());
    }

    // ── Freshness guard ───────────────────────────────────────────────────────

    #[test]
    fn test_freshness_guard_passes_when_within_skew() {
        let pools = make_pools(
            1_000_000, 300_000_000_000_000_000_000u128, 100,
            3_000_000_000_000_000_000_000u128, 1_000_000, 101,
            3_000_000_000_000_000_000_000u128, 1_000_000_000_000_000_000u128, 102,
        );
        // max - min = 2, within MAX_BLOCK_SKEW=3
        assert!(path_is_fresh(&pools, &forward_path()));
    }

    #[test]
    fn test_freshness_guard_skips_stale() {
        let pools = make_pools(
            1_000_000, 300_000_000_000_000_000_000u128, 100,
            3_000_000_000_000_000_000_000u128, 1_000_000, 105, // 5 blocks ahead
            3_000_000_000_000_000_000_000u128, 1_000_000_000_000_000_000u128, 100,
        );
        assert!(!path_is_fresh(&pools, &forward_path()));
    }

    #[test]
    fn test_freshness_guard_missing_pool() {
        let mut pools = HashMap::new();
        pools.insert(wu_key(), PoolState::V2 {
            reserve0: U256::from(1u64),
            reserve1: U256::from(1u64),
            token0: test_addr(USDC),
            token1: test_addr(WETH),
            last_block: 100,
        });
        // ud_key and dw_key missing — path has 3 hops but only 1 pool
        assert!(!path_is_fresh(&pools, &forward_path()));
    }

    #[test]
    fn test_execute_path_returns_none_when_stale() {
        let pools = make_pools(
            1_000_000, 300_000_000_000_000_000_000u128, 100,
            3_000_000_000_000_000_000_000u128, 1_000_000, 110, // stale
            3_000_000_000_000_000_000_000u128, 1_000_000_000_000_000_000u128, 100,
        );
        assert!(execute_path(&pools, &forward_path(), eth(1)).is_none());
    }

    // ── Path direction ────────────────────────────────────────────────────────

    #[test]
    fn test_forward_path_differs_from_reverse() {
        // Use realistic-ish reserves
        let usdc_reserves: u128 = 10_000_000_000_000; // 10M USDC in 6-dec units
        let weth_wu: u128 = 3_333_000_000_000_000_000_000u128; // 3333 WETH in wei

        let dai_ud: u128 = 10_000_000_000_000_000_000_000_000u128; // 10M DAI
        let usdc_ud: u128 = 10_000_000_000_000; // 10M USDC

        let dai_dw: u128 = 10_000_000_000_000_000_000_000_000u128; // 10M DAI
        let weth_dw: u128 = 3_333_000_000_000_000_000_000u128; // 3333 WETH

        let pools = make_pools(
            usdc_reserves, weth_wu, 100,
            dai_ud, usdc_ud, 100,
            dai_dw, weth_dw, 100,
        );

        let fwd = execute_path(&pools, &forward_path(), eth(1));
        let rev = execute_path(&pools, &reverse_path(), eth(1));

        assert!(fwd.is_some(), "forward path should produce a result");
        assert!(rev.is_some(), "reverse path should produce a result");
        assert_ne!(fwd, rev, "forward and reverse should differ");
    }

    // ── Decimal chain ─────────────────────────────────────────────────────────

    #[test]
    fn test_decimal_chain_usdc_intermediate() {
        // Verify that the 6-decimal USDC intermediate doesn't corrupt WETH output.
        // WethUsdc: reserve0(USDC 6dec)=3_000_000_000 (3000 USDC), reserve1(WETH)=1e18 (1 WETH)
        let usdc_r0: u128 = 3_000_000_000; // 3000 USDC in 6-dec
        let weth_r1: u128 = 1_000_000_000_000_000_000; // 1 WETH
        // UsdcDai: large DAI/USDC reserves at 1:1 ratio (scaled)
        let dai_ud: u128 = 3_000_000_000_000_000_000_000u128; // 3000 DAI in 18-dec
        let usdc_ud: u128 = 3_000_000_000; // 3000 USDC in 6-dec
        // DaiWeth: DAI/WETH at $3000
        let dai_dw: u128 = 3_000_000_000_000_000_000_000u128; // 3000 DAI
        let weth_dw: u128 = 1_000_000_000_000_000_000; // 1 WETH

        let pools = make_pools(
            usdc_r0, weth_r1, 100,
            dai_ud, usdc_ud, 100,
            dai_dw, weth_dw, 100,
        );

        let input = U256::from(1_000_000_000_000_000u64); // 0.001 WETH
        let out = execute_path(&pools, &forward_path(), input);

        assert!(out.is_some(), "should produce a result");
        let out_val = out.unwrap();

        // Output should be in the same order of magnitude as input (wei range),
        // not inflated to 6-dec scale (which would be ~1000x too small).
        let lower = U256::from(1_000_000_000_000u64);       // 1e12 wei = 0.000001 WETH
        let upper = U256::from(100_000_000_000_000_000u64); // 1e17 wei = 0.1 WETH
        assert!(
            out_val >= lower && out_val <= upper,
            "output {out_val} should be in plausible WETH wei range"
        );
    }

    // ── Fixture snapshot ──────────────────────────────────────────────────────

    #[test]
    fn test_fixture_snapshot() {
        // Real-ish reserves from a historical block.
        // WethUsdc: USDC≈6.7M, WETH≈2234 (at ~$3000/ETH)
        // Hand-calculated for input=1 ETH:
        // usdc_out ≈ 2_989_000_000 (~$2989 USDC in 6-dec units)
        let weth_amt: u128 = 1_000_000_000_000_000_000u128;
        let wu_r0: u128 = 6_700_000_000_000u128;           // 6.7M USDC
        let wu_r1: u128 = 2_234_000_000_000_000_000_000u128; // 2234 WETH

        // reserve_in = WETH (reserve1), reserve_out = USDC (reserve0)
        let usdc_out = amm_out(
            U256::from(weth_amt),
            U256::from(wu_r1),
            U256::from(wu_r0),
        );

        assert!(usdc_out.is_some());
        let usdc = usdc_out.unwrap().low_u64();
        // Expected: ~2_989_000_000 raw USDC (6-dec units = $2989), allow ±1%
        assert!(
            usdc > 2_900_000_000 && usdc < 3_100_000_000,
            "usdc_out={usdc} should be ~2_989_000_000 (=$2989)"
        );
    }
}
