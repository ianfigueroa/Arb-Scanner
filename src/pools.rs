use std::collections::HashMap;
use std::sync::Arc;

use ethers::abi::Abi;
use ethers::contract::Contract;
use ethers::providers::{Middleware, Provider, StreamExt, Ws};
use ethers::types::{Address, Filter, Log, H256, U256};
use eyre::{eyre, Result, WrapErr};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::config::{arb_paths, pool_catalog, PoolCatalogEntry};
use crate::types::{ArbPath, ChainId, DexType, PoolKey, PoolState};

// ─── ABI fragments ────────────────────────────────────────────────────────────

const PAIR_ABI: &str = r#"[
    {"inputs":[],"name":"token0","outputs":[{"internalType":"address","name":"","type":"address"}],"stateMutability":"view","type":"function"},
    {"inputs":[],"name":"token1","outputs":[{"internalType":"address","name":"","type":"address"}],"stateMutability":"view","type":"function"},
    {"inputs":[],"name":"getReserves","outputs":[{"internalType":"uint112","name":"_reserve0","type":"uint112"},{"internalType":"uint112","name":"_reserve1","type":"uint112"},{"internalType":"uint32","name":"_blockTimestampLast","type":"uint32"}],"stateMutability":"view","type":"function"},
    {"anonymous":false,"inputs":[{"indexed":false,"internalType":"uint112","name":"reserve0","type":"uint112"},{"indexed":false,"internalType":"uint112","name":"reserve1","type":"uint112"}],"name":"Sync","type":"event"}
]"#;

fn parse_abi() -> Abi {
    serde_json::from_str(PAIR_ABI).expect("hardcoded ABI is valid JSON")
}

const SLOT0_ABI: &str = r#"[
    {"inputs":[],"name":"slot0","outputs":[{"internalType":"uint160","name":"sqrtPriceX96","type":"uint160"},{"internalType":"int24","name":"tick","type":"int24"},{"internalType":"uint16","name":"observationIndex","type":"uint16"},{"internalType":"uint16","name":"observationCardinality","type":"uint16"},{"internalType":"uint16","name":"observationCardinalityNext","type":"uint16"},{"internalType":"uint8","name":"feeProtocol","type":"uint8"},{"internalType":"bool","name":"unlocked","type":"bool"}],"stateMutability":"view","type":"function"}
]"#;

fn parse_slot0_abi() -> Abi {
    serde_json::from_str(SLOT0_ABI).expect("hardcoded slot0 ABI is valid JSON")
}

const CURVE_BALANCES_ABI: &str = r#"[
    {"inputs":[{"internalType":"uint256","name":"i","type":"uint256"}],"name":"balances","outputs":[{"internalType":"uint256","name":"","type":"uint256"}],"stateMutability":"view","type":"function"}
]"#;

fn parse_curve_balances_abi() -> Abi {
    serde_json::from_str(CURVE_BALANCES_ABI).expect("hardcoded Curve balances ABI is valid JSON")
}

const FACTORY_ABI: &str = r#"[{"inputs":[{"internalType":"address","name":"tokenA","type":"address"},{"internalType":"address","name":"tokenB","type":"address"}],"name":"getPair","outputs":[{"internalType":"address","name":"pair","type":"address"}],"stateMutability":"view","type":"function"}]"#;

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

// ─── Factory resolution ───────────────────────────────────────────────────────

/// Resolve pool addresses from the chain's factory contract and return the
/// runtime catalog + arb paths with correct on-chain addresses.
///
/// Ethereum and Arbitrum use their static configs (addresses are well-known and
/// verified at startup). Base and Polygon query `getPair()` from the canonical
/// factory contract to derive the correct pair address.
pub async fn resolve_pool_catalog(
    provider: &Arc<Provider<Ws>>,
    chain: ChainId,
) -> Result<(Vec<PoolCatalogEntry>, Vec<ArbPath>)> {
    match chain {
        ChainId::Ethereum | ChainId::Arbitrum => Ok((pool_catalog(chain), arb_paths(chain))),
        ChainId::Base => {
            use crate::config::base;
            let factory = base::factory();
            let weth_usdc = get_pair(provider, factory, base::weth(), base::usdc())
                .await
                .wrap_err("Base: getPair WETH/USDC")?;
            let weth_dai = get_pair(provider, factory, base::weth(), base::dai())
                .await
                .wrap_err("Base: getPair WETH/DAI")?;
            let dai_usdc = get_pair(provider, factory, base::dai(), base::usdc())
                .await
                .wrap_err("Base: getPair DAI/USDC")?;
            info!(
                chain = "base",
                weth_usdc = %weth_usdc,
                weth_dai  = %weth_dai,
                dai_usdc  = %dai_usdc,
                "resolved pool addresses from factory"
            );
            Ok((
                base::base_pools_from_pairs(weth_usdc, weth_dai, dai_usdc),
                base::base_arb_paths_from_pairs(weth_usdc, weth_dai, dai_usdc),
            ))
        }
        ChainId::Polygon => {
            use crate::config::polygon;
            let factory = polygon::factory();
            let weth_usdc = get_pair(provider, factory, polygon::weth(), polygon::usdc())
                .await
                .wrap_err("Polygon: getPair WETH/USDC")?;
            let usdc_dai = get_pair(provider, factory, polygon::usdc(), polygon::dai())
                .await
                .wrap_err("Polygon: getPair USDC/DAI")?;
            let weth_dai = get_pair(provider, factory, polygon::weth(), polygon::dai())
                .await
                .wrap_err("Polygon: getPair WETH/DAI")?;
            info!(
                chain = "polygon",
                weth_usdc = %weth_usdc,
                usdc_dai  = %usdc_dai,
                weth_dai  = %weth_dai,
                "resolved pool addresses from factory"
            );
            Ok((
                polygon::polygon_pools_from_pairs(weth_usdc, usdc_dai, weth_dai),
                polygon::polygon_arb_paths_from_pairs(weth_usdc, usdc_dai, weth_dai),
            ))
        }
    }
}

async fn get_pair(
    provider: &Arc<Provider<Ws>>,
    factory: Address,
    token_a: Address,
    token_b: Address,
) -> Result<Address> {
    let abi: Abi = serde_json::from_str(FACTORY_ABI).expect("factory ABI is valid");
    let contract = Contract::new(factory, abi, provider.clone());
    let pair: Address = contract
        .method::<_, Address>("getPair", (token_a, token_b))?
        .call()
        .await
        .wrap_err_with(|| format!("getPair({token_a:?}, {token_b:?}) failed"))?;
    if pair == Address::zero() {
        return Err(eyre!(
            "getPair({:?}, {:?}) returned zero address — pair does not exist",
            token_a,
            token_b
        ));
    }
    Ok(pair)
}

// ─── Startup: verify token ordering on-chain ─────────────────────────────────

pub async fn verify_pool_tokens(
    provider: Arc<Provider<Ws>>,
    catalog: &[PoolCatalogEntry],
) -> Result<()> {
    let abi = parse_abi();

    for cfg in catalog {
        if cfg.dex_type == DexType::CurveStableswap {
            // Curve pools use coins(i) not token0/token1; skip on-chain verification
            info!(
                pool = cfg.name,
                "skipping token verification for Curve pool"
            );
            continue;
        }

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
    catalog: &[PoolCatalogEntry],
) -> Result<PoolRegistry> {
    let abi = parse_abi();

    let current_block = provider
        .get_block_number()
        .await
        .wrap_err("get_block_number")?
        .as_u64();

    let registry = PoolRegistry::new();

    for cfg in catalog {
        match cfg.dex_type {
            DexType::UniswapV2 => {
                let (reserve0, reserve1) =
                    fetch_reserves(&abi, provider.clone(), cfg.pool_key.address)
                        .await
                        .wrap_err_with(|| {
                            format!("bootstrap getReserves failed for {}", cfg.name)
                        })?;
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
            DexType::UniswapV3 => {
                let sqrt_price_x96 = fetch_slot0(provider.clone(), cfg.pool_key.address)
                    .await
                    .wrap_err_with(|| format!("bootstrap slot0 failed for {}", cfg.name))?;
                info!(
                    pool = cfg.name,
                    sqrt_price_x96 = %sqrt_price_x96,
                    block = current_block,
                    "bootstrapped slot0"
                );
                registry
                    .update(
                        cfg.pool_key,
                        PoolState::V3 {
                            sqrt_price_x96,
                            fee_tier: cfg.fee_tier,
                            token0: cfg.expected_token0,
                            token1: cfg.expected_token1,
                            last_block: current_block,
                        },
                    )
                    .await;
            }
            DexType::CurveStableswap => {
                let n = usize::from(cfg.n_coins);
                let mut balances = Vec::with_capacity(n);
                for i in 0..n {
                    let b = fetch_curve_balance(provider.clone(), cfg.pool_key.address, i as u64)
                        .await
                        .wrap_err_with(|| {
                            format!("bootstrap balances({i}) failed for {}", cfg.name)
                        })?;
                    balances.push(b);
                }
                info!(
                    pool = cfg.name,
                    block = current_block,
                    "bootstrapped curve balances"
                );
                registry
                    .update(
                        cfg.pool_key,
                        PoolState::Curve {
                            balances,
                            last_block: current_block,
                        },
                    )
                    .await;
            }
        }
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

async fn fetch_slot0(provider: Arc<Provider<Ws>>, address: Address) -> Result<U256> {
    let abi = parse_slot0_abi();
    let contract = Contract::new(address, abi, provider);
    let (sqrt_price_x96, _, _, _, _, _, _): (U256, i32, u16, u16, u16, u8, bool) = contract
        .method::<_, (U256, i32, u16, u16, u16, u8, bool)>("slot0", ())?
        .call()
        .await
        .wrap_err("slot0()")?;
    Ok(sqrt_price_x96)
}

async fn fetch_curve_balance(
    provider: Arc<Provider<Ws>>,
    address: Address,
    i: u64,
) -> Result<U256> {
    let abi = parse_curve_balances_abi();
    let contract = Contract::new(address, abi, provider);
    contract
        .method::<_, U256>("balances", U256::from(i))?
        .call()
        .await
        .wrap_err("balances()")
}

// ─── Sync event subscriptions ─────────────────────────────────────────────────

/// Subscribes to Sync/Swap/TokenExchange events for all pools in the catalog.
/// Returns on stream end or error. Caller wraps in a reconnect loop.
pub async fn run_subscriptions(
    provider: Arc<Provider<Ws>>,
    registry: PoolRegistry,
    catalog: Arc<Vec<PoolCatalogEntry>>,
    chain: ChainId,
    arb_tx: tokio::sync::watch::Sender<()>,
) -> Result<()> {
    let pair_addresses: Vec<Address> = catalog.iter().map(|c| c.pool_key.address).collect();
    let sync_topic = H256::from(ethers::core::utils::keccak256("Sync(uint112,uint112)"));
    let swap_topic = H256::from(ethers::core::utils::keccak256(
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
    ));
    let token_exchange_topic = H256::from(ethers::core::utils::keccak256(
        "TokenExchange(address,int128,uint256,int128,uint256)",
    ));

    let filter = Filter::new().address(pair_addresses);

    let mut stream = provider
        .subscribe_logs(&filter)
        .await
        .wrap_err("subscribe_logs")?;

    info!(chain = chain.name(), "subscribed to pool events");

    while let Some(log) = stream.next().await {
        let handled = match log.topics.first().copied() {
            Some(t) if t == sync_topic => {
                handle_sync_log(log, &registry, &catalog).await;
                true
            }
            Some(t) if t == swap_topic => {
                handle_swap_log(log, &registry, &catalog).await;
                true
            }
            Some(t) if t == token_exchange_topic => {
                handle_token_exchange_log(log, &registry, &catalog).await;
                true
            }
            _ => false,
        };
        if handled {
            let _ = arb_tx.send(());
        }
    }

    Err(eyre!("pool log stream ended unexpectedly"))
}

async fn handle_sync_log(log: Log, registry: &PoolRegistry, catalog: &[PoolCatalogEntry]) {
    let address = log.address;
    let block_number = match log.block_number {
        Some(b) => b.as_u64(),
        None => {
            warn!("Sync log missing block_number, skipping");
            return;
        }
    };

    let entry = catalog.iter().find(|c| c.pool_key.address == address);
    let Some(entry) = entry else {
        debug!(?address, "received Sync from unknown address");
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

async fn handle_swap_log(log: Log, registry: &PoolRegistry, catalog: &[PoolCatalogEntry]) {
    let address = log.address;
    let block_number = match log.block_number {
        Some(b) => b.as_u64(),
        None => {
            warn!("Swap log missing block_number, skipping");
            return;
        }
    };

    let entry = catalog.iter().find(|c| c.pool_key.address == address);
    let Some(entry) = entry else {
        debug!(?address, "received Swap from unknown address");
        return;
    };

    // Swap data: amount0(32), amount1(32), sqrtPriceX96(32), liquidity(32), tick(32)
    if log.data.len() < 96 {
        warn!(pool = entry.name, "Swap log data too short");
        return;
    }
    let sqrt_price_x96 = U256::from_big_endian(&log.data[64..96]);

    registry
        .update(
            entry.pool_key,
            PoolState::V3 {
                sqrt_price_x96,
                fee_tier: entry.fee_tier,
                token0: entry.expected_token0,
                token1: entry.expected_token1,
                last_block: block_number,
            },
        )
        .await;
}

async fn handle_token_exchange_log(
    log: Log,
    registry: &PoolRegistry,
    catalog: &[PoolCatalogEntry],
) {
    let address = log.address;
    let block_number = match log.block_number {
        Some(b) => b.as_u64(),
        None => {
            warn!("TokenExchange log missing block_number, skipping");
            return;
        }
    };

    let entry = catalog.iter().find(|c| c.pool_key.address == address);
    let Some(entry) = entry else {
        debug!(?address, "received TokenExchange from unknown address");
        return;
    };

    // data: sold_id(32), tokens_sold(32), bought_id(32), tokens_bought(32)
    if log.data.len() < 128 {
        warn!(pool = entry.name, "TokenExchange log data too short");
        return;
    }
    let sold_id = log.data[31] as usize;
    let tokens_sold = U256::from_big_endian(&log.data[32..64]);
    let bought_id = log.data[95] as usize;
    let tokens_bought = U256::from_big_endian(&log.data[96..128]);

    let current_state = registry.get(entry.pool_key).await;
    if let Some(PoolState::Curve { balances, .. }) = current_state {
        let n = balances.len();
        if sold_id >= n || bought_id >= n {
            warn!(
                pool = entry.name,
                sold_id, bought_id, n, "coin index out of range"
            );
            return;
        }
        let mut new_balances = balances.clone();
        new_balances[sold_id] = new_balances[sold_id].saturating_add(tokens_sold);
        new_balances[bought_id] = new_balances[bought_id].saturating_sub(tokens_bought);
        registry
            .update(
                entry.pool_key,
                PoolState::Curve {
                    balances: new_balances,
                    last_block: block_number,
                },
            )
            .await;
    }
}

// ─── Stale reserve refresh ────────────────────────────────────────────────────

const STALE_BLOCK_THRESHOLD: u64 = 50;

/// For any pool whose last_block is more than STALE_BLOCK_THRESHOLD behind
/// current_block, refresh via getReserves().
pub async fn refresh_stale_pools(
    provider: Arc<Provider<Ws>>,
    registry: &PoolRegistry,
    catalog: &[PoolCatalogEntry],
    current_block: u64,
) {
    let abi = parse_abi();

    for cfg in catalog {
        let stale = registry
            .get(cfg.pool_key)
            .await
            .map(|s| current_block.saturating_sub(s.last_block()) > STALE_BLOCK_THRESHOLD)
            .unwrap_or(true);

        if stale {
            warn!(pool = cfg.name, current_block, "pool is stale, refreshing");
            match cfg.dex_type {
                DexType::UniswapV2 => {
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
                DexType::UniswapV3 => {
                    match fetch_slot0(provider.clone(), cfg.pool_key.address).await {
                        Ok(sqrt_price_x96) => {
                            registry
                                .update(
                                    cfg.pool_key,
                                    PoolState::V3 {
                                        sqrt_price_x96,
                                        fee_tier: cfg.fee_tier,
                                        token0: cfg.expected_token0,
                                        token1: cfg.expected_token1,
                                        last_block: current_block,
                                    },
                                )
                                .await;
                        }
                        Err(e) => error!(pool = cfg.name, "slot0 refresh failed: {e}"),
                    }
                }
                DexType::CurveStableswap => {
                    let n = usize::from(cfg.n_coins);
                    let mut balances = Vec::with_capacity(n);
                    let mut ok = true;
                    for i in 0..n {
                        match fetch_curve_balance(provider.clone(), cfg.pool_key.address, i as u64)
                            .await
                        {
                            Ok(b) => balances.push(b),
                            Err(e) => {
                                error!(pool = cfg.name, "balances({i}) refresh failed: {e}");
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        registry
                            .update(
                                cfg.pool_key,
                                PoolState::Curve {
                                    balances,
                                    last_block: current_block,
                                },
                            )
                            .await;
                    }
                }
            }
        }
    }
}
