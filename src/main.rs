mod arb;
mod config;
mod dex;
mod pools;
mod types;

use std::sync::Arc;
use std::time::{Duration, Instant};

use ethers::providers::{Middleware, Provider, StreamExt, Ws};
use ethers::types::U256;
use eyre::{Result, WrapErr};
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use arb::{scan_all_opportunities, u256_to_f64};
use config::ethereum::{pool_key_dai_weth, pool_key_usdc_dai, pool_key_weth_usdc};
use pools::{bootstrap_reserves, refresh_stale_pools, run_subscriptions, verify_pool_tokens};
use types::{ChainId, PoolKey, PoolState, SessionStats};

#[tokio::main]
async fn main() -> Result<()> {
    // Logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load .env if present
    let _ = dotenv::dotenv();

    let ws_url = std::env::var("ETH_WS_URL")
        .wrap_err("ETH_WS_URL not set — copy .env.example to .env and fill in your key")?;

    info!("arb-bot starting");

    // ── Connect ───────────────────────────────────────────────────────────────
    let provider = Arc::new(
        Provider::<Ws>::connect(&ws_url)
            .await
            .wrap_err("WebSocket connect failed")?,
    );
    info!("connected to Ethereum node");

    // ── Startup: verify token ordering on-chain ───────────────────────────────
    verify_pool_tokens(provider.clone(), ChainId::Ethereum)
        .await
        .wrap_err("token0/token1 verification failed — check pool addresses")?;
    info!("all pool token orderings verified");

    // ── Startup: bootstrap reserves ───────────────────────────────────────────
    let registry = bootstrap_reserves(provider.clone(), ChainId::Ethereum)
        .await
        .wrap_err("failed to bootstrap reserves")?;
    info!("reserves bootstrapped");

    // ── Arb paths (computed once at startup) ──────────────────────────────────
    let eth_paths = Arc::new(config::arb_paths(ChainId::Ethereum));

    // ── Shared state ──────────────────────────────────────────────────────────
    let gas_price: Arc<RwLock<Option<U256>>> = Arc::new(RwLock::new(None));
    let stats: Arc<RwLock<SessionStats>> = Arc::new(RwLock::new(SessionStats::default()));
    let start_time = Instant::now();

    // Channel: Sync events → arb recalculation
    let (arb_tx, mut arb_rx) = watch::channel(());

    // ── Sync event subscription task (with reconnect loop) ────────────────────
    let ws_url_sub = ws_url.clone();
    let registry_sub = registry.clone();
    let arb_tx_sub = arb_tx.clone();
    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            match Provider::<Ws>::connect(&ws_url_sub).await {
                Err(e) => {
                    error!("WebSocket reconnect failed: {e:#}; retrying in {}s", backoff.as_secs());
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                }
                Ok(fresh_provider) => {
                    let fresh_provider = Arc::new(fresh_provider);
                    match run_subscriptions(
                        fresh_provider,
                        registry_sub.clone(),
                        ChainId::Ethereum,
                        arb_tx_sub.clone(),
                    )
                    .await
                    {
                        Ok(_) => break,
                        Err(e) => {
                            error!("subscription error: {e:#}; reconnecting in {}s", backoff.as_secs());
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(60));
                        }
                    }
                }
            }
        }
    });

    // ── Block subscription task ───────────────────────────────────────────────
    let provider_blocks = provider.clone();
    let gas_price_blocks = gas_price.clone();
    let registry_blocks = registry.clone();
    let stats_blocks = stats.clone();
    tokio::spawn(async move {
        match provider_blocks.subscribe_blocks().await {
            Err(e) => error!("subscribe_blocks failed: {e}"),
            Ok(mut stream) => {
                while let Some(block) = stream.next().await {
                    let block_num = block.number.map(|b| b.as_u64()).unwrap_or(0);

                    // Fetch and cache gas price
                    let gp = match provider_blocks.get_gas_price().await {
                        Ok(p) => {
                            *gas_price_blocks.write().await = Some(p);
                            p
                        }
                        Err(e) => {
                            warn!("get_gas_price failed: {e}");
                            match *gas_price_blocks.read().await {
                                Some(p) => p,
                                None => continue,
                            }
                        }
                    };

                    // Refresh stale pools
                    refresh_stale_pools(provider_blocks.clone(), &registry_blocks, ChainId::Ethereum, block_num).await;

                    // Display prices from current reserves
                    let snap = registry_blocks.snapshot().await;
                    let price_line = build_price_line(&snap);

                    info!(
                        block = block_num,
                        gas_gwei = format!("{:.2}", u256_to_f64(gp) / 1e9),
                        "{price_line}"
                    );

                    stats_blocks.write().await.blocks_scanned += 1;
                }
            }
        }
    });

    // ── Arb calculation task ──────────────────────────────────────────────────
    let registry_arb = registry.clone();
    let gas_price_arb = gas_price.clone();
    let stats_arb = stats.clone();
    let paths_arb = eth_paths.clone();
    tokio::spawn(async move {
        loop {
            if arb_rx.changed().await.is_err() {
                break;
            }

            let gp = match *gas_price_arb.read().await {
                Some(p) => p,
                None => continue,
            };

            let snap = registry_arb.snapshot().await;
            let weth_price_usd = match weth_price_from_pools(&snap) {
                Some(p) => p,
                None => continue,
            };

            let opps = scan_all_opportunities(&snap, &paths_arb, gp, weth_price_usd);
            let mut s = stats_arb.write().await;

            for opp in opps {
                if opp.estimated_net_after_gas > 0 {
                    s.opps_found += 1;

                    info!(
                        chain = opp.chain.name(),
                        path = opp.path,
                        input_eth = format!("{:.4}", u256_to_f64(opp.input_weth) / 1e18),
                        roi_pct = format!("{:.4}", opp.roi_pct),
                        estimated_net_after_gas_wei = opp.estimated_net_after_gas,
                        gas_cost_usd = format!("{:.2}", opp.gas_cost_usd),
                        "[OPPORTUNITY]"
                    );

                    let is_better = s
                        .best_opp
                        .as_ref()
                        .map(|b| opp.estimated_net_after_gas > b.estimated_net_after_gas)
                        .unwrap_or(true);
                    if is_better {
                        s.best_opp = Some(opp);
                    }
                }
            }
        }
    });

    // ── Wait for Ctrl+C ───────────────────────────────────────────────────────
    tokio::signal::ctrl_c()
        .await
        .wrap_err("failed to listen for ctrl_c")?;

    // ── Session summary ───────────────────────────────────────────────────────
    let elapsed = start_time.elapsed();
    let s = stats.read().await;
    println!("\n=== Session Summary ===");
    println!("Blocks scanned:       {}", s.blocks_scanned);
    println!("Opportunities found:  {}", s.opps_found);
    match &s.best_opp {
        None => println!("Best opportunity:     none"),
        Some(opp) => {
            println!("Best opportunity:");
            println!("  Chain:                {}", opp.chain.name());
            println!("  Path:                 {}", opp.path);
            println!("  Input:                {:.4} ETH", u256_to_f64(opp.input_weth) / 1e18);
            println!("  Estimated net (wei):  {}", opp.estimated_net_after_gas);
            println!("  ROI:                  {:.4}%", opp.roi_pct);
            println!("  Gas cost:             ${:.2}", opp.gas_cost_usd);
        }
    }
    println!("Runtime:              {:.1}s", elapsed.as_secs_f64());
    println!("======================");

    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn weth_price_from_pools(snap: &std::collections::HashMap<PoolKey, PoolState>) -> Option<f64> {
    let state = snap.get(&pool_key_weth_usdc())?;
    if let PoolState::V2 { reserve0, reserve1, .. } = state {
        if reserve1.is_zero() {
            return None;
        }
        // token0=USDC (6 dec), token1=WETH (18 dec)
        // price = (usdc_raw / 1e6) / (weth_raw / 1e18) = usdc_raw * 1e12 / weth_raw
        let usdc_raw = u256_to_f64(*reserve0);
        let weth_raw = u256_to_f64(*reserve1);
        Some(usdc_raw * 1e12 / weth_raw)
    } else {
        None
    }
}

fn build_price_line(snap: &std::collections::HashMap<PoolKey, PoolState>) -> String {
    let weth_usdc = format_pool_price(
        snap.get(&pool_key_weth_usdc()),
        "WETH/USDC",
        // token0=USDC (6dec), token1=WETH (18dec) → price = r0 * 1e12 / r1
        |r0, r1| if r1 == 0.0 { None } else { Some(r0 * 1e12 / r1) },
        "$",
        ".2",
    );

    let usdc_dai = snap
        .get(&pool_key_usdc_dai())
        .and_then(|s| {
            if let PoolState::V2 { reserve0, reserve1, .. } = s {
                if reserve0.is_zero() {
                    return None;
                }
                // token0=DAI (18dec), token1=USDC (6dec)
                let usdc_in_dai = u256_to_f64(*reserve0) / (u256_to_f64(*reserve1) * 1e12);
                Some(format!("USDC/DAI: {usdc_in_dai:.4}"))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "USDC/DAI: N/A".to_string());

    let dai_weth = snap
        .get(&pool_key_dai_weth())
        .and_then(|s| {
            if let PoolState::V2 { reserve0, reserve1, .. } = s {
                if reserve1.is_zero() {
                    return None;
                }
                // token0=DAI (18dec), token1=WETH (18dec)
                let price = u256_to_f64(*reserve0) / u256_to_f64(*reserve1);
                Some(format!("DAI/WETH: {price:.2}"))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "DAI/WETH: N/A".to_string());

    format!("{weth_usdc} | {usdc_dai} | {dai_weth}")
}

fn format_pool_price<F>(
    state: Option<&PoolState>,
    label: &str,
    compute: F,
    prefix: &str,
    fmt: &str,
) -> String
where
    F: Fn(f64, f64) -> Option<f64>,
{
    state
        .and_then(|s| {
            if let PoolState::V2 { reserve0, reserve1, .. } = s {
                let price = compute(u256_to_f64(*reserve0), u256_to_f64(*reserve1))?;
                Some(if fmt == ".2" {
                    format!("{label}: {prefix}{price:.2}")
                } else {
                    format!("{label}: {prefix}{price:.4}")
                })
            } else {
                None
            }
        })
        .unwrap_or_else(|| format!("{label}: N/A"))
}
