use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tracing::info;

use crate::types::ChainId;

// ─── Types ────────────────────────────────────────────────────────────────────

pub type PriceRef = Arc<RwLock<Option<f64>>>;

// ─── Spread calculation ───────────────────────────────────────────────────────

/// Returns the percentage spread between the highest and lowest price.
/// `(max - min) / min * 100.0`
/// Returns None if fewer than 2 prices are present or min is zero.
pub fn compute_spread_pct(prices: &[(ChainId, f64)]) -> Option<f64> {
    if prices.len() < 2 {
        return None;
    }
    let min = prices
        .iter()
        .map(|(_, p)| *p)
        .fold(f64::INFINITY, f64::min);
    let max = prices
        .iter()
        .map(|(_, p)| *p)
        .fold(f64::NEG_INFINITY, f64::max);
    if min == 0.0 {
        return None;
    }
    Some((max - min) / min * 100.0)
}

// ─── Monitor task ─────────────────────────────────────────────────────────────

/// Periodically reads WETH/USD prices from all active chains and logs an alert
/// when the spread between the highest and lowest price exceeds `threshold_pct`.
pub async fn cross_chain_monitor(price_refs: Vec<(ChainId, PriceRef)>, threshold_pct: f64) {
    let mut interval = tokio::time::interval(Duration::from_secs(10));
    loop {
        interval.tick().await;

        let mut available: Vec<(ChainId, f64)> = Vec::new();
        for (chain, price_ref) in &price_refs {
            if let Some(p) = *price_ref.read().await {
                available.push((*chain, p));
            }
        }

        if available.len() < 2 {
            continue;
        }

        if let Some(spread) = compute_spread_pct(&available) {
            if spread > threshold_pct {
                let price_str: Vec<String> = available
                    .iter()
                    .map(|(c, p)| format!("{}=${:.2}", c.name(), p))
                    .collect();
                info!(
                    spread = format!("{:.4}%", spread),
                    "[CROSS-CHAIN ALERT] {}",
                    price_str.join(" ")
                );
            }
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spread_none_on_empty() {
        assert!(compute_spread_pct(&[]).is_none());
    }

    #[test]
    fn test_spread_none_on_single_price() {
        let prices = vec![(ChainId::Ethereum, 3000.0)];
        assert!(compute_spread_pct(&prices).is_none());
    }

    #[test]
    fn test_spread_zero_when_prices_equal() {
        let prices = vec![
            (ChainId::Ethereum, 3000.0),
            (ChainId::Arbitrum, 3000.0),
        ];
        assert_eq!(compute_spread_pct(&prices), Some(0.0));
    }

    #[test]
    fn test_spread_computed_correctly() {
        // (3012 - 3000) / 3000 * 100 = 0.4%
        let prices = vec![
            (ChainId::Ethereum, 3000.0),
            (ChainId::Arbitrum, 3012.0),
        ];
        let spread = compute_spread_pct(&prices).unwrap();
        assert!((spread - 0.4).abs() < 1e-9);
    }

    #[test]
    fn test_spread_alert_fires_above_threshold() {
        let prices = vec![
            (ChainId::Ethereum, 3000.0),
            (ChainId::Arbitrum, 3012.0),
        ];
        let spread = compute_spread_pct(&prices).unwrap();
        // threshold 0.1% — spread 0.4% should exceed it
        assert!(spread > 0.1);
    }

    #[test]
    fn test_spread_no_alert_below_threshold() {
        // 0.05% spread should not exceed 0.1% threshold
        let prices = vec![
            (ChainId::Ethereum, 3000.0),
            (ChainId::Arbitrum, 3001.5),
        ];
        let spread = compute_spread_pct(&prices).unwrap();
        assert!(spread < 0.1);
    }

    #[test]
    fn test_spread_with_four_chains() {
        let prices = vec![
            (ChainId::Ethereum, 3000.0),
            (ChainId::Arbitrum, 3006.0),
            (ChainId::Base, 2994.0),
            (ChainId::Polygon, 3003.0),
        ];
        // max=3006 min=2994 => (12/2994)*100 = 0.4008...%
        let spread = compute_spread_pct(&prices).unwrap();
        assert!(spread > 0.40 && spread < 0.41);
    }

    #[test]
    fn test_spread_none_when_min_is_zero() {
        let prices = vec![
            (ChainId::Ethereum, 0.0),
            (ChainId::Arbitrum, 3000.0),
        ];
        assert!(compute_spread_pct(&prices).is_none());
    }
}
