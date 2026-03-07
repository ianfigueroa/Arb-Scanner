mod arb;
mod config;
mod cross_chain;
mod dex;
mod pools;
mod types;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ethers::providers::{Middleware, Provider, StreamExt, Ws};
use ethers::types::U256;
use eyre::{Result, WrapErr};
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use arb::{scan_all_opportunities, u256_to_f64};
use cross_chain::{cross_chain_monitor, PriceRef};
use pools::{bootstrap_reserves, refresh_stale_pools, run_subscriptions, verify_pool_tokens};
use types::{ChainId, PoolKey, PoolState, SessionStats};

// ─── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let _ = dotenv::dotenv();

    let chains = collect_active_chains();
    if chains.is_empty() {
        warn!(
            "no chain WS URLs configured — set ETH_WS_URL, ARBITRUM_WS_URL, \
             BASE_WS_URL, or POLYGON_WS_URL"
        );
        return Ok(());
    }

    info!("arb-bot starting on {} chain(s)", chains.len());

    let stats: Arc<RwLock<SessionStats>> = Arc::new(RwLock::new(SessionStats::default()));
    let start_time = Instant::now();

    let threshold_pct = std::env::var("CROSS_CHAIN_THRESHOLD_PCT")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.1);

    let mut price_refs: Vec<(ChainId, PriceRef)> = Vec::new();

    for (chain, ws_url) in chains {
        let price_ref: PriceRef = Arc::new(RwLock::new(None));
        price_refs.push((chain, price_ref.clone()));
        spawn_chain_tasks(chain, ws_url, stats.clone(), price_ref)
            .await
            .wrap_err_with(|| format!("failed to start {} scanner", chain.name()))?;
    }

    tokio::spawn(cross_chain_monitor(price_refs, threshold_pct));

    tokio::signal::ctrl_c()
        .await
        .wrap_err("failed to listen for ctrl_c")?;

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

// ─── Chain discovery ──────────────────────────────────────────────────────────

fn collect_active_chains() -> Vec<(ChainId, String)> {
    let vars = [
        (ChainId::Ethereum, "ETH_WS_URL"),
        (ChainId::Arbitrum, "ARBITRUM_WS_URL"),
        (ChainId::Base, "BASE_WS_URL"),
        (ChainId::Polygon, "POLYGON_WS_URL"),
    ];
    let mut result = Vec::new();
    for (chain, var) in &vars {
        match std::env::var(var) {
            Ok(url) => {
                info!(chain = chain.name(), "chain enabled");
                result.push((*chain, url));
            }
            Err(_) => {
                info!(chain = chain.name(), var, "no WS URL — chain skipped");
            }
        }
    }
    result
}

// ─── Per-chain task spawner ───────────────────────────────────────────────────

/// Bootstrap a chain and spawn its subscription, block, and arb tasks.
/// Returns after spawning (tasks run independently in background).
async fn spawn_chain_tasks(
    chain: ChainId,
    ws_url: String,
    stats: Arc<RwLock<SessionStats>>,
    weth_usd_price: PriceRef,
) -> Result<()> {
    let provider = Arc::new(
        Provider::<Ws>::connect(&ws_url)
            .await
            .wrap_err_with(|| format!("WebSocket connect failed for {}", chain.name()))?,
    );
    info!(chain = chain.name(), "connected to node");

    verify_pool_tokens(provider.clone(), chain)
        .await
        .wrap_err_with(|| format!("token verification failed for {}", chain.name()))?;
    info!(chain = chain.name(), "pool token orderings verified");

    let registry = bootstrap_reserves(provider.clone(), chain)
        .await
        .wrap_err_with(|| format!("bootstrap failed for {}", chain.name()))?;
    info!(chain = chain.name(), "reserves bootstrapped");

    let paths = Arc::new(config::arb_paths(chain));
    let gas_price: Arc<RwLock<Option<U256>>> = Arc::new(RwLock::new(None));
    let (arb_tx, mut arb_rx) = watch::channel(());

    // Subscription task (reconnect loop)
    {
        let ws_url_sub = ws_url.clone();
        let registry_sub = registry.clone();
        let arb_tx_sub = arb_tx.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match Provider::<Ws>::connect(&ws_url_sub).await {
                    Err(e) => {
                        error!(
                            chain = chain.name(),
                            "reconnect failed: {e:#}; retrying in {}s",
                            backoff.as_secs()
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                    }
                    Ok(fresh_provider) => {
                        let fresh_provider = Arc::new(fresh_provider);
                        match run_subscriptions(
                            fresh_provider,
                            registry_sub.clone(),
                            chain,
                            arb_tx_sub.clone(),
                        )
                        .await
                        {
                            Ok(_) => break,
                            Err(e) => {
                                error!(
                                    chain = chain.name(),
                                    "subscription error: {e:#}; reconnecting in {}s",
                                    backoff.as_secs()
                                );
                                tokio::time::sleep(backoff).await;
                                backoff = (backoff * 2).min(Duration::from_secs(60));
                            }
                        }
                    }
                }
            }
        });
    }

    // Block task (gas price + stale refresh + price display)
    {
        let provider_blocks = provider.clone();
        let gas_price_blocks = gas_price.clone();
        let registry_blocks = registry.clone();
        let stats_blocks = stats.clone();
        let weth_usd_price_blocks = weth_usd_price.clone();
        tokio::spawn(async move {
            match provider_blocks.subscribe_blocks().await {
                Err(e) => error!(chain = chain.name(), "subscribe_blocks failed: {e}"),
                Ok(mut stream) => {
                    while let Some(block) = stream.next().await {
                        let block_num = block.number.map(|b| b.as_u64()).unwrap_or(0);

                        let gp = match provider_blocks.get_gas_price().await {
                            Ok(p) => {
                                *gas_price_blocks.write().await = Some(p);
                                p
                            }
                            Err(e) => {
                                warn!(chain = chain.name(), "get_gas_price failed: {e}");
                                match *gas_price_blocks.read().await {
                                    Some(p) => p,
                                    None => continue,
                                }
                            }
                        };

                        refresh_stale_pools(
                            provider_blocks.clone(),
                            &registry_blocks,
                            chain,
                            block_num,
                        )
                        .await;

                        let snap = registry_blocks.snapshot().await;
                        let price_line = build_chain_price_line(chain, &snap);

                        if let Some(p) = chain_weth_usd_price(chain, &snap) {
                            *weth_usd_price_blocks.write().await = Some(p);
                        }

                        info!(
                            chain = chain.name(),
                            block = block_num,
                            gas_gwei = format!("{:.2}", u256_to_f64(gp) / 1e9),
                            "{price_line}"
                        );

                        stats_blocks.write().await.blocks_scanned += 1;
                    }
                }
            }
        });
    }

    // Arb task
    {
        let registry_arb = registry.clone();
        let gas_price_arb = gas_price.clone();
        let stats_arb = stats.clone();
        let paths_arb = paths.clone();
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
                let weth_price_usd = match chain_weth_usd_price(chain, &snap) {
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
    }

    Ok(())
}

// ─── Price helpers ────────────────────────────────────────────────────────────

/// Derive the WETH/USD price for any chain from its primary WETH/stable V2 pool.
///
/// Uses `config::weth_usdc_pool_info` to identify which reserve is USDC (6 dec)
/// and which is WETH (18 dec), then computes: usdc_raw * 1e12 / weth_raw.
fn chain_weth_usd_price(chain: ChainId, snap: &HashMap<PoolKey, PoolState>) -> Option<f64> {
    let (key, usdc_is_token0) = config::weth_usdc_pool_info(chain)?;
    if let PoolState::V2 { reserve0, reserve1, .. } = snap.get(&key)? {
        let (usdc_raw, weth_raw) = if usdc_is_token0 {
            (u256_to_f64(*reserve0), u256_to_f64(*reserve1))
        } else {
            (u256_to_f64(*reserve1), u256_to_f64(*reserve0))
        };
        if weth_raw == 0.0 {
            return None;
        }
        Some(usdc_raw * 1e12 / weth_raw)
    } else {
        None
    }
}

fn build_chain_price_line(chain: ChainId, snap: &HashMap<PoolKey, PoolState>) -> String {
    match chain_weth_usd_price(chain, snap) {
        Some(p) => format!("WETH: ${p:.2}"),
        None => "WETH: N/A".to_string(),
    }
}
