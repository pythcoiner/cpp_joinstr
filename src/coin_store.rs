use joinstr::{
    miniscript::bitcoin::{self, address::NetworkUnchecked, OutPoint},
    signer::{self, WpkhHotSigner},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use crate::{address_store::AddressStore, cpp_joinstr::CoinStatus, Coins};

#[derive(Debug)]
pub struct CoinStore {
    store: BTreeMap<OutPoint, CoinEntry>,
    signer: WpkhHotSigner,
    address_store: Arc<Mutex<AddressStore>>,
}

impl CoinStore {
    pub fn new(signer: WpkhHotSigner, store: Arc<Mutex<AddressStore>>) -> Self {
        Self {
            store: BTreeMap::new(),
            signer,
            address_store: store,
        }
    }

    pub fn update(&mut self, coin: signer::Coin, status: CoinStatus) -> bool /* updated */ {
        let address = self
            .signer
            .address_at(&coin.coin_path)
            .expect("coin have index")
            .as_unchecked()
            .clone();
        let mut updated = false;
        self.store
            .entry(coin.outpoint)
            .and_modify(|e| {
                if e.status != status {
                    e.status = status;
                    updated = true;
                }
            })
            .or_insert(CoinEntry {
                coin: coin.clone(),
                status,
                address,
            });
        // update the address store
        // NOTE: we do not take a bare .lock() to avoid a deadlock
        //   as there is already a lock on self.
        loop {
            if let Ok(mut store) = self.address_store.try_lock() {
                store.insert_coin(coin.clone());
                break;
            } else {
                // FIXME: 11ms is likely way too much
                thread::sleep(Duration::from_millis(11));
            }
        }
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
                    Some(Box::new(coin))
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
                CoinStatus::Unconfirmed | CoinStatus::Confirmed => Some(Box::new(coin)),
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
    address: bitcoin::Address<NetworkUnchecked>,
}

impl CoinEntry {
    pub fn status(&self) -> CoinStatus {
        self.status
    }
    pub fn status_str(&self) -> String {
        format!("{:?}", self.status)
    }
    pub fn amount_sat(&self) -> u64 {
        self.coin.txout.value.to_sat()
    }
    pub fn amount_btc(&self) -> f64 {
        self.coin.txout.value.to_btc()
    }
    pub fn outpoint(&self) -> &OutPoint {
        &self.coin.outpoint
    }
    pub fn outpoint_str(&self) -> String {
        self.outpoint().to_string()
    }
    pub fn boxed(&self) -> Box<CoinEntry> {
        Box::new(self.clone())
    }
    pub fn address(&self) -> String {
        self.address.clone().assume_checked().to_string()
    }
}
