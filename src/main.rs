mod arb;
mod config;
mod cross_chain;
mod db;
mod dex;
mod pools;
mod types;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ethers::providers::{Middleware, Provider, StreamExt, Ws};
use ethers::types::U256;
use eyre::{Result, WrapErr};
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use arb::{scan_all_opportunities, u256_to_f64};
use config::PoolCatalogEntry;
use cross_chain::{cross_chain_monitor, PriceRef};
use db::OpportunityDb;
use pools::{bootstrap_reserves, refresh_stale_pools, resolve_pool_catalog, run_subscriptions, verify_pool_tokens};
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

    let db = Arc::new(
        OpportunityDb::open("arb_opportunities.db")
            .wrap_err("failed to open opportunity database")?,
    );
    info!(path = "arb_opportunities.db", "database opened");

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
        if let Err(e) =
            spawn_chain_tasks(chain, ws_url, stats.clone(), price_ref, db.clone()).await
        {
            error!(chain = chain.name(), "chain startup failed, skipping: {e:#}");
        }
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

    // ── Phase 3: Research summary ──────────────────────────────────────────────
    let by_count = db.top_paths_by_count(5).unwrap_or_default();
    let by_roi = db.top_paths_by_avg_roi(5).unwrap_or_default();

    if by_count.is_empty() {
        println!("\nNo data recorded.");
    } else {
        println!("\n=== Research Summary ===");
        println!("Top paths by frequency:");
        for (i, (path, count)) in by_count.iter().enumerate() {
            println!("  {}.  {}     {} hits", i + 1, path, count);
        }
        println!("\nTop paths by avg ROI:");
        for (i, (path, roi)) in by_roi.iter().enumerate() {
            println!("  {}.  {}     {:.4}%", i + 1, path, roi);
        }
        let csv_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let csv_path = format!("arb_opportunities_{csv_ts}.csv");
        match db.export_csv(&csv_path) {
            Ok(()) => println!("\nExported: {csv_path}"),
            Err(e) => println!("\nCSV export failed: {e:#}"),
        }
        println!("========================");
    }

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
    db: Arc<OpportunityDb>,
) -> Result<()> {
    let provider = Arc::new(
        Provider::<Ws>::connect(&ws_url)
            .await
            .wrap_err_with(|| format!("WebSocket connect failed for {}", chain.name()))?,
    );
    info!(chain = chain.name(), "connected to node");

    // Resolve pool catalog from factory (Base/Polygon) or static config (Eth/Arb).
    let (catalog, paths) = resolve_pool_catalog(&provider, chain)
        .await
        .wrap_err_with(|| format!("pool catalog resolution failed for {}", chain.name()))?;
    info!(
        chain = chain.name(),
        pools = catalog.len(),
        "pool catalog resolved"
    );

    let catalog = Arc::new(catalog);
    let paths = Arc::new(paths);

    verify_pool_tokens(provider.clone(), &catalog)
        .await
        .wrap_err_with(|| format!("token verification failed for {}", chain.name()))?;
    info!(chain = chain.name(), "pool token orderings verified");

    let registry = bootstrap_reserves(provider.clone(), &catalog)
        .await
        .wrap_err_with(|| format!("bootstrap failed for {}", chain.name()))?;
    info!(chain = chain.name(), "reserves bootstrapped");

    let gas_price: Arc<RwLock<Option<U256>>> = Arc::new(RwLock::new(None));
    let current_block: Arc<RwLock<u64>> = Arc::new(RwLock::new(0));
    let (arb_tx, mut arb_rx) = watch::channel(());

    match provider.get_block_number().await {
        Ok(block) => {
            *current_block.write().await = block.as_u64();
        }
        Err(e) => {
            warn!(chain = chain.name(), "initial get_block_number failed: {e}");
        }
    }

    match provider.get_gas_price().await {
        Ok(price) => {
            *gas_price.write().await = Some(price);
        }
        Err(e) => {
            warn!(chain = chain.name(), "initial get_gas_price failed: {e}");
        }
    }

    // Subscription task (reconnect loop)
    {
        let ws_url_sub = ws_url.clone();
        let registry_sub = registry.clone();
        let catalog_sub = catalog.clone();
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
                            catalog_sub.clone(),
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
        let ws_url_blocks = ws_url.clone();
        let gas_price_blocks = gas_price.clone();
        let current_block_blocks = current_block.clone();
        let registry_blocks = registry.clone();
        let catalog_blocks = catalog.clone();
        let stats_blocks = stats.clone();
        let weth_usd_price_blocks = weth_usd_price.clone();
        let db_blocks = db.clone();
        let arb_tx_blocks = arb_tx.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match Provider::<Ws>::connect(&ws_url_blocks).await {
                    Err(e) => {
                        error!(
                            chain = chain.name(),
                            "block reconnect failed: {e:#}; retrying in {}s",
                            backoff.as_secs()
                        );
                        tokio::time::sleep(backoff).await;
                        backoff = (backoff * 2).min(Duration::from_secs(60));
                    }
                    Ok(provider_blocks) => {
                        let provider_blocks = Arc::new(provider_blocks);
                        let subscribe_result = provider_blocks.subscribe_blocks().await;
                        match subscribe_result {
                            Err(e) => {
                                error!(
                                    chain = chain.name(),
                                    "subscribe_blocks failed: {e:#}; retrying in {}s",
                                    backoff.as_secs()
                                );
                                tokio::time::sleep(backoff).await;
                                backoff = (backoff * 2).min(Duration::from_secs(60));
                            }
                            Ok(mut stream) => {
                                backoff = Duration::from_secs(1);
                                while let Some(block) = stream.next().await {
                                    let block_num = block.number.map(|b| b.as_u64()).unwrap_or(0);
                                    *current_block_blocks.write().await = block_num;

                                    let gp = match provider_blocks.get_gas_price().await {
                                        Ok(p) => {
                                            *gas_price_blocks.write().await = Some(p);
                                            p
                                        }
                                        Err(e) => {
                                            warn!(
                                                chain = chain.name(),
                                                "get_gas_price failed: {e}"
                                            );
                                            match *gas_price_blocks.read().await {
                                                Some(p) => p,
                                                None => continue,
                                            }
                                        }
                                    };

                                    refresh_stale_pools(
                                        provider_blocks.clone(),
                                        &registry_blocks,
                                        &catalog_blocks,
                                        block_num,
                                    )
                                    .await;

                                    let snap = registry_blocks.snapshot().await;
                                    let price_line =
                                        build_chain_price_line(chain, &snap, &catalog_blocks);

                                    if let Some(p) =
                                        chain_weth_usd_price(chain, &snap, &catalog_blocks)
                                    {
                                        *weth_usd_price_blocks.write().await = Some(p);
                                        if let Err(e) = db_blocks.insert_price_snapshot(
                                            chain.name(),
                                            block_num,
                                            p,
                                        ) {
                                            warn!(
                                                chain = chain.name(),
                                                "insert_price_snapshot failed: {e:#}"
                                            );
                                        }
                                    }

                                    info!(
                                        chain = chain.name(),
                                        block = block_num,
                                        gas_gwei = format!("{:.2}", u256_to_f64(gp) / 1e9),
                                        "{price_line}"
                                    );

                                    stats_blocks.write().await.blocks_scanned += 1;
                                    let _ = arb_tx_blocks.send(());
                                }

                                error!(
                                    chain = chain.name(),
                                    "block stream ended unexpectedly; reconnecting in {}s",
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

    // Arb task
    {
        let registry_arb = registry.clone();
        let gas_price_arb = gas_price.clone();
        let current_block_arb = current_block.clone();
        let catalog_arb = catalog.clone();
        let stats_arb = stats.clone();
        let paths_arb = paths.clone();
        let db_arb = db.clone();
        tokio::spawn(async move {
            loop {
                if arb_rx.changed().await.is_err() {
                    break;
                }

                let gp = match *gas_price_arb.read().await {
                    Some(p) => p,
                    None => continue,
                };

                let block = *current_block_arb.read().await;
                if block == 0 {
                    continue;
                }

                let snap = registry_arb.snapshot().await;
                let weth_price_usd = match chain_weth_usd_price(chain, &snap, &catalog_arb) {
                    Some(p) => p,
                    None => continue,
                };

                let opps = scan_all_opportunities(&snap, &paths_arb, gp, weth_price_usd, block);

                // Insert profitable opportunities before acquiring the stats lock.
                for opp in &opps {
                    if opp.estimated_net_after_gas > 0 {
                        if let Err(e) = db_arb.insert_opportunity(opp) {
                            warn!(chain = chain.name(), "insert_opportunity failed: {e:#}");
                        }
                    }
                }

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

    if *current_block.read().await > 0 && gas_price.read().await.is_some() {
        let _ = arb_tx.send(());
    }

    Ok(())
}

// ─── Price helpers ────────────────────────────────────────────────────────────

/// Derive the WETH/USD price from the runtime-resolved catalog's WETH/USDC pool.
fn chain_weth_usd_price(
    chain: ChainId,
    snap: &HashMap<PoolKey, PoolState>,
    catalog: &[PoolCatalogEntry],
) -> Option<f64> {
    let (key, usdc_is_token0) = config::weth_usdc_from_catalog(catalog)?;
    // Only use the pool if it belongs to this chain (sanity check).
    if key.chain != chain {
        return None;
    }
    if let PoolState::V2 {
        reserve0, reserve1, ..
    } = snap.get(&key)?
    {
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

fn build_chain_price_line(
    chain: ChainId,
    snap: &HashMap<PoolKey, PoolState>,
    catalog: &[PoolCatalogEntry],
) -> String {
    match chain_weth_usd_price(chain, snap, catalog) {
        Some(p) => format!("WETH: ${p:.2}"),
        None => "WETH: N/A".to_string(),
    }
}
