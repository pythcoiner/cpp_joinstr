use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use joinstr::{
    electrum::{CoinRequest, CoinResponse},
    miniscript::{
        bitcoin::{self, absolute, Amount, EcdsaSighashType, OutPoint, ScriptBuf},
        psbt::PsbtExt,
    },
    nostr::{self, error, sync::NostrClient, Pool},
    simple_nostr_client::nostr::key::Keys,
};

use crate::{
    address_store::{AddressEntry, AddressTip},
    coin::Coin,
    coin_store::{CoinEntry, CoinStore},
    config::Tip,
    cpp_joinstr::{
        AddrAccount, AddressStatus, CoinState, PoolStatus, RustAddress, RustPool, SignalFlag,
        TransactionTemplate,
    },
    derivator::Derivator,
    label_store::{LabelKey, LabelStore},
    pool_store::PoolStore,
    result,
    tx_store::TxStore,
    Config, PoolsResult, PsbtResult,
};

result!(Poll, Signal);

impl Poll {
    pub fn signal(&self) -> Box<Signal> {
        Box::new(self.value())
    }
}

/// Represents a signal that can either contain a value or an error message, emulating a Result type through bindings to C++.
#[derive(Debug, Default, Clone)]
pub struct Signal {
    inner: Option<SignalFlag>,
    error: Option<String>,
}
impl Signal {
    /// Creates a new `Signal` instance with no inner value or error.
    pub fn new() -> Self {
        Self {
            inner: None,
            error: None,
        }
    }
    /// Sets the inner value of the signal and clears any existing error.
    ///
    /// # Arguments
    ///
    /// * `value` - The `SignalFlag` to set as the inner value.
    pub fn set(&mut self, value: SignalFlag) {
        self.inner = Some(value);
        self.error = None;
    }
    /// Sets an error message for the signal and clears any existing inner value.
    ///
    /// # Arguments
    ///
    /// * `error` - The error message to set.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.inner = None;
    }
    /// Unwraps the inner value of the signal, assuming it is present.
    ///
    /// # Panics
    ///
    /// Panics if the inner value is not present.
    pub fn unwrap(&self) -> SignalFlag {
        self.inner.unwrap()
    }
    /// Returns a boxed version of the inner value of the signal.
    ///
    /// # Panics
    ///
    /// Panics if the inner value is not present.
    pub fn boxed(&self) -> Box<SignalFlag> {
        Box::new(self.inner.unwrap())
    }
    /// Returns the error message of the signal, if any.
    ///
    /// # Returns
    ///
    /// A string containing the error message, or an empty string if no error is present.
    pub fn error(&self) -> String {
        self.error.clone().unwrap_or_default()
    }
    /// Checks if the signal is in an "ok" state, meaning it has an inner value and no error.
    ///
    /// # Returns
    ///
    /// `true` if the signal is ok, `false` otherwise.
    pub fn is_ok(&self) -> bool {
        self.inner.is_some() && self.error.is_none()
    }
    /// Checks if the signal is in an error state.
    ///
    /// # Returns
    ///
    /// `true` if the signal is in an error state, `false` otherwise.
    pub fn is_err(&self) -> bool {
        let err = matches!(
            self.inner,
            Some(SignalFlag::TxListenerError)
                | Some(SignalFlag::PoolListenerError)
                | Some(SignalFlag::AccountError)
                | None
        );
        err && self.error.is_some()
    }
}

/// Represents different types of errors that can occur.
#[derive(Debug)]
pub enum Notification {
    Electrum(TxListenerNotif),
    Joinstr(JoinstrNotif),
    AddressTipChanged,
    CoinUpdate,
    InvalidElectrumConfig,
    InvalidNostrConfig,
    InvalidLookAhead,
    Stopped,
    Error(Error),
}

impl From<TxListenerNotif> for Notification {
    fn from(value: TxListenerNotif) -> Self {
        Notification::Electrum(value)
    }
}

impl From<JoinstrNotif> for Notification {
    fn from(value: JoinstrNotif) -> Self {
        Notification::Joinstr(value)
    }
}

impl From<joinstr::joinstr::Error> for Notification {
    fn from(value: joinstr::joinstr::Error) -> Self {
        Notification::Joinstr(JoinstrNotif::Error(Error::Joinstr(value)))
    }
}

impl From<joinstr::signer::Error> for Notification {
    fn from(value: joinstr::signer::Error) -> Self {
        Notification::Joinstr(JoinstrNotif::Error(Error::Signer(value)))
    }
}

impl From<nostr::error::Error> for Notification {
    fn from(value: nostr::error::Error) -> Self {
        Notification::Joinstr(JoinstrNotif::Error(Error::Nostr(value)))
    }
}

impl From<Error> for Notification {
    fn from(value: Error) -> Self {
        Self::Error(value)
    }
}

impl Notification {
    /// Converts a `Notification` into a `Signal`.
    ///
    /// # Returns
    ///
    /// A `Signal` representing the notification.
    pub fn to_signal(self) -> Signal {
        let mut signal = Signal::new();
        match self {
            Notification::Electrum(notif) => match notif {
                TxListenerNotif::Started => signal.set(SignalFlag::TxListenerStarted),
                TxListenerNotif::Error(e) => {
                    signal.set(SignalFlag::TxListenerError);
                    signal.set_error(e);
                }
                TxListenerNotif::Stopped => signal.set(SignalFlag::TxListenerStopped),
            },
            Notification::Joinstr(notif) => match notif {
                JoinstrNotif::Started => signal.set(SignalFlag::PoolListenerStarted),
                JoinstrNotif::PoolUpdate => signal.set(SignalFlag::PoolUpdate),
                JoinstrNotif::Stopped => signal.set(SignalFlag::PoolListenerStopped),
                JoinstrNotif::Error(e) => {
                    signal.set(SignalFlag::PoolListenerError);
                    signal.set_error(format!("{e:?}"));
                }
                JoinstrNotif::Stop => unreachable!(),
            },
            Notification::AddressTipChanged => signal.set(SignalFlag::AddressTipChanged),
            Notification::CoinUpdate => signal.set(SignalFlag::CoinUpdate),
            Notification::Stopped => signal.set(SignalFlag::Stopped),
            Notification::InvalidElectrumConfig => {
                signal.set(SignalFlag::AccountError);
                signal.set_error("Invalid electrum config".to_string());
            }
            Notification::InvalidNostrConfig => {
                signal.set(SignalFlag::AccountError);
                signal.set_error("Invalid nostr config".to_string());
            }
            Notification::InvalidLookAhead => {
                signal.set(SignalFlag::AccountError);
                signal.set_error("Invalid look_ahead value".to_string());
            }
            Notification::Error(e) => {
                signal.set(SignalFlag::Error);
                signal.set_error(format!("{e:?}"));
            }
        }
        signal
    }
}

#[derive(Debug)]
pub enum Error {
    Nostr(nostr::error::Error),
    Joinstr(joinstr::joinstr::Error),
    Signer(joinstr::signer::Error),
    CreatePool,
    JoinPool,
    InvalidOutPoint,
    CoinMissing,
    InvalidDenomination,
    RelayMissing,
    WrongElectrumConfig,
    PoolMissing,
}

impl From<nostr::error::Error> for Error {
    fn from(value: nostr::error::Error) -> Self {
        Error::Nostr(value)
    }
}

/// Represents notifications related to transaction listeners.
#[derive(Debug, Clone)]
pub enum TxListenerNotif {
    Started,
    Error(String),
    Stopped,
}

#[derive(Debug)]
pub enum JoinstrNotif {
    Started,
    PoolUpdate,
    Stopped,
    Stop,
    Error(Error),
}

impl From<nostr::error::Error> for JoinstrNotif {
    fn from(value: nostr::error::Error) -> Self {
        JoinstrNotif::Error(Error::Nostr(value))
    }
}

#[derive(Debug)]
pub struct Account {
    coin_store: Arc<Mutex<CoinStore>>,
    pool_store: Arc<Mutex<PoolStore>>,
    label_store: Arc<Mutex<LabelStore>>,
    receiver: mpsc::Receiver<Notification>,
    sender: mpsc::Sender<Notification>,
    tx_listener: Option<JoinHandle<()>>,
    pool_listener: Option<JoinHandle<()>>,
    config: Config,
    electrum_stop: Option<Arc<AtomicBool>>,
    nostr_stop: Option<Arc<AtomicBool>>,
}

impl Drop for Account {
    fn drop(&mut self) {
        if let Some(stop) = self.electrum_stop.as_mut() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(stop) = self.nostr_stop.as_mut() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

// Rust only interface
impl Account {
    /// Creates a new `Account` instance with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - The configuration for the account.
    ///
    /// # Returns
    ///
    /// A new `Account` instance.
    pub fn new(config: Config) -> Self {
        assert!(!config.account.is_empty());
        let (sender, receiver) = mpsc::channel();
        let tx_data = TxStore::store_from_file(config.transactions_path());
        let tx_store = TxStore::new(tx_data, Some(config.transactions_path()));
        let Tip { receive, change } = config.tip_from_file();
        let label_store = Arc::new(Mutex::new(LabelStore::from_file(config.clone())));
        let coin_store = Arc::new(Mutex::new(CoinStore::new(
            config.network,
            config.descriptor.clone(),
            sender.clone(),
            receive,
            change,
            config.look_ahead,
            tx_store,
            label_store.clone(),
            Some(config.clone()),
        )));
        coin_store.lock().expect("poisoned").generate();
        let pool_store = Arc::new(Mutex::new(PoolStore::new()));
        let mut account = Account {
            coin_store,
            pool_store,
            label_store,
            tx_listener: None,
            pool_listener: None,
            electrum_stop: None,
            nostr_stop: None,
            receiver,
            sender,
            config,
        };
        account.start_electrum();
        account.start_nostr();
        account
    }

    /// Returns a boxed version of the account.
    ///
    /// # Returns
    ///
    /// A boxed `Account` instance.
    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    /// Starts listening for transactions on the specified address and port.
    ///
    /// # Arguments
    ///
    /// * `addr` - The address to listen on.
    /// * `port` - The port to listen on.
    ///
    /// # Returns
    ///
    /// A tuple containing a sender for address tips and a stop flag.
    fn start_listen_txs(
        &mut self,
        addr: String,
        port: u16,
        config: Config,
    ) -> (mpsc::Sender<AddressTip>, Arc<AtomicBool>) {
        log::debug!("Account::start_poll_txs()");
        let (sender, address_tip) = mpsc::channel();
        let coin_store = self.coin_store.clone();
        let notification = self.sender.clone();
        let derivator = self.derivator();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_request = stop.clone();

        let poller = thread::spawn(move || {
            let client = match joinstr::electrum::Client::new(&addr, port) {
                Ok(c) => c,
                Err(e) => {
                    log::error!("start_listen_txs(): fail to create electrum client {}", e);
                    let _ = notification.send(TxListenerNotif::Error(e.to_string()).into());
                    return;
                }
            };

            let (request, response) = client.listen::<CoinRequest, CoinResponse>();

            listen_txs(
                coin_store,
                derivator,
                notification,
                address_tip,
                stop_request,
                request,
                response,
                Some(config),
            );
        });
        self.tx_listener = Some(poller);
        (sender, stop)
    }

    /// Starts polling pools with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `back` - The number of seconds in the past event publish date to retrieve.
    /// * `relay` - The relay address to connect to.
    ///
    /// # Returns
    ///
    /// A stop flag for the polling process.
    fn start_poll_pools(&mut self, back: u64, relay: String) -> Arc<AtomicBool> {
        log::debug!("Account::start_poll_pools()");
        let pool_store = self.pool_store.clone();
        let sender = self.sender.clone();

        let stop = Arc::new(AtomicBool::new(false));
        let cloned_stop = stop.clone();
        let poller = thread::spawn(move || {
            pool_listener(relay, pool_store, sender, back, cloned_stop);
        });
        self.pool_listener = Some(poller);
        stop
    }

    /// Returns the derivator associated with the account.
    ///
    /// # Returns
    ///
    /// A `Derivator` instance.
    pub fn derivator(&self) -> Derivator {
        self.coin_store.lock().expect("poisoned").derivator()
    }

    /// Returns a map of coins associated with the account.
    ///
    /// # Returns
    ///
    /// A `BTreeMap` of `OutPoint` to `CoinEntry`.
    pub fn coins(&self) -> BTreeMap<OutPoint, CoinEntry> {
        self.coin_store.lock().expect("poisoned").coins()
    }

    /// Returns the receiving address at the specified index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the receiving address.
    ///
    /// # Returns
    ///
    /// A `bitcoin::Address` instance.
    pub fn recv_at(&self, index: u32) -> bitcoin::Address {
        self.coin_store
            .lock()
            .expect("poisoned")
            .derivator_ref()
            .receive_at(index)
    }

    /// Returns the change address at the specified index.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the change address.
    ///
    /// # Returns
    ///
    /// A `bitcoin::Address` instance.
    pub fn change_at(&self, index: u32) -> bitcoin::Address {
        self.coin_store
            .lock()
            .expect("poisoned")
            .derivator_ref()
            .change_at(index)
    }

    /// Generates a new receiving address for the account.
    ///
    /// # Returns
    ///
    /// A `bitcoin::Address` instance.
    pub fn new_recv_addr(&mut self) -> bitcoin::Address {
        self.coin_store.lock().expect("poisoned").new_recv_addr()
    }
    /// Generates a new change address for the account.
    ///
    /// # Returns
    ///
    /// A `bitcoin::Address` instance.
    pub fn new_change_addr(&mut self) -> bitcoin::Address {
        self.coin_store.lock().expect("poisoned").new_change_addr()
    }

    /// Returns the current receiving watch tip index.
    ///
    /// # Returns
    ///
    /// The receiving watch tip index as a `u32`.
    pub fn recv_watch_tip(&self) -> u32 {
        self.coin_store.lock().expect("poisoned").recv_watch_tip()
    }

    /// Returns the current change watch tip index.
    ///
    /// # Returns
    ///
    /// The change watch tip index as a `u32`.
    pub fn change_watch_tip(&self) -> u32 {
        self.coin_store.lock().expect("poisoned").change_watch_tip()
    }
}

// C++ shared interface
impl Account {
    /// Re-generate coin_store from tx_store
    pub fn generate_coins(&mut self) {
        self.coin_store.lock().expect("poisoned").generate();
    }
    /// Returns spendable coins for the account.
    pub fn spendable_coins(&self) -> CoinState {
        self.coin_store.lock().expect("poisoned").spendable_coins()
    }

    /// Returns the coin matching the given outpoint if found, else None.
    pub fn get_coin(&self, outpoint: &OutPoint) -> Option<Coin> {
        self.coin_store
            .lock()
            .expect("poisoned")
            .get(outpoint)
            .map(|e| e.coin)
    }

    /// Prepare a PSBT ready to sign from a `TransactionTemplate`.
    ///
    /// # Returns
    ///
    /// A boxed PsbtResult containing the PSBT or the error string.
    pub fn prepare_transaction(&mut self, tx_template: TransactionTemplate) -> Box<PsbtResult> {
        // TODO: take output MAX in account
        // TODO: add a change when needed
        // TODO: handle fees
        let mut outpoints = vec![];
        for inp in tx_template.inputs {
            let parsed: Result<bitcoin::OutPoint, _> = serde_json::from_str(&inp.outpoint);
            match parsed {
                Ok(op) => outpoints.push(op),
                Err(_) => return "Fail to parse Outpoint".into(),
            }
        }

        let mut outputs = vec![];
        {
            let store = self.coin_store.lock().expect("poisoned");
            for out in tx_template.outputs {
                let addr = match bitcoin::Address::from_str(&out.address) {
                    Ok(a) => a,
                    Err(_) => return "Fail to parse address".into(),
                };
                if !addr.is_valid_for_network(self.config.network) {
                    return "Provided address is not valid for the current network".into();
                }
                let addr = addr.assume_checked();
                let txout = bitcoin::TxOut {
                    value: bitcoin::Amount::from_sat(out.amount),
                    script_pubkey: addr.script_pubkey(),
                };
                let deriv = store
                    .address_info(&addr.script_pubkey())
                    .map(|e| (e.account(), e.index()));
                outputs.push((txout, deriv));
            }
        } // <- drop coin_store lock here

        let inputs = {
            let store = self.coin_store.lock().expect("poisoned");
            let mut inputs = Vec::<CoinEntry>::new();
            for op in outpoints {
                match store.get(&op) {
                    Some(coin) => inputs.push(coin),
                    // TODO: support external inputs
                    None => return "Provided outpoint do not match an available coin".into(),
                }
            }
            inputs
        }; // <- release coin_store lock

        let mut tx = bitcoin::Transaction {
            version: bitcoin::transaction::Version(2),
            lock_time: absolute::LockTime::ZERO,
            input: vec![],
            output: vec![],
        };

        for i in &inputs {
            tx.input.push(i.txin());
        }

        for o in &outputs {
            tx.output.push(o.0.clone());
        }

        let mut psbt = match bitcoin::Psbt::from_unsigned_tx(tx) {
            Ok(psbt) => psbt,
            Err(_) => return "Fail to generate PSBT from unsigned transaction".into(),
        };

        let descriptor = self.config.descriptor.clone();
        let mut descriptors = descriptor
            .into_single_descriptors()
            .expect("have a multipath")
            .into_iter();
        let receive = descriptors.next().expect("have a multipath");
        let change = descriptors.next().expect("have a multipath");

        // Populate PSBT inputs
        for (index, coin) in inputs.iter().enumerate() {
            // NOTE: for now we consider all inputs to be owned by this descriptor
            let (account, addr_index) = coin.deriv();
            let descriptor = match account {
                AddrAccount::Receive => &receive,
                AddrAccount::Change => &change,
                _ => unreachable!(),
            };
            let definite = descriptor
                .at_derivation_index(addr_index)
                .expect("has a wildcard");
            if let Err(e) = psbt.update_input_with_descriptor(index, &definite) {
                log::error!("fail to update PSBT input w/ descriptor: {e}");
                return "Fail to update PSBT input with descriptor".into();
            }

            let input = psbt.inputs.get_mut(index).expect("valid index");

            // TODO: allow sighash to be passed in the TransactionTemplate
            input.sighash_type = Some(EcdsaSighashType::All.into());

            input.witness_utxo = Some(coin.txout());
        }

        // Populate PSBT outputs
        for (index, (_, deriv)) in outputs.iter().enumerate() {
            if let Some((account, addr_index)) = deriv {
                let descriptor = match *account {
                    AddrAccount::Receive => &receive,
                    AddrAccount::Change => &change,
                    _ => unreachable!(),
                };
                let definite = descriptor
                    .at_derivation_index(*addr_index)
                    .expect("has a wildcard");
                if let Err(e) = psbt.update_output_with_descriptor(index, &definite) {
                    log::error!("fail to update PSBT output w/ descriptor: {e}");
                    return "Fail to update PSBT output with descriptor".into();
                }
            }
        }
        PsbtResult::ok(psbt.to_string()).boxed()
    }

    /// Returns the available pools for the account.
    ///
    /// # Returns
    ///
    /// A boxed `Pools` instance containing the available pools.
    pub fn pools(&self) -> Box<PoolsResult> {
        let mut pools = match self.pool_store.try_lock() {
            Ok(lock) => {
                let pools = lock.available_pools();
                PoolsResult::ok(pools)
            }
            Err(_) => PoolsResult::err("PoolStore locked"),
        };
        pools.relay = self.relay();
        Box::new(pools)
    }

    /// Creates a new pool with the specified parameters.
    ///
    /// # Arguments
    ///
    /// * `outpoint` - The outpoint for the pool.
    /// * `denomination` - The denomination of the pool.
    /// * `fee` - The fee for the pool.
    /// * `max_duration` - The maximum duration of the pool.
    /// * `peers` - The number of peers in the pool.
    /// * `coin` - the outpoint of the coin to coinjoin.
    pub fn rust_create_pool(
        &mut self,
        outpoint: String,
        denomination: u64,
        fee: u32,
        timeout: u64,
        peers: usize,
    ) -> Result<(), Error> {
        let op = OutPoint::from_str(&outpoint).map_err(|_| Error::InvalidOutPoint)?;
        let coin = self.get_coin(&op).ok_or(Error::CoinMissing)?;
        let denomination = Amount::from_sat(denomination).to_btc();
        let address = self.new_recv_addr().as_unchecked().clone();
        let relay = self.config.nostr_relay.clone().ok_or(Error::RelayMissing)?;
        let electrum = if let (Some(url), Some(port)) =
            (self.config.electrum_url.clone(), self.config.electrum_port)
        {
            (url, port)
        } else {
            return Err(Error::WrongElectrumConfig);
        };
        PoolStore::create_pool(
            denomination,
            fee,
            timeout,
            peers,
            coin,
            address,
            self.config.mnemonic.clone(),
            relay,
            electrum,
            self.config.network,
            self.pool_store.clone(),
            self.sender.clone(),
        );
        Ok(())
    }

    pub fn create_pool(
        &mut self,
        outpoint: String,
        denomination: u64,
        fee: u32,
        timeout: u64,
        peers: usize,
    ) {
        if let Err(e) = self.rust_create_pool(outpoint, denomination, fee, timeout, peers) {
            let _ = self.sender.send(e.into());
        }
    }

    /// Joins an existing pool with the specified outpoint and pool ID.
    ///
    /// # Arguments
    ///
    /// * `_outpoint` - The outpoint for the pool.
    /// * `_pool_id` - The ID of the pool to join.
    pub fn rust_join_pool(&mut self, outpoint: String, pool_id: String) -> Result<(), Error> {
        let op = OutPoint::from_str(&outpoint).map_err(|_| Error::InvalidOutPoint)?;
        let coin = self.get_coin(&op).ok_or(Error::CoinMissing)?;
        let relay = self.config.nostr_relay.clone().ok_or(Error::RelayMissing)?;
        let electrum = if let (Some(url), Some(port)) =
            (self.config.electrum_url.clone(), self.config.electrum_port)
        {
            (url, port)
        } else {
            return Err(Error::WrongElectrumConfig);
        };
        let address = self.new_recv_addr().as_unchecked().clone();
        let pool = self.rust_pool(pool_id).ok_or(Error::PoolMissing)?;
        PoolStore::join_pool(
            relay,
            electrum,
            pool,
            self.config.mnemonic.clone(),
            self.config.network,
            self.pool_store.clone(),
            self.sender.clone(),
            coin,
            address,
        );
        Ok(())
    }

    pub fn join_pool(&mut self, outpoint: String, pool_id: String) {
        if let Err(e) = self.rust_join_pool(outpoint, pool_id) {
            let _ = self.sender.send(e.into());
        }
    }

    /// Retrieves a pool with the specified pool ID.
    ///
    /// # Arguments
    ///
    /// * `_pool_id` - The ID of the pool to retrieve.
    ///
    /// # Returns
    ///
    /// A boxed `Pool` instance.
    pub fn pool(&mut self, pool_id: String) -> Box<RustPool> {
        let entry = self
            .pool_store
            .lock()
            .expect("poisoned")
            .get(&pool_id)
            .unwrap();
        let pool = entry.into();

        Box::new(pool)
    }

    pub fn rust_pool(&mut self, pool_id: String) -> Option<Pool> {
        self.pool_store
            .lock()
            .expect("poisoned")
            .get(&pool_id)
            .map(|e| e.pool())
    }

    /// Sets the Electrum server URL and port for the account.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL of the Electrum server.
    /// * `port` - The port of the Electrum server.
    pub fn set_electrum(&mut self, url: String, port: String) {
        if let Ok(port) = port.parse::<u16>() {
            self.config.electrum_url = Some(url);
            self.config.electrum_port = Some(port);
            self.config.to_file();
        } else {
            self.sender
                .send(Notification::InvalidElectrumConfig)
                .expect("cannot fail");
        }
    }

    /// Starts the Electrum listener for the account.
    pub fn start_electrum(&mut self) {
        if let (None, Some(addr), Some(port)) = (
            &self.tx_listener,
            self.config.electrum_url.clone(),
            self.config.electrum_port,
        ) {
            let (tx_listener, electrum_stop) =
                self.start_listen_txs(addr, port, self.config.clone());
            self.coin_store.lock().expect("poisoned").init(tx_listener);
            self.electrum_stop = Some(electrum_stop);
        }
    }

    /// Stops the Electrum listener for the account.
    pub fn stop_electrum(&mut self) {
        if let Some(stop) = self.electrum_stop.as_mut() {
            stop.store(true, Ordering::Relaxed);
        }
        self.electrum_stop = None;
    }

    /// Sets the Nostr relay URL and back value for the account.
    ///
    /// # Arguments
    ///
    /// * `url` - The URL of the Nostr relay.
    /// * `back` - The back value for the Nostr relay.
    pub fn set_nostr(&mut self, url: String, back: String) {
        if let Ok(back) = back.parse::<u64>() {
            self.config.nostr_relay = Some(url);
            self.config.nostr_back = Some(back);
            self.config.to_file();
        } else if !(url.is_empty() && back.is_empty()) {
            self.sender
                .send(Notification::InvalidNostrConfig)
                .expect("cannot fail");
        }
    }

    /// Starts the Nostr listener for the account.
    pub fn start_nostr(&mut self) {
        if let (None, Some(relay), Some(back)) = (
            &self.pool_listener,
            self.config.nostr_relay.as_ref(),
            self.config.nostr_back,
        ) {
            let stop = self.start_poll_pools(back, relay.clone());
            self.nostr_stop = Some(stop);
        }
    }

    /// Stops the Nostr listener for the account.
    pub fn stop_nostr(&mut self) {
        if let Some(stop) = self.nostr_stop.as_mut() {
            stop.store(true, Ordering::Relaxed);
        }
        self.nostr_stop = None;
    }

    /// Sets the look-ahead value for the account.
    ///
    /// # Arguments
    ///
    /// * `look_ahead` - The look-ahead value to set.
    pub fn set_look_ahead(&mut self, look_ahead: String) {
        log::warn!("Account::set_look_ahead() {look_ahead}");
        if let Ok(la) = look_ahead.parse::<u32>() {
            self.config.look_ahead = la;
            self.config.to_file();
        } else {
            self.sender
                .send(Notification::InvalidNostrConfig)
                .expect("cannot fail");
        }
    }

    /// Returns the configuration of the account.
    ///
    /// # Returns
    ///
    /// A boxed `Config` instance.
    pub fn get_config(&self) -> Box<Config> {
        self.config.clone().boxed()
    }

    /// Attempts to receive a notification and convert it to a signal.
    ///
    /// # Returns
    ///
    /// A boxed `Poll` instance containing the signal.
    pub fn try_recv(&mut self) -> Box<Poll> {
        match self.receiver.try_recv() {
            Ok(notif) => {
                if let Notification::Electrum(TxListenerNotif::Stopped) = &notif {
                    self.electrum_stop = None;
                    self.tx_listener = None;
                } else if let Notification::Joinstr(JoinstrNotif::Stopped) = &notif {
                    self.nostr_stop = None;
                    self.pool_listener = None;
                }
                Poll::ok(notif.to_signal()).boxed()
            }
            Err(e) => match e {
                mpsc::TryRecvError::Disconnected => Poll::err("Disconnected").boxed(),
                mpsc::TryRecvError::Empty => Poll::new().boxed(),
            },
        }
    }

    /// Returns the receiving address at the specified index as a string.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the receiving address.
    ///
    /// # Returns
    ///
    /// The receiving address as a string.
    pub fn recv_addr_at(&self, index: u32) -> String {
        self.coin_store
            .lock()
            .expect("poisoned")
            .derivator_ref()
            .receive_at(index)
            .to_string()
    }

    /// Returns the change address at the specified index as a string.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the change address.
    ///
    /// # Returns
    ///
    /// The change address as a string.
    pub fn change_addr_at(&self, index: u32) -> String {
        self.coin_store
            .lock()
            .expect("poisoned")
            .derivator_ref()
            .change_at(index)
            .to_string()
    }

    /// Generates a new receiving address entry for the account.
    ///
    /// # Returns
    ///
    /// A boxed `AddressEntry` instance.
    pub fn new_addr(&mut self) -> RustAddress {
        let addr = self.new_recv_addr();
        let index = self.coin_store.lock().expect("poisoned").recv_tip();
        AddressEntry {
            status: AddressStatus::NotUsed,
            address: addr.as_unchecked().clone(),
            account: AddrAccount::Receive,
            index,
        }
        .into()
    }

    /// Edits the label of a coin identified by the given outpoint.
    ///
    /// # Arguments
    ///
    /// * `outpoint` - A string representation of the outpoint for the coin.
    /// * `label` - The new label to set for the coin. If the label is empty, the label will be removed.
    pub fn edit_coin_label(&self, outpoint: String, label: String) {
        if let Ok(outpoint) = bitcoin::OutPoint::from_str(&outpoint) {
            if !label.is_empty() {
                self.label_store
                    .lock()
                    .expect("poisoned")
                    .edit(LabelKey::OutPoint(outpoint), Some(label));
            } else {
                self.label_store
                    .lock()
                    .expect("poisoned")
                    .remove(LabelKey::OutPoint(outpoint));
            }
        }
        if let Ok(mut store) = self.coin_store.try_lock() {
            store.generate();
        }
    }

    /// Returns the Nostr relay URL for the account.
    ///
    /// # Returns
    ///
    /// The Nostr relay URL as a string.
    pub fn relay(&self) -> String {
        self.config.nostr_relay.clone().unwrap()
    }

    /// Stops all listeners and sends a stopped notification.
    pub fn stop(&mut self) {
        if self.electrum_stop.is_none() && self.nostr_stop.is_none() {
            self.sender.send(Notification::Stopped).unwrap();
            return;
        }

        if let Some(stopper) = &self.electrum_stop {
            stopper.store(true, Ordering::Relaxed);
        }
        if let Some(stopper) = &self.nostr_stop {
            stopper.store(true, Ordering::Relaxed);
        }

        let notification = self.sender.clone();
        let tx_listener = self.tx_listener.take();
        let pool_listener = self.pool_listener.take();

        thread::spawn(move || loop {
            let tx_stopped = if let Some(handle) = tx_listener.as_ref() {
                handle.is_finished()
            } else {
                true
            };
            let pool_stopped = if let Some(handle) = pool_listener.as_ref() {
                handle.is_finished()
            } else {
                true
            };

            if tx_stopped && pool_stopped {
                notification.send(Notification::Stopped).unwrap();
            }
        });
    }
}

/// Creates a new account with the specified account name.
///
/// # Arguments
///
/// * `account` - The name of the account.
///
/// # Returns
///
/// A boxed `Account` instance.
pub fn new_account(account: String) -> Box<Account> {
    let config = Config::from_file(account);

    let account = Account::new(config);
    account.boxed()
}

macro_rules! send_notif {
    ($notification:expr, $request:expr, $msg:expr) => {
        let res = $notification.send($msg.into());
        if res.is_err() {
            // stop detached client
            let _ = $request.send(CoinRequest::Stop);
            return;
        }
    };
}

macro_rules! send_electrum {
    ($request:expr, $notification:expr, $msg:expr) => {
        if $request.send($msg).is_err() {
            send_notif!($notification, $request, TxListenerNotif::Stopped);
            return;
        }
    };
}

/// Listens for transactions on the specified address and port.
///
/// # Arguments
///
/// * `addr` - The address to listen on.
/// * `port` - The port to listen on.
/// * `coin_store` - The coin store to update with transaction data.
/// * `signer` - The signer for the account.
/// * `notification` - The sender for notifications.
/// * `address_tip` - The receiver for address tips.
/// * `stop_request` - The stop flag for the listener.
#[allow(clippy::too_many_arguments)]
fn listen_txs<T: From<TxListenerNotif>>(
    coin_store: Arc<Mutex<CoinStore>>,
    derivator: Derivator,
    notification: mpsc::Sender<T>,
    address_tip: mpsc::Receiver<AddressTip>,
    stop_request: Arc<AtomicBool>,
    request: mpsc::Sender<CoinRequest>,
    response: mpsc::Receiver<CoinResponse>,
    config: Option<Config>,
) {
    log::info!("listen_txs(): started");
    send_notif!(notification, request, TxListenerNotif::Started);

    let mut statuses = if let Some(config) = &config {
        config.statuses_from_file()
    } else {
        BTreeMap::<ScriptBuf, (Option<String>, u32, u32)>::new()
    };

    if !statuses.is_empty() {
        let sub: Vec<_> = statuses.keys().cloned().collect();
        send_electrum!(request, notification, CoinRequest::Subscribe(sub));
    }

    fn persist_status(
        config: &Option<Config>,
        statuses: &BTreeMap<ScriptBuf, (Option<String>, u32, u32)>,
    ) {
        if let Some(cfg) = config.as_ref() {
            cfg.persist_statuses(statuses);
        }
    }

    loop {
        // stop request from consumer side
        if stop_request.load(Ordering::Relaxed) {
            send_notif!(notification, request, TxListenerNotif::Stopped);
            let _ = request.send(CoinRequest::Stop);
            return;
        }

        let mut received = false;

        // listen for AddressTip update
        match address_tip.try_recv() {
            Ok(tip) => {
                log::debug!("listen_txs() receive {tip:?}");
                let AddressTip { recv, change } = tip;
                received = true;
                let mut sub = vec![];
                let r_spk = derivator.receive_at(recv).script_pubkey();
                if !statuses.contains_key(&r_spk) {
                    // FIXME: here we can be smart an not start at 0 but at `actual_tip`
                    for i in 0..recv {
                        let spk = derivator.receive_at(i).script_pubkey();
                        if !statuses.contains_key(&spk) {
                            statuses.insert(spk.clone(), (None, 0, i));
                            persist_status(&config, &statuses);
                            sub.push(spk);
                        }
                    }
                }
                let c_spk = derivator.change_at(recv).script_pubkey();
                if !statuses.contains_key(&c_spk) {
                    // FIXME: here we can be smart an not start at 0 but at `actual_tip`
                    for i in 0..change {
                        let spk = derivator.change_at(i).script_pubkey();
                        if !statuses.contains_key(&spk) {
                            statuses.insert(spk.clone(), (None, 1, i));
                            persist_status(&config, &statuses);
                            sub.push(spk);
                        }
                    }
                }
                if !sub.is_empty() {
                    send_electrum!(request, notification, CoinRequest::Subscribe(sub));
                }
            }
            Err(e) => match e {
                mpsc::TryRecvError::Empty => {}
                mpsc::TryRecvError::Disconnected => {
                    log::error!("listen_txs(): address store disconnected");
                    send_notif!(
                        notification,
                        request,
                        TxListenerNotif::Error("AddressStore disconnected".to_string())
                    );
                    // FIXME: what should we do there?
                    // it's AddressStore being dropped, but she should keep upating
                    // the actual spk set even if it cannot grow anymore
                }
            },
        }

        // listen for response
        match response.try_recv() {
            Ok(rsp) => {
                log::debug!("listen_txs() receive {rsp:#?}");
                received = true;
                match rsp {
                    CoinResponse::Status(elct_status) => {
                        let mut history = vec![];
                        for (spk, status) in elct_status {
                            if let Some((s, _, _)) = statuses.get_mut(&spk) {
                                // status is registered
                                if *s != status {
                                    // status changed
                                    if status.is_some() {
                                        // status is not empty so we ask for txs changes
                                        history.push(spk);
                                    } else {
                                        // status change from Some(_) to None we directly update
                                        // coin_store
                                        let mut store = coin_store.lock().expect("poisoned");
                                        let mut map = BTreeMap::new();
                                        map.insert(spk.clone(), vec![]);
                                        let _ = store.handle_history_response(map);
                                        store.generate();
                                    }
                                    // record the local status change
                                    *s = status;
                                }
                            } else if status.is_some() {
                                // status is not None & not registered
                                statuses.entry(spk.clone()).and_modify(|s| s.0 = status);
                                persist_status(&config, &statuses);
                                history.push(spk);
                            } else {
                                // status is None & not registered

                                // record local status
                                statuses.entry(spk.clone()).and_modify(|s| s.0 = status);
                                persist_status(&config, &statuses);

                                // update coin_store
                                let mut store = coin_store.lock().expect("poisoned");
                                let mut map = BTreeMap::new();
                                map.insert(spk.clone(), vec![]);
                                let _ = store.handle_history_response(map);
                            }
                        }
                        if !history.is_empty() {
                            let hist = CoinRequest::History(history);
                            log::debug!("listen_txs() send {:#?}", hist);
                            send_electrum!(request, notification, hist);
                        }
                        persist_status(&config, &statuses);
                    }
                    CoinResponse::History(map) => {
                        let mut store = coin_store.lock().expect("poisoned");
                        let (height_updated, missing_txs) = store.handle_history_response(map);
                        if !missing_txs.is_empty() {
                            send_electrum!(request, notification, CoinRequest::Txs(missing_txs));
                        }
                        if height_updated {
                            store.generate();
                        }
                    }
                    CoinResponse::Txs(txs) => {
                        let mut store = coin_store.lock().expect("poisoned");
                        store.handle_txs_response(txs);
                    }
                    CoinResponse::Stopped => {
                        send_notif!(notification, request, TxListenerNotif::Stopped);
                        let _ = request.send(CoinRequest::Stop);
                        return;
                    }
                    CoinResponse::Error(e) => {
                        send_notif!(notification, request, TxListenerNotif::Error(e));
                    }
                }
            }
            Err(e) => match e {
                mpsc::TryRecvError::Empty => {}
                mpsc::TryRecvError::Disconnected => {
                    // NOTE: here the electrum client is dropped, we cannot continue
                    log::error!("listen_txs() electrum client stopped unexpectedly");
                    send_notif!(notification, request, TxListenerNotif::Stopped);
                    let _ = request.send(CoinRequest::Stop);
                    return;
                }
            },
        }

        if received {
            continue;
        }
        // FIXME: 20 ms is likely WAY too much
        thread::sleep(Duration::from_millis(20));
    }
}

/// Listens for pool notifications on the specified relay.
///
/// # Arguments
///
/// * `relay` - The relay address to connect to.
/// * `pool_store` - The pool store to update with pool data.
/// * `sender` - The sender for notifications.
/// * `back` - The number of past events to retrieve.
/// * `stop_request` - The stop flag for the listener.
fn pool_listener<N: From<JoinstrNotif> + Send + 'static>(
    relay: String,
    pool_store: Arc<Mutex<PoolStore>>,
    sender: mpsc::Sender<N>,
    back: u64,
    stop_request: Arc<AtomicBool>,
) {
    let mut pool_listener = NostrClient::new("pool_listener")
        .relay(relay.clone())
        .expect("not connected")
        .keys(Keys::generate())
        .expect("not connected");

    if let Err(e) = pool_listener.connect_nostr() {
        log::error!("pool_listener() fail to connect to nostr relay: {e:?}");
        let msg: JoinstrNotif = e.into();
        let _ = sender.send(msg.into());
        return;
    }
    if let Err(e) = pool_listener.subscribe_pools(back) {
        log::error!("pool_listener() fail to subscribe to pool notifications: {e:?}");
        let msg: JoinstrNotif = e.into();
        let _ = sender.send(msg.into());
        return;
    }

    loop {
        if stop_request.load(Ordering::Relaxed) {
            log::error!("pool_listener() stop requested");
            let msg = JoinstrNotif::Stopped;
            let _ = sender.send(msg.into());
            return;
        }

        let pool = match pool_listener.receive_pool_notification() {
            Ok(Some(pool)) => pool,
            Ok(None) => {
                thread::sleep(Duration::from_millis(300));
                continue;
            }
            Err(e) => match e {
                error::Error::Disconnected | error::Error::NotConnected => {
                    log::error!("pool_listener() connexion lost: {e:?}");
                    // connexion lost try to reconnect
                    pool_listener = NostrClient::new("pool_listener")
                        .relay(relay.clone())
                        .expect("not connected")
                        .keys(Keys::generate())
                        .expect("not connected");

                    if let Err(e) = pool_listener.connect_nostr() {
                        log::error!("pool_listener() fail to reconnect: {e:?}");
                        let msg: JoinstrNotif = e.into();
                        let _ = sender.send(msg.into());
                        let msg = JoinstrNotif::Stopped;
                        let _ = sender.send(msg.into());
                        return;
                    }
                    if let Err(e) = pool_listener.subscribe_pools(back) {
                        log::error!(
                            "pool_listener() fail to subscribe to pool notifications: {e:?}"
                        );
                        let msg: JoinstrNotif = e.into();
                        let _ = sender.send(msg.into());
                        let msg = JoinstrNotif::Stopped;
                        let _ = sender.send(msg.into());
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                e => {
                    log::error!("pool_listener() unexpected error: {e:?}");
                    let msg: JoinstrNotif = e.into();
                    let _ = sender.send(msg.into());
                    let msg = JoinstrNotif::Stopped;
                    let _ = sender.send(msg.into());
                    return;
                }
            },
        };
        {
            let mut store = pool_store.lock().expect("poisoned");
            store.update(pool, PoolStatus::Available);
            if sender.send(JoinstrNotif::PoolUpdate.into()).is_err() {
                return;
            }
        } // release store lock
    }
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::mpsc::TryRecvError};

    use joinstr::{bip39, miniscript::bitcoin::bip32::DerivationPath};

    use crate::{
        cpp_joinstr::CoinStatus,
        signer::{wpkh, HotSigner},
        test_utils::{funding_tx, setup_logger, spending_tx},
        tx_store::TxStore,
    };

    use super::*;

    struct CoinStoreMock {
        pub store: Arc<Mutex<CoinStore>>,
        pub notif: mpsc::Receiver<Notification>,
        pub request: mpsc::Receiver<CoinRequest>,
        pub response: mpsc::Sender<CoinResponse>,
        pub listener: JoinHandle<()>,
        pub stop: Arc<AtomicBool>,
        pub derivator: Derivator,
    }

    impl Drop for CoinStoreMock {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    impl CoinStoreMock {
        fn new(recv_tip: u32, change_tip: u32, look_ahead: u32) -> Self {
            let (notif_sender, notif_recv) = mpsc::channel();
            let (tip_sender, tip_receiver) = mpsc::channel();
            let (req_sender, req_receiver) = mpsc::channel();
            let (resp_sender, resp_receiver) = mpsc::channel();

            let mnemonic = bip39::Mnemonic::generate(12).unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let signer =
                HotSigner::new_from_mnemonics(bitcoin::Network::Regtest, &mnemonic.to_string())
                    .unwrap();
            let xpub = signer.xpub(&DerivationPath::from_str("m/84'/0'/0'/1").unwrap());
            let descriptor = wpkh(xpub);
            let derivator = Derivator::new(descriptor.clone(), bitcoin::Network::Regtest).unwrap();

            let tx_store = TxStore::new(Default::default(), None);
            let label_store = Arc::new(Mutex::new(LabelStore::new()));
            let coin_store = Arc::new(Mutex::new(CoinStore::new(
                bitcoin::Network::Regtest,
                descriptor.clone(),
                notif_sender.clone(),
                recv_tip,
                change_tip,
                look_ahead,
                tx_store,
                label_store,
                None,
            )));
            coin_store.lock().expect("poisoned").init(tip_sender);
            let store = coin_store.clone();
            let cloned_stop = stop.clone();
            let cloned_derivator = derivator.clone();

            let listener_handle = thread::spawn(move || {
                listen_txs(
                    coin_store,
                    cloned_derivator,
                    notif_sender,
                    tip_receiver,
                    stop,
                    req_sender,
                    resp_receiver,
                    None,
                );
            });

            CoinStoreMock {
                store,
                notif: notif_recv,
                request: req_receiver,
                response: resp_sender,
                listener: listener_handle,
                stop: cloned_stop,
                derivator,
            }
        }

        fn coins(&mut self) -> BTreeMap<OutPoint, CoinEntry> {
            self.store.lock().expect("poisoned").coins()
        }

        fn stop(&self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn simple_start_stop() {
        setup_logger();
        let mock = CoinStoreMock::new(0, 0, 20);
        thread::sleep(Duration::from_millis(10));
        assert!(!mock.listener.is_finished());
        assert!(matches!(
            mock.notif.try_recv().unwrap(),
            Notification::AddressTipChanged,
        ));
        assert!(matches!(
            mock.notif.try_recv().unwrap(),
            Notification::Electrum(TxListenerNotif::Started)
        ));
        mock.stop();
        thread::sleep(Duration::from_secs(1));
        assert!(mock.listener.is_finished());
    }

    fn simple_recv() -> (bitcoin::Transaction, CoinStoreMock) {
        setup_logger();
        let look_ahead = 5;
        let mut mock = CoinStoreMock::new(0, 0, look_ahead);
        thread::sleep(Duration::from_millis(500));
        assert!(!mock.listener.is_finished());
        assert!(matches!(
            mock.notif.try_recv().unwrap(),
            Notification::AddressTipChanged,
        ));
        assert!(matches!(
            mock.notif.try_recv().unwrap(),
            Notification::Electrum(TxListenerNotif::Started)
        ));

        let mut init_spks = vec![];
        for i in 0..(look_ahead + 1) {
            let spk = mock.derivator.receive_spk_at(i);
            init_spks.push(spk);
        }
        for i in 0..(look_ahead + 1) {
            let spk = mock.derivator.change_spk_at(i);
            init_spks.push(spk);
        }

        // receive initial subscriptions
        if let Ok(CoinRequest::Subscribe(v)) = mock.request.try_recv() {
            // NOTE: we expect (tip + 1 + look_ahead )
            assert_eq!(v.len(), 12);
            for spk in &init_spks {
                assert!(v.contains(spk));
            }
        } else {
            panic!()
        }

        // electrum server send spks statuses (None)
        let statuses: BTreeMap<_, _> = init_spks.clone().into_iter().map(|s| (s, None)).collect();
        mock.response.send(CoinResponse::Status(statuses)).unwrap();

        thread::sleep(Duration::from_millis(100));

        assert!(mock.coins().is_empty());

        let spk_recv_0 = mock.derivator.receive_spk_at(0);

        // server send a status update at recv(0)
        let mut statuses = BTreeMap::new();
        statuses.insert(spk_recv_0.clone(), Some("1_tx_unco".to_string()));
        mock.response.send(CoinResponse::Status(statuses)).unwrap();
        thread::sleep(Duration::from_millis(100));

        // server should receive an history request for this spk
        if let Ok(CoinRequest::History(v)) = mock.request.try_recv() {
            assert!(v == vec![spk_recv_0.clone()]);
        } else {
            panic!()
        }

        thread::sleep(Duration::from_millis(100));

        let tx_0 = funding_tx(spk_recv_0.clone(), 0.1);

        // server must send history response
        let mut history = BTreeMap::new();
        history.insert(spk_recv_0.clone(), vec![(tx_0.compute_txid(), None)]);
        mock.response.send(CoinResponse::History(history)).unwrap();

        thread::sleep(Duration::from_millis(100));

        // server should receive a tx request
        if let Ok(CoinRequest::Txs(v)) = mock.request.try_recv() {
            assert!(v == vec![tx_0.compute_txid()]);
        } else {
            panic!()
        }

        thread::sleep(Duration::from_millis(100));

        // server send the requested tx
        mock.response
            .send(CoinResponse::Txs(vec![tx_0.clone()]))
            .unwrap();

        thread::sleep(Duration::from_millis(100));

        // now the store contain one coin
        let mut coins = mock.coins();
        assert_eq!(coins.len(), 1);
        let coin = coins.pop_first().unwrap().1;

        // the coin is unconfirmed
        assert_eq!(coin.height(), None);
        assert_eq!(coin.status(), CoinStatus::Unconfirmed);

        // NOTE: the coin is now confirmed

        // server send a status update at recv(0)
        let mut statuses = BTreeMap::new();
        statuses.insert(spk_recv_0.clone(), Some("1_tx_conf".to_string()));
        mock.response.send(CoinResponse::Status(statuses)).unwrap();
        thread::sleep(Duration::from_millis(100));

        // server should receive an history request for this spk
        if let Ok(CoinRequest::History(v)) = mock.request.try_recv() {
            assert!(v == vec![spk_recv_0.clone()]);
        } else {
            panic!()
        }

        thread::sleep(Duration::from_millis(100));

        // server must send history response
        let mut history = BTreeMap::new();
        // the coin have now 1 confirmation
        history.insert(spk_recv_0.clone(), vec![(tx_0.compute_txid(), Some(1))]);
        mock.response.send(CoinResponse::History(history)).unwrap();

        thread::sleep(Duration::from_millis(100));

        // NOTE: coin_store already have the tx it should not ask it
        assert!(matches!(mock.request.try_recv(), Err(TryRecvError::Empty)));

        // the coin is now confirmed
        let mut coins = mock.coins();
        assert_eq!(coins.len(), 1);
        let coin = coins.pop_first().unwrap().1;
        assert_eq!(coin.height(), Some(1));
        assert_eq!(coin.status(), CoinStatus::Confirmed);
        (tx_0, mock)
    }

    #[test]
    fn recv_and_spend() {
        // init & receive one coin
        let (tx_0, mut mock) = simple_recv();
        let spk_recv_0 = mock.derivator.receive_spk_at(0);

        // spend this coin
        let outpoint = mock.coins().pop_first().unwrap().0;
        let tx_1 = spending_tx(outpoint);

        // NOTE: the coin is now spent

        // server send a status update at recv(0)
        let mut statuses = BTreeMap::new();
        statuses.insert(spk_recv_0.clone(), Some("1_tx_spent".to_string()));
        mock.response.send(CoinResponse::Status(statuses)).unwrap();
        thread::sleep(Duration::from_millis(100));

        // server should receive an history request for this spk
        if let Ok(CoinRequest::History(v)) = mock.request.try_recv() {
            assert!(v == vec![spk_recv_0.clone()]);
        } else {
            panic!()
        }

        thread::sleep(Duration::from_millis(100));

        // server must send history response
        let mut history = BTreeMap::new();
        // the coin have now 1 confirmation
        history.insert(
            spk_recv_0.clone(),
            vec![(tx_0.compute_txid(), Some(1)), (tx_1.compute_txid(), None)],
        );
        mock.response.send(CoinResponse::History(history)).unwrap();

        thread::sleep(Duration::from_millis(100));

        // server should receive a tx request only for tx_1
        if let Ok(CoinRequest::Txs(v)) = mock.request.try_recv() {
            assert!(v == vec![tx_1.compute_txid()]);
        } else {
            panic!()
        }

        // server send the requested tx
        mock.response
            .send(CoinResponse::Txs(vec![tx_1.clone()]))
            .unwrap();

        thread::sleep(Duration::from_millis(100));

        // now the store contain one spent coin
        let mut coins = mock.coins();
        assert_eq!(coins.len(), 1);
        let coin = coins.pop_first().unwrap().1;

        // the coin is unconfirmed
        assert_eq!(coin.status(), CoinStatus::Spent);
    }

    #[test]
    fn simple_reorg() {
        // init & receive one coin
        let (tx_0, mut mock) = simple_recv();
        let spk_recv_0 = mock.derivator.receive_spk_at(0);

        // NOTE: the coin is now spent we can reorg it

        // server send a status update at recv(0)
        let mut statuses = BTreeMap::new();
        statuses.insert(spk_recv_0.clone(), Some("1_tx_reorg".to_string()));
        mock.response.send(CoinResponse::Status(statuses)).unwrap();
        thread::sleep(Duration::from_millis(100));

        // server should receive an history request for this spk
        if let Ok(CoinRequest::History(v)) = mock.request.try_recv() {
            assert!(v == vec![spk_recv_0.clone()]);
        } else {
            panic!()
        }

        thread::sleep(Duration::from_millis(100));

        // server must send history response
        let mut history = BTreeMap::new();
        // NOTE: confirmation height is changed to 2
        history.insert(spk_recv_0.clone(), vec![(tx_0.compute_txid(), Some(2))]);
        mock.response.send(CoinResponse::History(history)).unwrap();

        thread::sleep(Duration::from_millis(100));

        // server do not receive a tx request as the store already go the tx
        assert!(matches!(mock.request.try_recv(), Err(TryRecvError::Empty)));

        // the store still contain one spent coin
        let mut coins = mock.coins();
        assert_eq!(coins.len(), 1);
        let coin = coins.pop_first().unwrap().1;

        // the coin have a confirmation height of 2
        assert_eq!(coin.height(), Some(2));
    }
}
