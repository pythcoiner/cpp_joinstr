use joinstr::{
    miniscript::bitcoin::{self, address::NetworkUnchecked, Script, ScriptBuf},
    signer::WpkhHotSigner,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::mpsc};

use crate::{
    account::Notification,
    cpp_joinstr::{AddrAccount, AddressStatus},
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
        let store = Self {
            store: BTreeMap::new(),
            recv_generated_tip: recv_tip,
            change_generated_tip: change_tip,
            signer,
            notification,
            tx_poller: None,
            look_ahead,
        };
        store.update_watch_tip();

        store
    }

    fn notify(&self) {
        if let Err(e) = self.notification.send(Notification::AddressTipChanged) {
            log::error!("AddressStore::notify() fail to send notification: {e:?}");
        }
        self.update_watch_tip();
    }

    fn update_watch_tip(&self) {
        if let Some(tx_listener) = &self.tx_poller {
            let recv = self.recv_watch_tip();
            let change = self.change_watch_tip();
            // NOTE: tx_listener thread must send notification itself if
            // fail to connect to electrum
            let _ = tx_listener.send(AddressTip { recv, change });
        }
    }

    pub fn recv_coin_at(&mut self, spk: &ScriptBuf) {
        let AddressEntry { account, index, .. } = self.store.get(spk).expect("must be there");
        match *account {
            AddrAccount::Receive => self.update_recv(*index),
            AddrAccount::Change => self.update_change(*index),
            _ => unreachable!(),
        }
    }

    pub fn populate_maybe(&mut self) {
        for i in 0..self.recv_watch_tip() + 1 {
            let addr = self.signer.recv_addr_at(i);
            let script = addr.script_pubkey();
            self.store.entry(script).or_insert_with(|| {
                let address = addr.as_unchecked().clone();
                AddressEntry {
                    status: AddressStatus::NotUsed,
                    address,
                    account: AddrAccount::Receive,
                    index: i,
                }
            });
        }
        for i in 0..self.change_watch_tip() + 1 {
            let addr = self.signer.change_addr_at(i);
            let script = addr.script_pubkey();
            self.store.entry(script).or_insert_with(|| {
                let address = addr.as_unchecked().clone();
                AddressEntry {
                    status: AddressStatus::NotUsed,
                    address,
                    account: AddrAccount::Change,
                    index: i,
                }
            });
        }
    }

    pub fn update_recv(&mut self, index: u32) {
        if index > self.recv_generated_tip {
            self.recv_generated_tip = index;
        }
        self.populate_maybe();
        self.notify();
    }
    pub fn update_change(&mut self, index: u32) {
        if index > self.change_generated_tip {
            self.change_generated_tip = index;
        }
        self.populate_maybe();
        self.notify();
    }

    pub fn new_recv_addr(&mut self) -> bitcoin::Address {
        self.recv_generated_tip += 1;
        self.update_recv(self.recv_generated_tip);
        self.signer.recv_addr_at(self.recv_generated_tip)
    }

    pub fn new_change_addr(&mut self) -> bitcoin::Address {
        self.change_generated_tip += 1;
        self.update_change(self.change_generated_tip);
        self.signer.change_addr_at(self.change_generated_tip)
    }

    pub fn change_watch_tip(&self) -> u32 {
        self.change_generated_tip + self.look_ahead + 1
    }

    pub fn recv_watch_tip(&self) -> u32 {
        self.recv_generated_tip + self.look_ahead + 1
    }

    pub fn recv_tip(&self) -> u32 {
        self.recv_generated_tip
    }

    pub fn init(&mut self, tx_poller: mpsc::Sender<AddressTip>) {
        self.populate_maybe();
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
                if entry.status == AddressStatus::NotUsed && entry.account == AddrAccount::Receive {
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

    pub fn get(&self, account: AddrAccount) -> Addresses {
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
    pub status: AddressStatus,
    pub address: bitcoin::Address<NetworkUnchecked>,
    pub account: AddrAccount,
    pub index: u32,
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
    pub fn account(&self) -> AddrAccount {
        self.account
    }
    pub fn account_u32(&self) -> u32 {
        match self.account {
            AddrAccount::Receive => 0,
            AddrAccount::Change => 1,
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
