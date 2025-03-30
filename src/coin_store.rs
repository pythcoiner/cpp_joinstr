use joinstr::{
    bip39,
    miniscript::bitcoin::{self, address::NetworkUnchecked, OutPoint, ScriptBuf, Txid},
    signer::{self, WpkhHotSigner},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::mpsc,
};

use crate::{
    account::Notification,
    address_store::{AddressStore, AddressTip},
    cpp_joinstr::{AddressStatus, CoinStatus},
    tx_store::TxStore,
    Coins,
};

#[derive(Debug)]
pub struct CoinStore {
    // the coin store is always generated from the tx store
    // it's only a "coin cache" as the source of truth is
    // the tx store
    store: BTreeMap<OutPoint, CoinEntry>,
    spk_to_outpoint: BTreeMap<ScriptBuf, HashSet<OutPoint>>,
    address_store: AddressStore,
    tx_store: TxStore,
    spk_history: BTreeMap<ScriptBuf, SpkHistory>,
    updates: Vec<Update>,
    signer: WpkhHotSigner,
    notification: mpsc::Sender<Notification>,
}

#[derive(Debug, Default)]
pub struct SpkHistory {
    history: Vec<BTreeMap<bitcoin::Txid, Option<u64>>>,
}

#[derive(Debug, Default)]
pub struct HistoryDiff {
    pub added: BTreeMap<bitcoin::Txid, Option<u64>>,
    pub changed: BTreeMap<bitcoin::Txid, Option<u64>>,
    pub removed: BTreeMap<bitcoin::Txid, Option<u64>>,
}

impl SpkHistory {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn insert(&mut self, new: Vec<(bitcoin::Txid, Option<u64>)>) -> HistoryDiff {
        let new: BTreeMap<_, _> = new.into_iter().collect();
        let diff = if self.history.is_empty() {
            HistoryDiff {
                added: new.clone(),
                ..Default::default()
            }
        } else {
            let mut diff = HistoryDiff::default();
            let previous = self.history.last().expect("at least one element");

            new.iter().for_each(|(txid, height)| {
                if !previous.contains_key(txid) {
                    diff.added.insert(*txid, *height);
                } else {
                    let prev_height = previous.get(txid).expect("present");
                    if height != prev_height {
                        diff.changed.insert(*txid, *height);
                    }
                }
            });

            previous.iter().for_each(|(txid, height)| {
                if !new.contains_key(txid) {
                    diff.removed.insert(*txid, *height);
                }
            });
            diff
        };
        diff
    }
}

impl CoinStore {
    pub fn new(
        network: bitcoin::Network,
        mnemonic: bip39::Mnemonic,
        notification: mpsc::Sender<Notification>,
        recv_tip: u32,
        change_tip: u32,
        look_ahead: u32,
        // TODO: pass tx_store state
    ) -> Self {
        let signer = WpkhHotSigner::new_from_mnemonics(network, &mnemonic.to_string())
            .expect("valid mnemonic");
        let address_store = AddressStore::new(
            signer.clone(),
            notification.clone(),
            recv_tip,
            change_tip,
            look_ahead,
        );
        Self {
            store: BTreeMap::new(),
            spk_to_outpoint: BTreeMap::new(),
            address_store,
            tx_store: Default::default(),
            updates: Vec::new(),
            spk_history: BTreeMap::new(),
            signer,
            notification,
        }
    }

    pub fn init(&mut self, tx_poller: mpsc::Sender<AddressTip>) {
        self.address_store.init(tx_poller);
    }
    pub fn signer(&self) -> WpkhHotSigner {
        self.signer.clone()
    }
    pub fn signer_ref(&self) -> &WpkhHotSigner {
        &self.signer
    }
    pub fn recv_watch_tip(&self) -> u32 {
        self.address_store.recv_watch_tip()
    }

    pub fn change_watch_tip(&self) -> u32 {
        self.address_store.change_watch_tip()
    }

    pub fn new_recv_addr(&mut self) -> bitcoin::Address {
        self.address_store.new_recv_addr()
    }

    pub fn recv_tip(&self) -> u32 {
        self.address_store.recv_tip()
    }

    pub fn new_change_addr(&mut self) -> bitcoin::Address {
        self.address_store.new_change_addr()
    }

    pub fn recv_coin_at(&mut self, spk: &ScriptBuf) {
        self.address_store.recv_coin_at(spk);
    }

    pub fn handle_history_response(
        &mut self,
        hist: BTreeMap<ScriptBuf, Vec<(bitcoin::Txid, Option<u64>)>>,
    ) -> Vec<Txid> {
        let mut updates = vec![];
        // generate diff & drop double spent txs
        for (spk, history) in hist {
            self.recv_coin_at(&spk);
            let update = self.update_spk_history(spk, history);
            updates.push(update);
        }

        {
            // pre fill with tx we already have
            let store = &self.tx_store;
            for upd in &mut updates {
                for tx in &mut upd.txs {
                    if let Some(store_tx) = store.inner_get(&tx.0) {
                        tx.1 = Some(store_tx);
                    }
                }
            }
        } // <- release tx_store ref

        // request missing txs
        let mut txids = vec![];
        for upd in &updates {
            txids.append(&mut upd.missing());
        }

        {
            // apply updates that are already completes
            let store = &mut self.tx_store;
            updates = updates
                .into_iter()
                .filter_map(|u| {
                    if u.is_complete() {
                        store.insert_updates(vec![u]);
                        None
                    } else {
                        Some(u)
                    }
                })
                .collect();
        } // <- release &mut tx_store

        self.updates.append(&mut updates);
        txids
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
            let store = &mut self.tx_store;
            for txid in diff.removed.keys() {
                store.remove(txid);
            }
            for (txid, height) in &diff.changed {
                store.update_height(txid, *height);
            }
        } // <- release &mut tx_store

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
            let store = &mut self.tx_store;
            self.updates = self
                .updates
                .clone()
                .into_iter()
                .filter_map(|update| {
                    if update.is_complete() {
                        store.insert_updates(vec![update]);
                        None
                    } else {
                        Some(update)
                    }
                })
                .collect();
        } // <- release &mut tx_store

        // re-generate coin store from tx store
        self.generate();
    }

    // generate from tx_store
    pub fn generate(&mut self) {
        let addr_store = &mut self.address_store;
        let tx_store = &self.tx_store;

        let mut coins = BTreeMap::<OutPoint, CoinEntry>::new();

        // list all received coins
        for entry in tx_store.inner().values() {
            let tx = entry.tx();
            let txid = tx.compute_txid();
            for (vout, txout) in tx.output.iter().enumerate() {
                if let Some(addr) = addr_store.get_entry(&txout.script_pubkey) {
                    let txout = txout.clone();
                    let outpoint = OutPoint {
                        txid,
                        vout: vout as u32,
                    };
                    let height = entry.height();
                    let status = if height.is_some() {
                        CoinStatus::Confirmed
                    } else {
                        CoinStatus::Unconfirmed
                    };
                    let coin = signer::Coin {
                        txout,
                        outpoint,
                        // sequence is registered at spend time
                        // so fill with dummy value
                        sequence: bitcoin::Sequence::MAX,
                        coin_path: signer::CoinPath {
                            depth: addr.account_u32(),
                            index: Some(addr.index()),
                        },
                    };
                    let coin = CoinEntry {
                        height: entry.height(),
                        status,
                        coin,
                        address: addr.address(),
                    };
                    coins.insert(outpoint, coin);
                }
            }
        }
        // list all spent coins
        for tx_entry in tx_store.inner().values() {
            for inp in &tx_entry.tx().input {
                coins.entry(inp.previous_output).and_modify(|e| {
                    e.status = CoinStatus::Spent;
                });
            }
        }
        let mut spk_to_outpoint = BTreeMap::<ScriptBuf, HashSet<OutPoint>>::new();
        coins.iter().for_each(|(op, ce)| {
            spk_to_outpoint
                .entry(ce.spk())
                .and_modify(|e| {
                    e.insert(*op);
                })
                .or_insert({
                    let mut h = HashSet::new();
                    h.insert(*op);
                    h
                });
        });

        self.store = coins;
        self.spk_to_outpoint = spk_to_outpoint;

        // update address_store statuses
        self.spk_to_outpoint.iter().for_each(|(spk, op)| {
            let status = match op.len() {
                0 => AddressStatus::NotUsed,
                1 => AddressStatus::Used,
                _ => AddressStatus::Reused,
            };
            if let Some(e) = addr_store.get_entry_mut(spk) {
                e.set_status(status)
            }
        });

        // FIXME: update statuses of those w/ CoinStatus::BeeingSpent

        // TODO: do not unwrap
        self.notification.send(Notification::CoinUpdate).unwrap();
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
                CoinStatus::BeingSpend | CoinStatus::Spent => None,
                _ => unreachable!(),
            })
            .collect();
        out.set(coins);
        out
    }

    pub fn coins(&self) -> BTreeMap<bitcoin::OutPoint, CoinEntry> {
        self.store.clone()
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
    #[allow(unused)]
    spk: ScriptBuf,
    pub txs: Vec<(bitcoin::Txid, Option<bitcoin::Transaction>, Option<u64>)>,
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
            .filter_map(|(txid, tx, _)| if tx.is_none() { Some(*txid) } else { None })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinEntry {
    height: Option<u64>,
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
    pub fn spk(&self) -> ScriptBuf {
        self.address.clone().assume_checked().script_pubkey()
    }
}
