use std::collections::HashMap;
use std::sync::Arc;

use ethers::abi::Abi;
use ethers::contract::Contract;
use ethers::providers::{Middleware, Provider, StreamExt, Ws};
use ethers::types::{Address, Filter, Log, U256};
use eyre::{eyre, Result, WrapErr};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::{pool_configs, PoolConfig};
use crate::types::{PoolId, PoolState};

// ─── ABI fragment ─────────────────────────────────────────────────────────────

const PAIR_ABI: &str = r#"[
    {"inputs":[],"name":"token0","outputs":[{"internalType":"address","name":"","type":"address"}],"stateMutability":"view","type":"function"},
    {"inputs":[],"name":"token1","outputs":[{"internalType":"address","name":"","type":"address"}],"stateMutability":"view","type":"function"},
    {"inputs":[],"name":"getReserves","outputs":[{"internalType":"uint112","name":"_reserve0","type":"uint112"},{"internalType":"uint112","name":"_reserve1","type":"uint112"},{"internalType":"uint32","name":"_blockTimestampLast","type":"uint32"}],"stateMutability":"view","type":"function"},
    {"anonymous":false,"inputs":[{"indexed":false,"internalType":"uint112","name":"reserve0","type":"uint112"},{"indexed":false,"internalType":"uint112","name":"reserve1","type":"uint112"}],"name":"Sync","type":"event"}
]"#;

fn parse_abi() -> Abi {
    serde_json::from_str(PAIR_ABI).expect("hardcoded ABI is valid JSON")
}

// ─── PoolRegistry ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PoolRegistry {
    inner: Arc<RwLock<HashMap<PoolId, PoolState>>>,
}

impl PoolRegistry {
    fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, id: PoolId) -> Option<PoolState> {
        self.inner.read().await.get(&id).cloned()
    }

    pub async fn snapshot(&self) -> HashMap<PoolId, PoolState> {
        self.inner.read().await.clone()
    }

    pub async fn update(&self, id: PoolId, state: PoolState) {
        self.inner.write().await.insert(id, state);
    }
}

// ─── Startup: verify token ordering on-chain ─────────────────────────────────

pub async fn verify_pool_tokens(provider: Arc<Provider<Ws>>) -> Result<()> {
    let abi = parse_abi();
    let configs = pool_configs();

    for cfg in &configs {
        let contract = Contract::new(cfg.pair_address, abi.clone(), provider.clone());

        let token0: Address = contract
            .method::<_, Address>("token0", ())?
            .call()
            .await
            .wrap_err_with(|| format!("token0() call failed for {}", cfg.pool_id.name()))?;

        let token1: Address = contract
            .method::<_, Address>("token1", ())?
            .call()
            .await
            .wrap_err_with(|| format!("token1() call failed for {}", cfg.pool_id.name()))?;

        if token0 != cfg.expected_token0 {
            return Err(eyre!(
                "Pool {} token0 mismatch: on-chain={:?} expected={:?}",
                cfg.pool_id.name(),
                token0,
                cfg.expected_token0
            ));
        }
        if token1 != cfg.expected_token1 {
            return Err(eyre!(
                "Pool {} token1 mismatch: on-chain={:?} expected={:?}",
                cfg.pool_id.name(),
                token1,
                cfg.expected_token1
            ));
        }

        info!(
            pool = cfg.pool_id.name(),
            token0 = cfg.token0_symbol,
            token1 = cfg.token1_symbol,
            "token ordering verified"
        );
    }

    Ok(())
}

// ─── Startup: bootstrap reserves ─────────────────────────────────────────────

pub async fn bootstrap_reserves(provider: Arc<Provider<Ws>>) -> Result<PoolRegistry> {
    let abi = parse_abi();
    let configs = pool_configs();

    let current_block = provider
        .get_block_number()
        .await
        .wrap_err("get_block_number")?
        .as_u64();

    // Fetch reserves for all 3 pools concurrently
    let (r0, r1, r2) = tokio::join!(
        fetch_reserves(&abi, provider.clone(), &configs[0]),
        fetch_reserves(&abi, provider.clone(), &configs[1]),
        fetch_reserves(&abi, provider.clone(), &configs[2]),
    );

    let registry = PoolRegistry::new();

    for (result, cfg) in [
        (r0, &configs[0]),
        (r1, &configs[1]),
        (r2, &configs[2]),
    ] {
        let (reserve0, reserve1) = result.wrap_err_with(|| {
            format!("bootstrap getReserves failed for {}", cfg.pool_id.name())
        })?;

        info!(
            pool = cfg.pool_id.name(),
            reserve0 = %reserve0,
            reserve1 = %reserve1,
            block = current_block,
            "bootstrapped reserves"
        );

        registry
            .update(
                cfg.pool_id,
                PoolState {
                    reserve0,
                    reserve1,
                    last_block: current_block,
                },
            )
            .await;
    }

    Ok(registry)
}

async fn fetch_reserves(abi: &Abi, provider: Arc<Provider<Ws>>, cfg: &PoolConfig) -> Result<(U256, U256)> {
    let contract = Contract::new(cfg.pair_address, abi.clone(), provider);
    let (r0, r1, _ts): (u128, u128, u32) = contract
        .method::<_, (u128, u128, u32)>("getReserves", ())?
        .call()
        .await
        .wrap_err("getReserves()")?;
    Ok((U256::from(r0), U256::from(r1)))
}

// ─── Sync event subscriptions ────────────────────────────────────────────────

/// Subscribes to Sync events for all 3 pools. Returns on stream end or error.
/// Caller wraps in a reconnect loop.
pub async fn run_subscriptions(
    provider: Arc<Provider<Ws>>,
    registry: PoolRegistry,
    arb_tx: tokio::sync::watch::Sender<()>,
) -> Result<()> {
    let configs = pool_configs();
    let pair_addresses: Vec<Address> = configs.iter().map(|c| c.pair_address).collect();
    let sync_topic = ethers::core::utils::keccak256("Sync(uint112,uint112)");

    let filter = Filter::new()
        .address(pair_addresses)
        .topic0(ethers::types::H256::from(sync_topic));

    let mut stream = provider
        .subscribe_logs(&filter)
        .await
        .wrap_err("subscribe_logs")?;

    info!("subscribed to Sync events for all 3 pools");

    while let Some(log) = stream.next().await {
        handle_sync_log(log, &registry).await;
        let _ = arb_tx.send(());
    }

    Err(eyre!("Sync log stream ended unexpectedly"))
}

async fn handle_sync_log(log: Log, registry: &PoolRegistry) {
    let address = log.address;
    let block_number = match log.block_number {
        Some(b) => b.as_u64(),
        None => {
            warn!("Sync log missing block_number, skipping");
            return;
        }
    };

    let pool_id = pool_configs()
        .iter()
        .find(|c| c.pair_address == address)
        .map(|c| c.pool_id);

    let Some(pool_id) = pool_id else {
        warn!(?address, "received Sync from unknown address");
        return;
    };

    if log.data.len() < 64 {
        warn!(pool = pool_id.name(), "Sync log data too short");
        return;
    }
    let reserve0 = U256::from_big_endian(&log.data[0..32]);
    let reserve1 = U256::from_big_endian(&log.data[32..64]);

    registry
        .update(
            pool_id,
            PoolState {
                reserve0,
                reserve1,
                last_block: block_number,
            },
        )
        .await;
}

// ─── Stale reserve refresh ────────────────────────────────────────────────────

const STALE_BLOCK_THRESHOLD: u64 = 50;

/// For any pool whose last_block is more than STALE_BLOCK_THRESHOLD behind
/// current_block, refresh via getReserves().
pub async fn refresh_stale_pools(
    provider: Arc<Provider<Ws>>,
    registry: &PoolRegistry,
    current_block: u64,
) {
    let abi = parse_abi();
    let configs = pool_configs();

    for cfg in &configs {
        let stale = registry
            .get(cfg.pool_id)
            .await
            .map(|s| current_block.saturating_sub(s.last_block) > STALE_BLOCK_THRESHOLD)
            .unwrap_or(true);

        if stale {
            warn!(
                pool = cfg.pool_id.name(),
                current_block,
                "pool is stale, refreshing via getReserves()"
            );
            match fetch_reserves(&abi, provider.clone(), cfg).await {
                Ok((r0, r1)) => {
                    registry
                        .update(
                            cfg.pool_id,
                            PoolState {
                                reserve0: r0,
                                reserve1: r1,
                                last_block: current_block,
                            },
                        )
                        .await;
                }
                Err(e) => error!(pool = cfg.pool_id.name(), "refresh failed: {e}"),
            }
        }
    }
}
