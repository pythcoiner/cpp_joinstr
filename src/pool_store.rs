use crate::{cpp_joinstr::PoolStatus, Pool, Pools};
use joinstr::nostr::{self};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A structure to manage a collection of pools.
#[derive(Debug, Default)]
pub struct PoolStore {
    store: BTreeMap<String, PoolEntry>,
}

impl PoolStore {
    /// Creates a new instance of `PoolStore`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Updates the status of a pool in the store.
    ///
    /// Returns `true` if the status was changed, `false` otherwise.
    pub fn update(&mut self, pool: nostr::Pool, status: PoolStatus) -> bool /* updated */ {
        let mut updated = false;
        self.store
            .entry(pool.id.clone())
            .and_modify(|e| {
                if e.status != status {
                    e.status = status;
                    updated = true;
                }
            })
            .or_insert(PoolEntry { pool, status });
        updated
    }

    /// Retrieves all pools with the specified status.
    ///
    /// # Arguments
    /// * `status` - The status to filter pools by.
    pub fn get_by_status(&self, status: PoolStatus) -> Pools {
        let mut out = Pools::new();
        let pools = self
            .store
            .clone()
            .into_iter()
            .filter_map(|(_, pool)| {
                if pool.status == status {
                    Some(pool.pool())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        out.set(pools);
        out
    }

    /// Retrieves all available pools.
    pub fn available_pools(&self) -> Pools {
        let mut out = Pools::new();
        let pools = self
            .store
            .clone()
            .into_iter()
            .filter_map(|(_, entry)| match entry.status {
                PoolStatus::Available => Some(entry.pool()),
                PoolStatus::Closed | PoolStatus::Processing => None,
                _ => unreachable!(),
            })
            .collect();
        out.set(pools);
        out
    }
}

/// Represents a single pool entry with its status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntry {
    status: PoolStatus,
    pool: nostr::Pool,
}

impl PoolEntry {
    /// Returns the ID of the pool.
    pub fn pool_id(&self) -> String {
        self.pool.id.clone()
    }
    /// Returns the status of the pool.
    pub fn status(&self) -> PoolStatus {
        self.status
    }
    /// Returns a boxed clone of the pool.
    pub fn pool(&self) -> Box<Pool> {
        Box::new(self.pool.clone().into())
    }
}
