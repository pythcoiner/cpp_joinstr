use crate::cpp_joinstr::{PoolStatus, RustPool};
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
    pub fn get_by_status(&self, status: PoolStatus) -> Vec<RustPool> {
        self.store
            .clone()
            .into_iter()
            .filter_map(|(_, pool)| {
                if pool.status == status {
                    Some(pool.pool().into())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    /// Retrieves all available pools.
    pub fn available_pools(&self) -> Vec<RustPool> {
        self.store
            .clone()
            .into_iter()
            .filter_map(|(_, entry)| match entry.status {
                PoolStatus::Available | PoolStatus::Processing => Some(entry.pool().into()),
                PoolStatus::Closed => None,
                _ => unreachable!(),
            })
            .collect()
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
    /// Returns the a clone of the pool
    pub fn pool(&self) -> nostr::Pool {
        self.pool.clone()
    }
}

impl From<nostr::Pool> for RustPool {
    fn from(value: nostr::Pool) -> Self {
        let payload = value.payload.expect("have a payload");
        RustPool {
            denomination: payload.denomination.to_sat(),
            total_peers: payload.peers,
            current_peers: 0,
            relay: payload
                .relays
                .first()
                .expect("always have a relay")
                .to_string(),
            fees: match payload.fee {
                nostr::Fee::Fixed(f) => f,
                nostr::Fee::Provider(_) => unreachable!(),
            },
            id: value.id,
            timeout: match payload.timeout {
                nostr::Timeline::Simple(t) => t,
                _ => unreachable!(),
            },
        }
    }
}
