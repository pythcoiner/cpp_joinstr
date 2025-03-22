use joinstr::miniscript::bitcoin::{self, Txid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

    pub fn insert(&mut self, entry: TxEntry) {
        let txid = entry.txid();
        self.store.insert(txid, entry);
    }

    pub fn inner_get(&self, txid: &Txid) -> Option<bitcoin::Transaction> {
        self.store.get(txid).map(|e| e.tx.clone())
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
    height: u64,
    tx: bitcoin::Transaction,
    merkle: Vec<Vec<u8>>,
}

impl TxEntry {
    pub fn txid(&self) -> Txid {
        self.tx.compute_txid()
    }
    pub fn height(&self) -> u64 {
        self.height
    }
    pub fn tx(&self) -> &bitcoin::Transaction {
        &self.tx
    }
    pub fn merkle(&self) -> Vec<Vec<u8>> {
        self.merkle.clone()
    }
}
