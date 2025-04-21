use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
};

use joinstr::miniscript::bitcoin::{self, address::NetworkUnchecked, OutPoint};
use serde::{Deserialize, Serialize};

use crate::Config;

#[derive(Debug, Clone, Serialize, Deserialize, PartialOrd, Ord, Eq, PartialEq)]
pub enum LabelKey {
    OutPoint(bitcoin::OutPoint),
    Transaction(bitcoin::Txid),
    Address(bitcoin::Address<NetworkUnchecked>),
}

#[derive(Debug, Clone, Default)]
pub struct LabelStore {
    store: BTreeMap<LabelKey, String>,
    config: Option<Config>,
}

impl LabelStore {
    pub fn new() -> Self {
        LabelStore {
            store: BTreeMap::new(),
            config: None,
        }
    }

    pub fn from_file(config: Config) -> Self {
        let file = File::open(config.labels_path());
        match file {
            Ok(mut file) => {
                let mut content = String::new();
                let _ = file.read_to_string(&mut content);
                let store: BTreeMap<LabelKey, String> =
                    serde_json::from_str(&content).unwrap_or_default();
                LabelStore {
                    store,
                    config: Some(config),
                }
            }
            Err(_) => LabelStore {
                store: Default::default(),
                config: Some(config),
            },
        }
    }

    pub fn persist(&self) {
        if let Some(config) = self.config.as_ref() {
            let file = File::create(config.labels_path());
            match file {
                Ok(mut file) => {
                    let content = serde_json::to_string_pretty(&self.store).expect("cannot fail");
                    let _ = file.write(content.as_bytes());
                }
                Err(e) => {
                    log::error!("LabelStore::persist() fail to open file: {e}");
                }
            }
        }
    }

    pub fn get(&self, key: &LabelKey) -> Option<String> {
        self.store.get(key).cloned()
    }

    pub fn edit(&mut self, key: LabelKey, value: Option<String>) {
        if let Some(value) = value {
            self.store
                .entry(key)
                .and_modify(|e| *e = value.clone())
                .or_insert(value);
        } else {
            self.store.remove(&key);
        }
    }

    pub fn remove(&mut self, key: LabelKey) {
        self.store.remove(&key);
    }

    pub fn address(&self, address: bitcoin::Address) -> Option<String> {
        self.get(&LabelKey::Address(address.as_unchecked().clone()))
    }

    pub fn outpoint(&self, outpoint: OutPoint) -> Option<String> {
        self.get(&LabelKey::OutPoint(outpoint))
    }

    pub fn transaction(&self, txid: bitcoin::Txid) -> Option<String> {
        self.get(&LabelKey::Transaction(txid))
    }
}
