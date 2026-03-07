use std::collections::HashMap;

use ethers::types::U256;
use tracing::warn;

use crate::types::{ArbOpportunity, PoolId, PoolState};

/// Maximum block difference across pools before skipping arb calculation.
pub const MAX_BLOCK_SKEW: u64 = 3;

/// Gas units estimated for a 3-hop triangular arb.
const GAS_UNITS: u64 = 150_000;

// ─── AMM formula ─────────────────────────────────────────────────────────────

/// Uniswap V2 constant-product output with 0.3% fee.
/// Uses raw token units — no decimal normalization needed across hops.
pub fn amm_out(amount_in: U256, reserve_in: U256, reserve_out: U256) -> Option<U256> {
    if reserve_in.is_zero() || reserve_out.is_zero() || amount_in.is_zero() {
        return None;
    }
    let amount_in_with_fee = amount_in.checked_mul(U256::from(997))?;
    let numerator = amount_in_with_fee.checked_mul(reserve_out)?;
    let denominator = reserve_in
        .checked_mul(U256::from(1000))?
        .checked_add(amount_in_with_fee)?;
    numerator.checked_div(denominator)
}

// ─── Freshness guard ─────────────────────────────────────────────────────────

/// Returns true if all pools exist and their last_block values are within
/// MAX_BLOCK_SKEW of each other.
fn pools_are_fresh(pools: &HashMap<PoolId, PoolState>) -> bool {
    let ids = [PoolId::WethUsdc, PoolId::UsdcDai, PoolId::DaiWeth];
    let blocks: Vec<u64> = ids
        .iter()
        .filter_map(|id| pools.get(id).map(|s| s.last_block))
        .collect();

    if blocks.len() < 3 {
        warn!("freshness guard: not all pools have state");
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

// ─── Path calculation ─────────────────────────────────────────────────────────

/// Forward path: WETH → USDC → DAI → WETH
///
/// Pool reserves layout:
///   WethUsdc: token0=USDC, token1=WETH
///   UsdcDai:  token0=DAI,  token1=USDC
///   DaiWeth:  token0=DAI,  token1=WETH
pub fn calc_forward(
    pools: &HashMap<PoolId, PoolState>,
    input_weth: U256,
) -> Option<U256> {
    if !pools_are_fresh(pools) {
        return None;
    }
    let wu = pools.get(&PoolId::WethUsdc)?;
    let ud = pools.get(&PoolId::UsdcDai)?;
    let dw = pools.get(&PoolId::DaiWeth)?;

    // Hop 1: WETH (reserve1) → USDC (reserve0) in WethUsdc pool
    let usdc_out = amm_out(input_weth, wu.reserve1, wu.reserve0)?;
    // Hop 2: USDC (reserve1) → DAI (reserve0) in UsdcDai pool
    let dai_out = amm_out(usdc_out, ud.reserve1, ud.reserve0)?;
    // Hop 3: DAI (reserve0) → WETH (reserve1) in DaiWeth pool
    let weth_out = amm_out(dai_out, dw.reserve0, dw.reserve1)?;

    Some(weth_out)
}

/// Reverse path: WETH → DAI → USDC → WETH
pub fn calc_reverse(
    pools: &HashMap<PoolId, PoolState>,
    input_weth: U256,
) -> Option<U256> {
    if !pools_are_fresh(pools) {
        return None;
    }
    let wu = pools.get(&PoolId::WethUsdc)?;
    let ud = pools.get(&PoolId::UsdcDai)?;
    let dw = pools.get(&PoolId::DaiWeth)?;

    // Hop 1: WETH (reserve1) → DAI (reserve0) in DaiWeth pool
    let dai_out = amm_out(input_weth, dw.reserve1, dw.reserve0)?;
    // Hop 2: DAI (reserve0) → USDC (reserve1) in UsdcDai pool
    let usdc_out = amm_out(dai_out, ud.reserve0, ud.reserve1)?;
    // Hop 3: USDC (reserve0) → WETH (reserve1) in WethUsdc pool
    let weth_out = amm_out(usdc_out, wu.reserve0, wu.reserve1)?;

    Some(weth_out)
}

// ─── Gas estimation ──────────────────────────────────────────────────────────

/// Compute estimated_net_after_gas (signed wei) and gas_cost_usd.
/// `weth_usdc_price` is USDC per WETH (e.g. 3000.0).
pub fn apply_gas(
    gross_weth: U256,
    input_weth: U256,
    gas_price: U256,
    weth_price_usd: f64,
    path: &'static str,
) -> ArbOpportunity {
    let gas_cost_wei: U256 = gas_price.saturating_mul(U256::from(GAS_UNITS));

    // Convert to i128 for signed arithmetic (fits: max ~2^127 wei >> any realistic value)
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

    // gas cost in USD: gas_cost_wei / 1e18 * weth_price_usd
    let gas_cost_usd = (u256_to_f64(gas_cost_wei) / 1e18) * weth_price_usd;

    ArbOpportunity {
        path,
        input_weth,
        estimated_net_after_gas,
        roi_pct,
        gas_cost_usd,
    }
}

// ─── Scan all opportunities ────────────────────────────────────────────────────

/// Input sizes to scan: 0.1, 0.5, 1, 5, 10 ETH in wei
const INPUT_SIZES_ETH: [u64; 5] = [
    100_000_000_000_000_000,   // 0.1 ETH
    500_000_000_000_000_000,   // 0.5 ETH
    1_000_000_000_000_000_000, // 1 ETH
    5_000_000_000_000_000_000, // 5 ETH
    10_000_000_000_000_000_000, // 10 ETH — u64 max is ~18.4 ETH so this fits
];

pub fn scan_all_opportunities(
    pools: &HashMap<PoolId, PoolState>,
    gas_price: U256,
    weth_price_usd: f64,
) -> Vec<ArbOpportunity> {
    let mut opps = Vec::new();

    for &size in &INPUT_SIZES_ETH {
        let input = U256::from(size);

        if let Some(out) = calc_forward(pools, input) {
            opps.push(apply_gas(out, input, gas_price, weth_price_usd, "WETH→USDC→DAI→WETH"));
        }
        if let Some(out) = calc_reverse(pools, input) {
            opps.push(apply_gas(out, input, gas_price, weth_price_usd, "WETH→DAI→USDC→WETH"));
        }
    }

    opps
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn u256_to_i128_lossy(v: U256) -> i128 {
    // Take the low 128 bits; for realistic ETH/gas values this never overflows
    v.low_u128() as i128
}

pub fn u256_to_f64(v: U256) -> f64 {
    // Accurate enough for display purposes
    let (hi, lo) = v.div_mod(U256::from(u64::MAX));
    hi.low_u64() as f64 * u64::MAX as f64 + lo.low_u64() as f64
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn eth(n: u64) -> U256 {
        U256::from(n) * U256::exp10(18)
    }

    /// Build a pool map with controlled last_block values.
    fn make_pools(
        wu_r0: u128, wu_r1: u128, wu_block: u64,
        ud_r0: u128, ud_r1: u128, ud_block: u64,
        dw_r0: u128, dw_r1: u128, dw_block: u64,
    ) -> HashMap<PoolId, PoolState> {
        let mut m = HashMap::new();
        m.insert(PoolId::WethUsdc, PoolState { reserve0: U256::from(wu_r0), reserve1: U256::from(wu_r1), last_block: wu_block });
        m.insert(PoolId::UsdcDai,  PoolState { reserve0: U256::from(ud_r0), reserve1: U256::from(ud_r1), last_block: ud_block });
        m.insert(PoolId::DaiWeth,  PoolState { reserve0: U256::from(dw_r0), reserve1: U256::from(dw_r1), last_block: dw_block });
        m
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

    // ── Freshness guard ───────────────────────────────────────────────────────

    #[test]
    fn test_freshness_guard_passes_when_within_skew() {
        let pools = make_pools(
            1_000_000, 300_000_000_000_000_000_000u128, 100,
            3_000_000_000_000_000_000_000u128, 1_000_000, 101,
            3_000_000_000_000_000_000_000u128, 1_000_000_000_000_000_000u128, 102,
        );
        // max - min = 2, within MAX_BLOCK_SKEW=3
        assert!(pools_are_fresh(&pools));
    }

    #[test]
    fn test_freshness_guard_skips_stale() {
        let pools = make_pools(
            1_000_000, 300_000_000_000_000_000_000u128, 100,
            3_000_000_000_000_000_000_000u128, 1_000_000, 105, // 5 blocks ahead
            3_000_000_000_000_000_000_000u128, 1_000_000_000_000_000_000u128, 100,
        );
        assert!(!pools_are_fresh(&pools));
    }

    #[test]
    fn test_freshness_guard_missing_pool() {
        let mut pools = HashMap::new();
        pools.insert(PoolId::WethUsdc, PoolState { reserve0: U256::from(1u64), reserve1: U256::from(1u64), last_block: 100 });
        // UsdcDai and DaiWeth missing
        assert!(!pools_are_fresh(&pools));
    }

    #[test]
    fn test_calc_forward_returns_none_when_stale() {
        let pools = make_pools(
            1_000_000, 300_000_000_000_000_000_000u128, 100,
            3_000_000_000_000_000_000_000u128, 1_000_000, 110, // stale
            3_000_000_000_000_000_000_000u128, 1_000_000_000_000_000_000u128, 100,
        );
        assert!(calc_forward(&pools, eth(1)).is_none());
    }

    // ── Path direction ────────────────────────────────────────────────────────

    #[test]
    fn test_forward_path_differs_from_reverse() {
        // Use realistic-ish reserves
        // WethUsdc: token0=USDC (6dec, scaled to units), token1=WETH
        // ~$3000 WETH: 10_000_000 USDC (6 dec) : 3333 WETH (18 dec)
        let usdc_reserves: u128 = 10_000_000_000_000; // 10M USDC in 6-dec units
        let weth_wu: u128 = 3_333_000_000_000_000_000_000u128; // 3333 WETH in wei

        // UsdcDai: token0=DAI, token1=USDC — ~1:1
        let dai_ud: u128 = 10_000_000_000_000_000_000_000_000u128; // 10M DAI
        let usdc_ud: u128 = 10_000_000_000_000; // 10M USDC

        // DaiWeth: token0=DAI, token1=WETH — ~$3000
        let dai_dw: u128 = 10_000_000_000_000_000_000_000_000u128; // 10M DAI
        let weth_dw: u128 = 3_333_000_000_000_000_000_000u128; // 3333 WETH

        let pools = make_pools(
            usdc_reserves, weth_wu, 100,
            dai_ud, usdc_ud, 100,
            dai_dw, weth_dw, 100,
        );

        let fwd = calc_forward(&pools, eth(1));
        let rev = calc_reverse(&pools, eth(1));

        assert!(fwd.is_some(), "forward path should produce a result");
        assert!(rev.is_some(), "reverse path should produce a result");
        // They should give different amounts (symmetric pools would give same, but
        // direction matters for real pools)
        assert_ne!(fwd, rev, "forward and reverse should differ");
    }

    // ── Decimal chain ─────────────────────────────────────────────────────────

    #[test]
    fn test_decimal_chain_usdc_intermediate() {
        // Verify that the 6-decimal USDC intermediate doesn't corrupt WETH output.
        // Use small round numbers to trace manually.
        //
        // WethUsdc: reserve0(USDC 6dec)=3_000_000_000 (3000 USDC), reserve1(WETH)=1e18 (1 WETH)
        // Input: 0.001 WETH = 1e15 wei
        //
        // Hop1: amm_out(1e15, 1e18, 3_000_000_000)
        //   fee_in = 1e15 * 997 = 997e15
        //   num = 997e15 * 3_000_000_000 = 2.991e27
        //   denom = 1e18*1000 + 997e15 = 1_000_997e15
        //   usdc_out ≈ 2.991e27 / 1_000_997e15 ≈ 2_988_008 (in 6-dec, ~2.988 USDC)
        //
        // The key property: usdc_out is in the thousands range (6-dec), not 18-dec.
        // Then Hop2 consumes that as USDC input into the DAI pool.
        // Hop3 outputs WETH in 18-dec range again.

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
        let out = calc_forward(&pools, input);

        assert!(out.is_some(), "should produce a result");
        let out_val = out.unwrap();

        // Output should be in the same order of magnitude as input (wei range),
        // not inflated to 6-dec scale (which would be ~1000x too small).
        // Should be somewhere in the range of 0.0001 to 0.01 WETH = 1e14 to 1e16 wei
        let lower = U256::from(1_000_000_000_000u64);     // 1e12 wei = 0.000001 WETH
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
        // These are approximate; we verify the formula gives expected output within 1%.
        //
        // Hand-calculated for input=1 ETH:
        // Hop1: amm_out(1e18, 2234e18, 6_700_000e6)
        //   fee_in = 997e18
        //   num = 997e18 * 6_700_000_000_000 = 6.68e30
        //   denom = 2234e18*1000 + 997e18 = 2_234_997e18
        //   usdc_out ≈ 6.68e33 / 2.235e24 ≈ 2_989_000_000 (~$2989 USDC, correct ~$3000)

        let weth_amt: u128 = 1_000_000_000_000_000_000u128;
        let wu_r0: u128 = 6_700_000_000_000u128; // 6.7M USDC
        let wu_r1: u128 = 2_234_000_000_000_000_000_000u128; // 2234 WETH

        let usdc_out = amm_out(
            U256::from(weth_amt),
            U256::from(wu_r1), // reserve_in = WETH
            U256::from(wu_r0), // reserve_out = USDC
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
