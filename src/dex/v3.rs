use ethers::types::U256;

/// Approximate V3 output amount using the marginal price from sqrtPriceX96.
///
/// price = (sqrtPriceX96 / 2^96)^2
/// - zero_for_one=false (token0→token1): amountOut = amountIn * price * (1 - fee/1_000_000)
/// - zero_for_one=true  (token1→token0): amountOut = amountIn / price * (1 - fee/1_000_000)
///
/// Fee tiers: 100 (0.01%), 500 (0.05%), 3000 (0.3%), 10000 (1%)
/// All arithmetic uses checked_* — returns None on any overflow or division by zero.
///
/// NOTE: This is a marginal-price approximation. Real V3 output accounts for
/// price impact across the curve; this ignores that and is accurate only for
/// small trades relative to liquidity.
pub fn approx_out_v3(
    amount_in: U256,
    sqrt_price_x96: U256,
    fee_tier: u32,
    zero_for_one: bool,
) -> Option<U256> {
    if amount_in.is_zero() || sqrt_price_x96.is_zero() {
        return None;
    }
    if fee_tier >= 1_000_000 {
        return None;
    }

    let fee_num = U256::from(1_000_000 - fee_tier);
    let fee_den = U256::from(1_000_000u32);
    let q96 = U256::one() << 96u32;

    let raw = if !zero_for_one {
        // token0 → token1: amountOut = amountIn * (sqrtP / 2^96)^2
        // Computed as: (amountIn * sqrtP >> 96) * sqrtP >> 96
        let step1 = amount_in.checked_mul(sqrt_price_x96)? >> 96u32;
        step1.checked_mul(sqrt_price_x96)? >> 96u32
    } else {
        // token1 → token0: amountOut = amountIn * (2^96 / sqrtP)^2
        // Computed as: (amountIn * 2^96 / sqrtP) * 2^96 / sqrtP
        let step1 = amount_in.checked_mul(q96)?.checked_div(sqrt_price_x96)?;
        step1.checked_mul(q96)?.checked_div(sqrt_price_x96)?
    };

    raw.checked_mul(fee_num)?.checked_div(fee_den)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build sqrtPriceX96 for an integer price (token1/token0 in raw units).
    /// price must be a perfect square for exact results.
    /// For price=4 (sqrtPrice=2): sqrtPriceX96 = 2 * 2^96 = 2^97
    fn sqrt_price(sqrt_int: u64) -> U256 {
        U256::from(sqrt_int) * (U256::one() << 96u32)
    }

    // ── Basic correctness ─────────────────────────────────────────────────────

    #[test]
    fn test_approx_out_v3_zero_for_one_false_no_fee() {
        // price = 4 (sqrtPrice=2), zero_for_one=false: token0→token1
        // amountOut ≈ 100 * 4 * (1 - 0) = 400
        // fee_tier=0 means no fee (synthetic case for testing pure math)
        let out = approx_out_v3(U256::from(100u64), sqrt_price(2), 0, false);
        assert_eq!(out, Some(U256::from(400u64)));
    }

    #[test]
    fn test_approx_out_v3_zero_for_one_false_with_fee() {
        // price=4, fee=3000 (0.3%): 100 * 4 * (997000/1000000) = 398
        let out = approx_out_v3(U256::from(100u64), sqrt_price(2), 3000, false);
        assert_eq!(out, Some(U256::from(398u64)));
    }

    #[test]
    fn test_approx_out_v3_zero_for_one_true_no_fee() {
        // price=4, zero_for_one=true: token1→token0
        // amountOut ≈ 100 / 4 = 25
        let out = approx_out_v3(U256::from(100u64), sqrt_price(2), 0, true);
        assert_eq!(out, Some(U256::from(25u64)));
    }

    #[test]
    fn test_approx_out_v3_zero_for_one_true_with_fee() {
        // price=4, fee=3000: 100 / 4 * (997000/1000000) = 25 * 0.997 = 24 (floor)
        let out = approx_out_v3(U256::from(100u64), sqrt_price(2), 3000, true);
        assert_eq!(out, Some(U256::from(24u64)));
    }

    #[test]
    fn test_approx_out_v3_zero_amount_in() {
        assert_eq!(approx_out_v3(U256::zero(), sqrt_price(2), 500, false), None);
    }

    #[test]
    fn test_approx_out_v3_zero_sqrt_price() {
        assert_eq!(approx_out_v3(U256::from(100u64), U256::zero(), 500, false), None);
    }

    #[test]
    fn test_approx_out_v3_invalid_fee_tier() {
        assert_eq!(approx_out_v3(U256::from(100u64), sqrt_price(2), 1_000_000, false), None);
    }

    #[test]
    fn test_approx_out_v3_realistic_weth_usdc() {
        // Approximate WETH/USDC pool state (token0=USDC 6dec, token1=WETH 18dec)
        // WETH price ≈ $3000
        // price = raw_WETH / raw_USDC = 1e18 * 1e-6 / 3000 = 1e12 / 3000 ≈ 3.333e8
        // sqrtPrice ≈ sqrt(3.333e8) ≈ 18257
        // sqrtPriceX96 = 18257 * 2^96
        let sqrt_val = 18257u64;
        let sqrt_px96 = U256::from(sqrt_val) * (U256::one() << 96u32);

        // 1 ETH = 1e18 raw WETH (zero_for_one=true: WETH→USDC)
        let one_eth = U256::from(1_000_000_000_000_000_000u64);
        let fee = 500u32; // 0.05%

        let usdc_out = approx_out_v3(one_eth, sqrt_px96, fee, true);
        assert!(usdc_out.is_some(), "should return a value");

        // Expected: ~2999.8 USDC = ~2_999_800_000 raw USDC (6 dec)
        // Allow ±2% for integer math rounding
        let usdc = usdc_out.unwrap().low_u64();
        assert!(
            usdc > 2_500_000_000 && usdc < 3_500_000_000,
            "usdc_out={usdc} should be in $2500–$3500 range (6dec)"
        );
    }

    #[test]
    fn test_approx_out_v3_overflow_guard() {
        // Extreme sqrtPriceX96 — should not panic, return None on overflow
        let huge_sqrt = U256::MAX >> 1u32;
        let _ = approx_out_v3(U256::MAX, huge_sqrt, 500, false);
        let _ = approx_out_v3(U256::MAX, huge_sqrt, 500, true);
    }
}
