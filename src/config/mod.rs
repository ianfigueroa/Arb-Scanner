pub mod ethereum;

pub use ethereum::PoolCatalogEntry;

use crate::types::{ArbPath, ChainId};

pub fn pool_catalog(chain: ChainId) -> Vec<PoolCatalogEntry> {
    match chain {
        ChainId::Ethereum => ethereum::ethereum_pools(),
        _ => vec![],
    }
}

pub fn arb_paths(chain: ChainId) -> Vec<ArbPath> {
    match chain {
        ChainId::Ethereum => ethereum::ethereum_arb_paths(),
        _ => vec![],
    }
}
