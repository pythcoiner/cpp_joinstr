use joinstr::{
    miniscript::bitcoin::{self, address::NetworkUnchecked, OutPoint, ScriptBuf, Txid},
    signer::{self, WpkhHotSigner},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::{address_store::AddressStore, cpp_joinstr::CoinStatus, tx_store::TxStore, Coins};

#[derive(Debug)]
pub struct CoinStore {
    // the coin store is always generated from the tx store
    // it's only a "coin cache" as the source of truth is
    // the tx store
    store: BTreeMap<OutPoint, CoinEntry>,
    signer: WpkhHotSigner,
    address_store: Arc<Mutex<AddressStore>>,
    tx_store: Arc<Mutex<TxStore>>,
    spk_history: BTreeMap<ScriptBuf, SpkHistory>,
    updates: Vec<Update>,
}

#[derive(Debug, Default)]
pub struct SpkHistory {
    history: Vec<Vec<(bitcoin::Txid, Option<u64>)>>,
}

#[derive(Debug)]
pub struct HistoryDiff {
    pub added: Vec<(bitcoin::Txid, Option<u64>)>,
    pub changed: Vec<(bitcoin::Txid, Option<u64>)>,
    pub removed: Vec<(bitcoin::Txid, Option<u64>)>,
}

impl SpkHistory {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, new: Vec<(bitcoin::Txid, Option<u64>)>) -> HistoryDiff {
        todo!()
    }
}

impl CoinStore {
    pub fn new(
        signer: WpkhHotSigner,
        address_store: Arc<Mutex<AddressStore>>,
        tx_store: Arc<Mutex<TxStore>>,
    ) -> Self {
        Self {
            store: BTreeMap::new(),
            signer,
            address_store,
            tx_store,
            updates: Vec::new(),
            spk_history: BTreeMap::new(),
        }
    }

    pub fn handle_history_response(
        &mut self,
        hist: BTreeMap<ScriptBuf, Vec<(bitcoin::Txid, Option<u64>)>>,
    ) {
        let mut updates = vec![];
        // generate diff & drop double spent txs
        for (spk, history) in hist {
            let update = self.update_spk_history(spk, history);
            updates.push(update);
        }

        {
            // pre fill with tx we already have
            let store = self.tx_store.lock().expect("poisoned");
            for upd in &mut updates {
                for tx in &mut upd.txs {
                    if let Some(store_tx) = store.inner_get(&tx.0) {
                        tx.1 = Some(store_tx);
                    }
                }
            }
        } // <- release tx lock

        // request missing txs
        let mut txids = vec![];
        for upd in &updates {
            txids.append(&mut upd.missing());
        }

        {
            // apply updates that are already completes
            let mut store = self.tx_store.lock().expect("poisoned");
            updates = updates
                .into_iter()
                .filter_map(|u| {
                    if u.is_complete() {
                        store.apply_updates(vec![u]);
                        None
                    } else {
                        Some(u)
                    }
                })
                .collect();
        } // <- release tx lock

        self.updates.append(&mut updates);
    }

    // triggered on history_get response
    pub fn update_spk_history(
        &mut self,
        spk: ScriptBuf,
        history: Vec<(Txid, Option<u64> /* height */)>,
    ) -> Update {
        // insert a blank history if no one
        if !self.spk_history.contains_key(&spk) {
            self.spk_history.insert(spk.clone(), SpkHistory::new());
        }

        // generate the diff w/ the last spk history
        let diff = self
            .spk_history
            .get_mut(&spk)
            .expect("already inserted")
            .insert(history);

        {
            // drop tx in the tx_store & update heights
            let mut store = self.tx_store.lock().expect("poisoned");
            for (txid, _) in &diff.removed {
                store.remove(txid);
            }
            for (txid, height) in &diff.changed {
                store.update_height(txid, *height);
            }
        } // <- release tx lock

        Update::from_diff(spk, diff)
    }

    pub fn handle_txs_response(&mut self, txs: Vec<bitcoin::Transaction>) {
        // iter over updates & fill where the transaction is requested
        for new_tx in txs {
            let new_txid = new_tx.compute_txid();
            for Update { txs, .. } in &mut self.updates {
                txs.iter_mut().for_each(|(txid, tx, _)| {
                    if (*txid == new_txid) && tx.is_none() {
                        *tx = Some(new_tx.clone());
                    }
                });
            }
        }
        {
            // push every complete update to the tx store
            let mut store = self.tx_store.lock().expect("poisoned");
            self.updates = self
                .updates
                .clone()
                .into_iter()
                .filter_map(|update| {
                    if update.is_complete() {
                        store.apply_updates(vec![update]);
                        None
                    } else {
                        Some(update)
                    }
                })
                .collect();
        } // <- release tx lock

        // re-generate coin store from tx store
        self.generate();
    }

    // generate from tx_store
    pub fn generate(&mut self) {
        // TODO:
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

#[derive(Debug, Clone)]
pub struct Update {
    spk: ScriptBuf,
    txs: Vec<(bitcoin::Txid, Option<bitcoin::Transaction>, Option<u64>)>,
}

impl Update {
    pub fn from_diff(spk: ScriptBuf, diff: HistoryDiff) -> Self {
        Update {
            spk,
            txs: diff
                .added
                .into_iter()
                .map(|(txid, height)| (txid, None, height))
                .collect(),
        }
    }

    pub fn is_complete(&self) -> bool {
        self.txs.iter().all(|(_, tx, _)| tx.is_some())
    }

    pub fn missing(&self) -> Vec<Txid> {
        self.txs
            .iter()
            .filter_map(|(txid, tx, _)| {
                if tx.is_none() {
                    Some(txid.clone())
                } else {
                    None
                }
            })
            .collect()
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
