use joinstr::{miniscript::bitcoin::OutPoint, signer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{cpp_joinstr::CoinStatus, Coin, Coins};

#[derive(Debug, Default)]
pub struct CoinStore {
    store: BTreeMap<OutPoint, CoinEntry>,
}

impl CoinStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, coin: signer::Coin, status: CoinStatus) -> bool /* updated */ {
        let mut updated = false;
        self.store
            .entry(coin.outpoint)
            .and_modify(|e| {
                if e.status != status {
                    e.status = status;
                    updated = true;
                }
            })
            .or_insert(CoinEntry { coin, status });
        updated
    }

    // Call by C++
    pub fn get_by_status(&self, status: CoinStatus) -> Coins {
        let mut out = Coins::new();
        let coins = self
            .store
            .clone()
            .into_iter()
            .filter_map(|(_, coin)| {
                if coin.status == status {
                    Some(coin.coin())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        out.set(coins);
        out
    }

    // Call by C++
    pub fn spendable_coins(&self) -> Coins {
        let mut out = Coins::new();
        let coins = self
            .store
            .clone()
            .into_iter()
            .filter_map(|(_, coin)| match coin.status {
                CoinStatus::Unconfirmed | CoinStatus::Confirmed => Some(coin.coin()),
                CoinStatus::BeingSpend | CoinStatus::Spend => None,
                _ => unreachable!(),
            })
            .collect();
        out.set(coins);
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
pub struct CoinEntry {
    status: CoinStatus,
    coin: signer::Coin,
}

impl CoinEntry {
    pub fn outpoint(&self) -> &OutPoint {
        &self.coin.outpoint
    }
    pub fn status(&self) -> CoinStatus {
        self.status
    }
    pub fn coin(&self) -> Box<Coin> {
        Box::new(self.coin.clone().into())
    }
}
