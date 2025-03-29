use joinstr::{
    miniscript::bitcoin::{self, address::NetworkUnchecked, Script, ScriptBuf},
    signer::WpkhHotSigner,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::mpsc};

use crate::{
    cpp_joinstr::{Account, AddressStatus},
    wallet::Notification,
    Addresses,
};

#[derive(Debug, Clone, Copy)]
pub struct AddressTip {
    pub recv: u32,
    pub change: u32,
}

#[derive(Debug)]
pub struct AddressStore {
    store: BTreeMap<ScriptBuf, AddressEntry>,
    recv_received_tip: u32,
    change_received_tip: u32,
    recv_generated_tip: u32,
    change_generated_tip: u32,
    signer: WpkhHotSigner,
    notification: mpsc::Sender<Notification>,
    tx_poller: Option<mpsc::Sender<AddressTip>>,
    look_ahead: u32,
}

impl AddressStore {
    pub fn new(
        signer: WpkhHotSigner,
        notification: mpsc::Sender<Notification>,
        recv_tip: u32,
        change_tip: u32,
        look_ahead: u32,
    ) -> Self {
        Self {
            store: BTreeMap::new(),
            recv_received_tip: 0,
            change_received_tip: 0,
            recv_generated_tip: recv_tip,
            change_generated_tip: change_tip,
            signer,
            notification,
            tx_poller: None,
            look_ahead,
        }
    }

    fn notify(&self) {
        self.notification
            .send(Notification::AddressTipChanged)
            .unwrap();
        if let Some(tx_poller) = &self.tx_poller {
            let recv = self.recv_generated_tip + self.look_ahead;
            let change = self.change_generated_tip + self.look_ahead;
            tx_poller.send(AddressTip { recv, change }).unwrap();
        }
    }

    pub fn update_recv(&mut self, index: u32) {
        if index > self.recv_generated_tip {
            self.recv_generated_tip = index;
        }
        if index > self.recv_received_tip {
            self.recv_received_tip = index;
        }
        self.notify();
    }
    pub fn update_change(&mut self, index: u32) {
        if index > self.change_generated_tip {
            self.change_generated_tip = index;
        }
        if index > self.change_received_tip {
            self.change_received_tip = index;
        }
        self.notify();
    }

    pub fn init(&mut self, tx_poller: mpsc::Sender<AddressTip>) {
        for i in 0..self.recv_generated_tip {
            let addr = self.signer.recv_addr_at(i);
            let script = addr.script_pubkey();
            let address = addr.as_unchecked().clone();
            let entry = AddressEntry {
                status: AddressStatus::NotUsed,
                address,
                account: Account::Receive,
                index: i,
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
            };
            self.store.insert(script, entry);
        }
        self.tx_poller = Some(tx_poller);
        self.notify();
    }

    pub fn get_entry(&self, spk: &Script) -> Option<AddressEntry> {
        self.store.get(spk).cloned()
    }

    pub fn get_entry_mut(&mut self, spk: &Script) -> Option<&mut AddressEntry> {
        self.store.get_mut(spk)
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
    // outpoints: BTreeSet<OutPoint>,
}

impl AddressEntry {
    pub fn script(&self) -> ScriptBuf {
        self.address.clone().assume_checked().script_pubkey()
    }
    pub fn set_status(&mut self, status: AddressStatus) {
        self.status = status;
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
    pub fn account_u32(&self) -> u32 {
        match self.account {
            Account::Receive => 0,
            Account::Change => 1,
            _ => unreachable!(),
        }
    }
    pub fn index(&self) -> u32 {
        self.index
    }
    pub fn address(&self) -> bitcoin::Address<NetworkUnchecked> {
        self.address.clone()
    }
}
