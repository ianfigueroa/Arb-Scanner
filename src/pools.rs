use std::collections::HashMap;
use std::sync::Arc;

use ethers::abi::Abi;
use ethers::contract::Contract;
use ethers::providers::{Middleware, Provider, StreamExt, Ws};
use ethers::types::{Address, Filter, Log, U256};
use eyre::{eyre, Result, WrapErr};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::{pool_catalog, PoolCatalogEntry};
use crate::types::{ChainId, PoolKey, PoolState};

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
    inner: Arc<RwLock<HashMap<PoolKey, PoolState>>>,
}

impl PoolRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get(&self, key: PoolKey) -> Option<PoolState> {
        self.inner.read().await.get(&key).cloned()
    }

    pub async fn snapshot(&self) -> HashMap<PoolKey, PoolState> {
        self.inner.read().await.clone()
    }

    pub async fn update(&self, key: PoolKey, state: PoolState) {
        self.inner.write().await.insert(key, state);
    }
}

// ─── Startup: verify token ordering on-chain ─────────────────────────────────

pub async fn verify_pool_tokens(provider: Arc<Provider<Ws>>, chain: ChainId) -> Result<()> {
    let abi = parse_abi();
    let configs = pool_catalog(chain);

    for cfg in &configs {
        let contract = Contract::new(cfg.pool_key.address, abi.clone(), provider.clone());

        let token0: Address = contract
            .method::<_, Address>("token0", ())?
            .call()
            .await
            .wrap_err_with(|| format!("token0() call failed for {}", cfg.name))?;

        let token1: Address = contract
            .method::<_, Address>("token1", ())?
            .call()
            .await
            .wrap_err_with(|| format!("token1() call failed for {}", cfg.name))?;

        if token0 != cfg.expected_token0 {
            return Err(eyre!(
                "Pool {} token0 mismatch: on-chain={:?} expected={:?}",
                cfg.name,
                token0,
                cfg.expected_token0
            ));
        }
        if token1 != cfg.expected_token1 {
            return Err(eyre!(
                "Pool {} token1 mismatch: on-chain={:?} expected={:?}",
                cfg.name,
                token1,
                cfg.expected_token1
            ));
        }

        info!(
            pool = cfg.name,
            token0 = cfg.token0_symbol,
            token1 = cfg.token1_symbol,
            "token ordering verified"
        );
    }

    Ok(())
}

// ─── Startup: bootstrap reserves ─────────────────────────────────────────────

pub async fn bootstrap_reserves(
    provider: Arc<Provider<Ws>>,
    chain: ChainId,
) -> Result<PoolRegistry> {
    let abi = parse_abi();
    let configs = pool_catalog(chain);

    let current_block = provider
        .get_block_number()
        .await
        .wrap_err("get_block_number")?
        .as_u64();

    let registry = PoolRegistry::new();

    for cfg in &configs {
        let (reserve0, reserve1) = fetch_reserves(&abi, provider.clone(), cfg.pool_key.address)
            .await
            .wrap_err_with(|| format!("bootstrap getReserves failed for {}", cfg.name))?;

        info!(
            pool = cfg.name,
            reserve0 = %reserve0,
            reserve1 = %reserve1,
            block = current_block,
            "bootstrapped reserves"
        );

        registry
            .update(
                cfg.pool_key,
                PoolState::V2 {
                    reserve0,
                    reserve1,
                    token0: cfg.expected_token0,
                    token1: cfg.expected_token1,
                    last_block: current_block,
                },
            )
            .await;
    }

    Ok(registry)
}

async fn fetch_reserves(
    abi: &Abi,
    provider: Arc<Provider<Ws>>,
    address: Address,
) -> Result<(U256, U256)> {
    let contract = Contract::new(address, abi.clone(), provider);
    let (r0, r1, _ts): (u128, u128, u32) = contract
        .method::<_, (u128, u128, u32)>("getReserves", ())?
        .call()
        .await
        .wrap_err("getReserves()")?;
    Ok((U256::from(r0), U256::from(r1)))
}

// ─── Sync event subscriptions ─────────────────────────────────────────────────

/// Subscribes to Sync events for all V2 pools on the given chain.
/// Returns on stream end or error. Caller wraps in a reconnect loop.
pub async fn run_subscriptions(
    provider: Arc<Provider<Ws>>,
    registry: PoolRegistry,
    chain: ChainId,
    arb_tx: tokio::sync::watch::Sender<()>,
) -> Result<()> {
    let configs = pool_catalog(chain);
    let pair_addresses: Vec<Address> = configs.iter().map(|c| c.pool_key.address).collect();
    let sync_topic = ethers::core::utils::keccak256("Sync(uint112,uint112)");

    let filter = Filter::new()
        .address(pair_addresses)
        .topic0(ethers::types::H256::from(sync_topic));

    let mut stream = provider
        .subscribe_logs(&filter)
        .await
        .wrap_err("subscribe_logs")?;

    info!(chain = chain.name(), "subscribed to Sync events");

    while let Some(log) = stream.next().await {
        handle_sync_log(log, &registry, chain).await;
        let _ = arb_tx.send(());
    }

    Err(eyre!("Sync log stream ended unexpectedly"))
}

async fn handle_sync_log(log: Log, registry: &PoolRegistry, chain: ChainId) {
    let address = log.address;
    let block_number = match log.block_number {
        Some(b) => b.as_u64(),
        None => {
            warn!("Sync log missing block_number, skipping");
            return;
        }
    };

    let entry: Option<PoolCatalogEntry> = pool_catalog(chain)
        .into_iter()
        .find(|c| c.pool_key.address == address);

    let Some(entry) = entry else {
        warn!(?address, "received Sync from unknown address");
        return;
    };

    if log.data.len() < 64 {
        warn!(pool = entry.name, "Sync log data too short");
        return;
    }
    let reserve0 = U256::from_big_endian(&log.data[0..32]);
    let reserve1 = U256::from_big_endian(&log.data[32..64]);

    registry
        .update(
            entry.pool_key,
            PoolState::V2 {
                reserve0,
                reserve1,
                token0: entry.expected_token0,
                token1: entry.expected_token1,
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
    chain: ChainId,
    current_block: u64,
) {
    let abi = parse_abi();
    let configs = pool_catalog(chain);

    for cfg in &configs {
        let stale = registry
            .get(cfg.pool_key)
            .await
            .map(|s| current_block.saturating_sub(s.last_block()) > STALE_BLOCK_THRESHOLD)
            .unwrap_or(true);

        if stale {
            warn!(
                pool = cfg.name,
                current_block,
                "pool is stale, refreshing via getReserves()"
            );
            match fetch_reserves(&abi, provider.clone(), cfg.pool_key.address).await {
                Ok((r0, r1)) => {
                    registry
                        .update(
                            cfg.pool_key,
                            PoolState::V2 {
                                reserve0: r0,
                                reserve1: r1,
                                token0: cfg.expected_token0,
                                token1: cfg.expected_token1,
                                last_block: current_block,
                            },
                        )
                        .await;
                }
                Err(e) => error!(pool = cfg.name, "refresh failed: {e}"),
            }
        }
    }
}
