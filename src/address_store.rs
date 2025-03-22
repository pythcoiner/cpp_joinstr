use joinstr::{
    miniscript::bitcoin::{self, address::NetworkUnchecked, OutPoint, ScriptBuf},
    signer::{self, WpkhHotSigner},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::{
    cpp_joinstr::{Account, AddressStatus},
    Addresses,
};

#[derive(Debug)]
pub struct AddressStore {
    store: BTreeMap<ScriptBuf, AddressEntry>,
    recv_received_tip: u32,
    change_received_tip: u32,
    recv_generated_tip: u32,
    change_generated_tip: u32,
    signer: WpkhHotSigner,
}

impl AddressStore {
    pub fn new(signer: WpkhHotSigner, recv_tip: u32, change_tip: u32) -> Self {
        let mut store = Self {
            store: BTreeMap::new(),
            recv_received_tip: 0,
            change_received_tip: 0,
            recv_generated_tip: recv_tip,
            change_generated_tip: change_tip,
            signer,
        };
        store.init();
        store
    }

    fn notify(&self) {
        // TODO: notify the consumer that indexes changed
        // TODO: notify electrum poller that indexes changed
    }

    fn update_recv(&mut self, index: u32) {
        if index > self.recv_generated_tip {
            self.recv_generated_tip = index;
        }
        if index > self.recv_received_tip {
            self.recv_received_tip = index;
        }
        self.notify();
    }
    fn update_change(&mut self, index: u32) {
        if index > self.change_generated_tip {
            self.change_generated_tip = index;
        }
        if index > self.change_received_tip {
            self.change_received_tip = index;
        }
        self.notify();
    }

    fn init(&mut self) {
        for i in 0..self.recv_generated_tip {
            let addr = self.signer.recv_addr_at(i);
            let script = addr.script_pubkey();
            let address = addr.as_unchecked().clone();
            let entry = AddressEntry {
                status: AddressStatus::NotUsed,
                address,
                account: Account::Receive,
                index: i,
                outpoints: BTreeSet::new(),
            };
            self.store.insert(script, entry);
        }
        for i in 0..self.change_generated_tip {
            let addr = self.signer.change_addr_at(i);
            let script = addr.script_pubkey();
            let address = addr.as_unchecked().clone();
            let entry = AddressEntry {
                status: AddressStatus::NotUsed,
                address,
                account: Account::Change,
                index: i,
                outpoints: BTreeSet::new(),
            };
            self.store.insert(script, entry);
        }
    }

    pub fn insert_coin(&mut self, coin: signer::Coin) {
        let path = coin.coin_path;
        let addr = self.signer.address_at(&path).expect("coins have index");
        let script = addr.clone().script_pubkey();
        let account = match path.depth {
            0 => Account::Receive,
            1 => Account::Change,
            _ => unreachable!(),
        };
        let index = path.index.expect("coin have index");
        let outpoint = coin.outpoint;

        match account {
            Account::Receive => self.update_recv(index),
            Account::Change => self.update_change(index),
            _ => unreachable!(),
        }

        self.store
            .entry(script)
            .and_modify(|e| {
                e.outpoints.insert(outpoint);
                match e.outpoints.len() {
                    0 => unreachable!(),
                    1 => e.status == AddressStatus::Used,
                    _ => e.status == AddressStatus::Reused,
                };
            })
            .or_insert({
                let mut outpoints = BTreeSet::new();
                outpoints.insert(outpoint);
                AddressEntry {
                    status: AddressStatus::Used,
                    address: addr.as_unchecked().clone(),
                    account,
                    index,
                    outpoints,
                }
            });
    }

    // Call by C++
    pub fn get_unused(&self) -> Addresses {
        let mut out = Addresses::new();
        let mut addrs = self
            .store
            .clone()
            .into_iter()
            .filter_map(|(_, entry)| {
                if entry.status == AddressStatus::NotUsed && entry.account == Account::Receive {
                    Some(Box::new(entry.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        addrs.sort_by(|a, b| {
            a.account
                .cmp(&b.account)
                .then_with(|| a.index.cmp(&b.index))
        });
        out.set(addrs);
        out
    }

    pub fn get(&self, account: Account) -> Addresses {
        let mut out = Addresses::new();
        let mut addrs = self
            .store
            .clone()
            .into_iter()
            .filter_map(|(_, entry)| {
                if entry.account == account {
                    Some(Box::new(entry.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        addrs.sort_by(|a, b| {
            a.account
                .cmp(&b.account)
                .then_with(|| a.index.cmp(&b.index))
        });
        out.set(addrs);
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
pub struct AddressEntry {
    status: AddressStatus,
    address: bitcoin::Address<NetworkUnchecked>,
    account: Account,
    index: u32,
    outpoints: BTreeSet<OutPoint>,
}

impl AddressEntry {
    pub fn script(&self) -> ScriptBuf {
        self.address.clone().assume_checked().script_pubkey()
    }
    pub fn status(&self) -> AddressStatus {
        self.status
    }
    pub fn value(&self) -> String {
        self.address.clone().assume_checked().to_string()
    }
    pub fn account(&self) -> Account {
        self.account
    }
    pub fn index(&self) -> u32 {
        self.index
    }
}
