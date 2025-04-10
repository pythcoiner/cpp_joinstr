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
/// Represents a store for managing coins and their associated data.
///
/// The `CoinStore` is generated from the transaction store after every
/// TxStore update and acts as a cache for coins. It maintains mappings
/// of outpoints to coin entries and tracks the history of script public
/// keys (SPKs).
pub struct CoinStore {
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
/// Represents the history of transactions for a specific script public key (SPK).
///
/// The `SpkHistory` stores a history of txids and their associated
/// heights, allowing for tracking incremental changes over time.
pub struct SpkHistory {
    history: Vec<BTreeMap<bitcoin::Txid, Option<u64>>>,
}

#[derive(Debug, Default)]
/// Represents the differences in transaction history for a script public key.
///
/// The `HistoryDiff` struct contains the added, changed, and removed
/// transactions, allowing for easy tracking of updates to the SPK history.
pub struct HistoryDiff {
    pub added: BTreeMap<bitcoin::Txid, Option<u64>>,
    pub changed: BTreeMap<bitcoin::Txid, Option<u64>>,
    pub removed: BTreeMap<bitcoin::Txid, Option<u64>>,
}

impl SpkHistory {
    /// Creates a new instance of `SpkHistory`.
    ///
    /// This method initializes the history with default values.
    pub fn new() -> Self {
        Self {
            history: vec![BTreeMap::default()],
        }
    }
    /// Inserts new transaction data into the SPK history and returns the differences.
    ///
    /// This method compares the new transaction data with the existing history
    /// and returns a `HistoryDiff` struct indicating added, changed, and removed
    /// transactions.
    pub fn insert(&mut self, new: Vec<(bitcoin::Txid, Option<u64>)>) -> HistoryDiff {
        let new: BTreeMap<_, _> = new.into_iter().collect();
        assert!(!self.history.is_empty());

        // last state have no txs
        let diff = if self.history.last().expect("not empty").is_empty() {
            if new.is_empty() {
                HistoryDiff::default()
            } else {
                self.history.push(new.clone());
                HistoryDiff {
                    added: new.clone(),
                    ..Default::default()
                }
            }
        } else {
            let mut diff = HistoryDiff::default();
            {
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
            }
            // FIXME: do not insert if last == new
            self.history.push(new);
            diff
        };
        diff
    }
}

impl CoinStore {
    /// Creates a new instance of `CoinStore`.
    ///
    /// # Parameters
    /// - `network`: The Bitcoin network to use.
    /// - `mnemonic`: The mnemonic phrase for generating keys.
    /// - `notification`: Channel for sending notifications about updates.
    /// - `recv_tip`: Initial index for receiving address generation.
    /// - `change_tip`: Initial index for change address generation.
    /// - `look_ahead`: Number of addresses to generate ahead of the current tip.
    ///
    /// # Returns
    /// A new instance of `CoinStore`.
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

    /// Initializes the address store with a channel to the tx listener.
    ///
    /// This method sets up the address store to send updates to the
    /// specified transaction listener.
    pub fn init(&mut self, tx_listener: mpsc::Sender<AddressTip>) {
        self.address_store.init(tx_listener);
    }
    /// Returns a clone of the signer used for generating addresses.
    ///
    /// # Returns
    /// A `WpkhHotSigner` instance.
    pub fn signer(&self) -> WpkhHotSigner {
        self.signer.clone()
    }
    /// Returns a reference to the signer used for generating addresses.
    ///
    /// # Returns
    /// A reference to a `WpkhHotSigner`.
    pub fn signer_ref(&self) -> &WpkhHotSigner {
        &self.signer
    }
    /// Returns the current receiving watch tip index.
    ///
    /// # Returns
    /// The index of the last generated receiving address.
    pub fn recv_watch_tip(&self) -> u32 {
        self.address_store.recv_watch_tip()
    }

    /// Returns the current change watch tip index.
    ///
    /// # Returns
    /// The index of the last generated change address.
    pub fn change_watch_tip(&self) -> u32 {
        self.address_store.change_watch_tip()
    }

    /// Generates a new receiving address.
    ///
    /// # Returns
    /// A new `bitcoin::Address` for receiving funds.
    pub fn new_recv_addr(&mut self) -> bitcoin::Address {
        self.address_store.new_recv_addr()
    }

    /// Returns the current receiving address tip index.
    ///
    /// # Returns
    /// The index of the last generated receiving address.
    pub fn recv_tip(&self) -> u32 {
        self.address_store.recv_tip()
    }

    /// Generates a new change address.
    ///
    /// # Returns
    /// A new `bitcoin::Address` for change outputs.
    pub fn new_change_addr(&mut self) -> bitcoin::Address {
        self.address_store.new_change_addr()
    }

    /// Processes a received coin at the specified script public key.
    ///
    /// # Parameters
    /// - `spk`: The script public key of the received coin.
    pub fn recv_coin_at(&mut self, spk: &ScriptBuf) {
        self.address_store.recv_coin_at(spk);
    }

    /// Handles the response containing transaction history for SPKs.
    ///
    /// This method processes the history and updates the internal state of the
    /// `CoinStore`. It returns a list of transaction IDs that are missing.
    ///
    /// # Parameters
    /// - `hist`: A map of script public keys to their transaction history.
    ///
    /// # Returns
    /// A vector of `Txid` representing missing transactions.
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

    /// Updates the history for a specific script public key (SPK).
    ///
    /// This method generates a diff of the SPK history and updates the
    /// transaction store accordingly.
    ///
    /// # Parameters
    /// - `spk`: The script public key to update.
    /// - `history`: The new transaction history for the SPK.
    ///
    /// # Returns
    /// An `Update` representing the changes made to the SPK history.
    ///
    /// Note: triggered on history_get response
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

    /// Handles the response containing transactions.
    ///
    /// This method processes the received transactions and updates the
    /// internal state of the `CoinStore`. It regenerates the coin store
    /// from the transaction store.
    ///
    /// # Parameters
    /// - `txs`: A vector of Bitcoin transactions received.
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

    /// Generates the coin store from the transaction store.
    ///
    /// This method populates the coin store with coins based on the
    /// transactions in the transaction store and updates the address
    /// statuses accordingly.
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

        if let Err(e) = self.notification.send(Notification::CoinUpdate) {
            log::error!("CoinStore::generate() fail to send notification: {e:?}");
        }
    }

    /// Retrieves coins by their status.
    ///
    /// This method filters the coins in the store based on the specified
    /// status and returns them as a `Coins` object.
    ///
    /// # Parameters
    /// - `status`: The status of the coins to retrieve.
    ///
    /// # Returns
    /// A `Coins` object containing the filtered coins.
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

    /// Retrieves spendable coins from the store.
    ///
    /// This method filters the coins that are either unconfirmed or
    /// confirmed and returns them as a `Coins` object.
    ///
    /// # Returns
    /// A `Coins` object containing the spendable coins.
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

    /// Returns all coins in the store.
    ///
    /// # Returns
    /// A `BTreeMap` of outpoints to their corresponding `CoinEntry`.
    pub fn coins(&self) -> BTreeMap<bitcoin::OutPoint, CoinEntry> {
        self.store.clone()
    }

    /// Dumps the coin store as a JSON value.
    ///
    /// # Returns
    /// A `Result` containing the serialized JSON value of the coin store
    /// or an error if serialization fails.
    pub fn dump(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::to_value(&self.store)
    }

    /// Restores the coin store from a JSON value.
    ///
    /// # Parameters
    /// - `value`: The JSON value to restore the coin store from.
    ///
    /// # Returns
    /// A `Result` indicating success or failure of the restoration.
    pub fn restore(&mut self, value: serde_json::Value) -> Result<(), serde_json::Error> {
        self.store = serde_json::from_value(value)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
/// Represents an update to the transaction history for a script public key (SPK).
///
/// The `Update` struct contains the script public key and a list of
/// transactions that have been added or changed.
pub struct Update {
    #[allow(unused)]
    spk: ScriptBuf,
    pub txs: Vec<(bitcoin::Txid, Option<bitcoin::Transaction>, Option<u64>)>,
}

impl Update {
    /// Creates an `Update` from the differences in SPK history.
    ///
    /// # Parameters
    /// - `spk`: The script public key associated with the update.
    /// - `diff`: The differences in the SPK history.
    ///
    /// # Returns
    /// A new `Update` instance.
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

    /// Checks if the update is complete.
    ///
    /// An update is considered complete if all transactions have been
    /// received.
    ///
    /// # Returns
    /// `true` if the update is complete, otherwise `false`.
    pub fn is_complete(&self) -> bool {
        self.txs.iter().all(|(_, tx, _)| tx.is_some())
    }

    /// Returns a list of missing transaction IDs in the update.
    ///
    /// # Returns
    /// A vector of `Txid` representing transactions that are missing.
    pub fn missing(&self) -> Vec<Txid> {
        self.txs
            .iter()
            .filter_map(|(txid, tx, _)| if tx.is_none() { Some(*txid) } else { None })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a coin entry in the coin store.
///
/// The `CoinEntry` struct contains information about the coin's height,
/// status, associated coin data, and the address it belongs to.
pub struct CoinEntry {
    height: Option<u64>,
    status: CoinStatus,
    coin: signer::Coin,
    address: bitcoin::Address<NetworkUnchecked>,
}

impl CoinEntry {
    /// Returns the height of the coin in the blockchain.
    ///
    /// # Returns
    /// An `Option<u64>` representing the height of the coin,
    /// or `None` if the coin is not confirmed.
    pub fn height(&self) -> Option<u64> {
        self.height
    }
    /// Returns the status of the coin.
    ///
    /// # Returns
    /// The `CoinStatus` of the coin.
    pub fn status(&self) -> CoinStatus {
        self.status
    }
    /// Returns a string representation of the coin's status.
    ///
    /// # Returns
    /// A string describing the coin's status.
    pub fn status_str(&self) -> String {
        format!("{:?}", self.status)
    }
    /// Returns the amount of the coin in satoshis.
    ///
    /// # Returns
    /// The value of the coin in satoshis.
    pub fn amount_sat(&self) -> u64 {
        self.coin.txout.value.to_sat()
    }
    /// Returns the amount of the coin in Bitcoin.
    ///
    /// # Returns
    /// The value of the coin in Bitcoin.
    pub fn amount_btc(&self) -> f64 {
        self.coin.txout.value.to_btc()
    }
    /// Returns a reference to the coin's outpoint.
    ///
    /// # Returns
    /// A reference to the `OutPoint` of the coin.
    pub fn outpoint(&self) -> &OutPoint {
        &self.coin.outpoint
    }
    /// Returns a string representation of the coin's outpoint.
    ///
    /// # Returns
    /// A string describing the coin's outpoint.
    pub fn outpoint_str(&self) -> String {
        self.outpoint().to_string()
    }
    /// Returns a boxed version of the coin entry.
    ///
    /// # Returns
    /// A `Box` containing the coin entry.
    pub fn boxed(&self) -> Box<CoinEntry> {
        Box::new(self.clone())
    }
    /// Returns the address associated with the coin.
    ///
    /// # Returns
    /// A string representation of the coin's address.
    pub fn address(&self) -> String {
        self.address.clone().assume_checked().to_string()
    }
    /// Returns the script public key (SPK) associated with the coin.
    ///
    /// # Returns
    /// The `ScriptBuf` representing the coin's SPK.
    pub fn spk(&self) -> ScriptBuf {
        self.address.clone().assume_checked().script_pubkey()
    }
}
