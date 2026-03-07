use std::sync::Arc;

use ethers::abi::Abi;
use ethers::contract::Contract;
use ethers::providers::{Provider, Ws};
use ethers::types::{Address, I256, U256};

// ─── ABI fragment ─────────────────────────────────────────────────────────────

const CURVE_ABI: &str = r#"[
    {
        "name": "get_dy",
        "type": "function",
        "inputs": [
            {"name": "i", "type": "int128"},
            {"name": "j", "type": "int128"},
            {"name": "dx", "type": "uint256"}
        ],
        "outputs": [{"name": "", "type": "uint256"}],
        "stateMutability": "view"
    }
]"#;

fn parse_curve_abi() -> Abi {
    serde_json::from_str(CURVE_ABI).expect("hardcoded Curve ABI is valid JSON")
}

// ─── Validation ───────────────────────────────────────────────────────────────

/// Returns true if the parameters are valid for a Curve get_dy call.
fn validate_curve_params(i: i128, j: i128, dx: U256) -> bool {
    !dx.is_zero() && i >= 0 && j >= 0 && i != j
}

// ─── Quote ────────────────────────────────────────────────────────────────────

/// Query a Curve pool's `get_dy(i, j, dx)` for an estimated output amount.
///
/// `i` and `j` are coin indices (0-indexed, must be non-negative and distinct).
/// `dx` is the input amount in the token's native units.
///
/// Returns None on invalid parameters or RPC error.
pub async fn curve_quote(
    provider: Arc<Provider<Ws>>,
    pool_addr: Address,
    i: i128,
    j: i128,
    dx: U256,
) -> Option<U256> {
    if !validate_curve_params(i, j, dx) {
        return None;
    }
    let abi = parse_curve_abi();
    let contract = Contract::new(pool_addr, abi, provider);
    contract
        .method::<_, U256>("get_dy", (I256::from(i), I256::from(j), dx))
        .ok()?
        .call()
        .await
        .ok()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_zero_dx() {
        assert!(!validate_curve_params(0, 1, U256::zero()));
    }

    #[test]
    fn test_validate_same_indices() {
        assert!(!validate_curve_params(1, 1, U256::from(100u64)));
    }

    #[test]
    fn test_validate_negative_i() {
        assert!(!validate_curve_params(-1, 1, U256::from(100u64)));
    }

    #[test]
    fn test_validate_negative_j() {
        assert!(!validate_curve_params(0, -1, U256::from(100u64)));
    }

    #[test]
    fn test_validate_valid_params() {
        assert!(validate_curve_params(0, 1, U256::from(1_000_000u64)));
        assert!(validate_curve_params(1, 0, U256::from(1_000_000u64)));
        assert!(validate_curve_params(0, 2, U256::from(1u64)));
    }
}
