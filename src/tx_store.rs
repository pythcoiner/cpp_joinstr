use joinstr::miniscript::bitcoin::{self, Txid};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

use crate::coin_store::Update;

#[derive(Debug)]
pub struct TxStore {
    store: BTreeMap<Txid, TxEntry>,
    path: Option<PathBuf>,
}

impl TxStore {
    pub fn new(store: BTreeMap<Txid, TxEntry>, path: Option<PathBuf>) -> Self {
        Self { store, path }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.store.len()
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

    pub fn store_from_file(path: PathBuf) -> BTreeMap<Txid, TxEntry> {
        let file = File::open(path);
        if let Ok(mut file) = file {
            let mut content = String::new();
            let _ = file.read_to_string(&mut content);
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Default::default()
        }
    }

    pub fn persist(&self) {
        if let Some(path) = &self.path {
            let mut file = File::create(path.clone()).unwrap();
            let content = serde_json::to_string_pretty(&self.store).unwrap();
            let _ = file.write(content.as_bytes());
        }
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
