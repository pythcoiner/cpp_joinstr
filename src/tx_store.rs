use joinstr::miniscript::bitcoin::{self, Txid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::coin_store::Update;

#[derive(Debug)]
pub struct TxStore {
    store: BTreeMap<Txid, TxEntry>,
}

impl TxStore {
    pub fn new() -> Self {
        Self {
            store: BTreeMap::new(),
        }
    }

    pub fn inner(&self) -> &BTreeMap<Txid, TxEntry> {
        &self.store
    }

    pub fn insert_updates(&mut self, updates: Vec<Update>) {
        // sanitize, all Txs must Some(_)
        updates.iter().for_each(|u| {
            assert!(u.is_complete());
        });

        for upd in updates {
            for (txid, tx, height) in upd.txs {
                let entry = TxEntry {
                    height,
                    tx: tx.expect("all txs populated"),
                    merkle: Default::default(),
                };
                self.store.insert(txid, entry);
            }
        }
    }

    pub fn update(&mut self, entry: TxEntry) {
        let txid = entry.txid();
        self.store.insert(txid, entry);
    }

    pub fn inner_get(&self, txid: &Txid) -> Option<bitcoin::Transaction> {
        self.store.get(txid).map(|e| e.tx.clone())
    }

    pub fn remove(&mut self, txid: &bitcoin::Txid) {
        self.store.remove(txid);
    }

    pub fn update_height(&mut self, txid: &bitcoin::Txid, height: Option<u64>) {
        self.store.get_mut(txid).expect("is present").height = height;
    }

    pub fn dump(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(&self.store)
    }

    pub fn restore(&mut self, value: serde_json::Value) -> Result<(), serde_json::Error> {
        self.store = serde_json::from_value(value)?;
        Ok(())
    }
}

impl Default for TxStore {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxEntry {
    height: Option<u64>,
    tx: bitcoin::Transaction,
    merkle: Vec<Vec<u8>>,
}

impl TxEntry {
    pub fn txid(&self) -> Txid {
        self.tx.compute_txid()
    }
    pub fn height(&self) -> Option<u64> {
        self.height
    }
    pub fn tx(&self) -> &bitcoin::Transaction {
        &self.tx
    }
    pub fn merkle(&self) -> Vec<Vec<u8>> {
        self.merkle.clone()
    }
}
