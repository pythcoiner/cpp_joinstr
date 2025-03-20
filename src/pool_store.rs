use crate::{cpp_joinstr::PoolStatus, Pool, Pools};
use joinstr::nostr::{self};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct PoolStore {
    store: BTreeMap<String, PoolEntry>,
}

impl PoolStore {
    pub fn new() -> Self {
        Self::default()
    }

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

    // Call by C++
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

    // Call by C++
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

    pub fn dump(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(&self.store)
    }

    pub fn restore(&mut self, value: serde_json::Value) -> Result<(), serde_json::Error> {
        self.store = serde_json::from_value(value)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntry {
    status: PoolStatus,
    pool: nostr::Pool,
}

impl PoolEntry {
    pub fn pool_id(&self) -> String {
        self.pool.id.clone()
    }
    pub fn status(&self) -> PoolStatus {
        self.status
    }
    pub fn pool(&self) -> Box<Pool> {
        Box::new(self.pool.clone().into())
    }
}
