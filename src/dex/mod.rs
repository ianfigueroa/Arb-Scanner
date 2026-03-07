pub mod curve;
pub mod v2;
pub mod v3;

use ethers::types::{Address, U256};

use crate::types::{DexType, PoolState};

// ─── Synchronous quote dispatch ───────────────────────────────────────────────

/// Compute an output quote for a single hop using pool state already in memory.
///
/// Handles UniswapV2 (constant-product) and returns None for other types:
/// - UniswapV3: requires tick/liquidity data added in Phase 4
/// - CurveStableswap: requires async on-chain call via dex::curve::curve_quote
pub fn quote(
    dex_type: DexType,
    state: &PoolState,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
) -> Option<U256> {
    match dex_type {
        DexType::UniswapV2 => v2_dispatch(state, token_in, token_out, amount_in),
        DexType::UniswapV3 => v3_dispatch(state, token_in, token_out, amount_in),
        DexType::CurveStableswap => None,
    }
}

fn v2_dispatch(
    state: &PoolState,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
) -> Option<U256> {
    let (reserve0, reserve1, token0, token1) = match state {
        PoolState::V2 { reserve0, reserve1, token0, token1, .. } => {
            (*reserve0, *reserve1, *token0, *token1)
        }
        _ => return None,
    };
    if token_in == token0 && token_out == token1 {
        v2::amm_out(amount_in, reserve0, reserve1)
    } else if token_in == token1 && token_out == token0 {
        v2::amm_out(amount_in, reserve1, reserve0)
    } else {
        None
    }
}

fn v3_dispatch(
    state: &PoolState,
    token_in: Address,
    token_out: Address,
    amount_in: U256,
) -> Option<U256> {
    let (sqrt_price_x96, fee_tier, token0, token1) = match state {
        PoolState::V3 { sqrt_price_x96, fee_tier, token0, token1, .. } => {
            (*sqrt_price_x96, *fee_tier, *token0, *token1)
        }
        _ => return None,
    };
    // zero_for_one=false: token0→token1 (multiply by price)
    // zero_for_one=true:  token1→token0 (divide by price)
    let zero_for_one = if token_in == token0 && token_out == token1 {
        false
    } else if token_in == token1 && token_out == token0 {
        true
    } else {
        return None;
    };
    v3::approx_out_v3(amount_in, sqrt_price_x96, fee_tier, zero_for_one)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_addr(n: u8) -> Address {
        let mut bytes = [0u8; 20];
        bytes[19] = n;
        Address::from(bytes)
    }

    fn make_v2_state(r0: u128, r1: u128, t0: u8, t1: u8) -> PoolState {
        PoolState::V2 {
            reserve0: U256::from(r0),
            reserve1: U256::from(r1),
            token0: test_addr(t0),
            token1: test_addr(t1),
            last_block: 100,
        }
    }

    #[test]
    fn test_quote_v2_forward() {
        let state = make_v2_state(1000, 2000, 1, 2);
        let out = quote(DexType::UniswapV2, &state, test_addr(1), test_addr(2), U256::from(100u64));
        assert_eq!(out, v2::amm_out(U256::from(100u64), U256::from(1000u64), U256::from(2000u64)));
    }

    #[test]
    fn test_quote_v2_reverse() {
        let state = make_v2_state(1000, 2000, 1, 2);
        let out = quote(DexType::UniswapV2, &state, test_addr(2), test_addr(1), U256::from(100u64));
        assert_eq!(out, v2::amm_out(U256::from(100u64), U256::from(2000u64), U256::from(1000u64)));
    }

    #[test]
    fn test_quote_v2_wrong_tokens() {
        let state = make_v2_state(1000, 2000, 1, 2);
        let out = quote(DexType::UniswapV2, &state, test_addr(3), test_addr(1), U256::from(100u64));
        assert!(out.is_none());
    }

    #[test]
    fn test_quote_v2_wrong_state_type() {
        let state = PoolState::V3 {
            sqrt_price_x96: U256::from(1u64),
            fee_tier: 500,
            token0: test_addr(1),
            token1: test_addr(2),
            last_block: 100,
        };
        let out = quote(DexType::UniswapV2, &state, test_addr(1), test_addr(2), U256::from(100u64));
        assert!(out.is_none());
    }

    #[test]
    fn test_quote_curve_returns_none() {
        let state = make_v2_state(1000, 2000, 1, 2);
        let out = quote(DexType::CurveStableswap, &state, test_addr(1), test_addr(2), U256::from(100u64));
        assert!(out.is_none());
    }

    #[test]
    fn test_quote_v3_dispatch_works() {
        // price=4 (sqrtPrice=2), token0=addr(1), token1=addr(2)
        // token_in=addr(1)=token0 → zero_for_one=false (token0→token1, multiply by price)
        // out = 100 * 4 * (999500/1000000) = 399
        let state = PoolState::V3 {
            sqrt_price_x96: U256::from(2u64) * (U256::one() << 96u32),
            fee_tier: 500,
            token0: test_addr(1),
            token1: test_addr(2),
            last_block: 100,
        };
        let out = quote(DexType::UniswapV3, &state, test_addr(1), test_addr(2), U256::from(100u64));
        assert_eq!(out, Some(U256::from(399u64)));
    }

    #[test]
    fn test_quote_v3_wrong_state_type() {
        let state = make_v2_state(1000, 2000, 1, 2);
        let out = quote(DexType::UniswapV3, &state, test_addr(1), test_addr(2), U256::from(100u64));
        assert!(out.is_none());
    }

    #[test]
    fn test_quote_v3_wrong_tokens() {
        let state = PoolState::V3 {
            sqrt_price_x96: U256::from(2u64) * (U256::one() << 96u32),
            fee_tier: 500,
            token0: test_addr(1),
            token1: test_addr(2),
            last_block: 100,
        };
        let out = quote(DexType::UniswapV3, &state, test_addr(3), test_addr(1), U256::from(100u64));
        assert!(out.is_none());
    }

}
