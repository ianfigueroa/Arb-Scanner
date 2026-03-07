use ethers::types::U256;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_amm_out_basic_v2_module() {
        let out = amm_out(U256::from(100u64), U256::from(1000u64), U256::from(1000u64));
        assert_eq!(out, Some(U256::from(90u64)));
    }

    #[test]
    fn test_amm_out_zero_inputs_v2_module() {
        assert_eq!(amm_out(U256::zero(), U256::from(1000u64), U256::from(1000u64)), None);
        assert_eq!(amm_out(U256::from(100u64), U256::zero(), U256::from(1000u64)), None);
        assert_eq!(amm_out(U256::from(100u64), U256::from(1000u64), U256::zero()), None);
    }
}
