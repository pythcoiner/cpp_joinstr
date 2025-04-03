use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use joinstr::{
    bip39,
    electrum::{CoinRequest, CoinResponse},
    miniscript::bitcoin::{self, OutPoint, ScriptBuf},
    nostr::{self, error, sync::NostrClient},
    signer::WpkhHotSigner,
    simple_nostr_client::nostr::key::Keys,
};

use crate::{
    address_store::{AddressEntry, AddressTip},
    coin_store::{CoinEntry, CoinStore},
    cpp_joinstr::{AddrAccount, AddressStatus, PoolStatus, SignalFlag},
    pool_store::PoolStore,
    result, Coins, Config, Mnemonic, Pool, Pools,
};

result!(Poll, Signal);

#[derive(Default, Clone)]
pub struct Signal {
    inner: Option<SignalFlag>,
    error: Option<String>,
}
impl Signal {
    pub fn new() -> Self {
        Self {
            inner: None,
            error: None,
        }
    }
    pub fn set(&mut self, value: SignalFlag) {
        self.inner = Some(value);
        self.error = None;
    }
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.inner = None;
    }
    pub fn unwrap(&self) -> SignalFlag {
        self.inner.unwrap()
    }
    pub fn boxed(&self) -> Box<SignalFlag> {
        Box::new(self.inner.unwrap())
    }
    pub fn error(&self) -> String {
        self.error.clone().unwrap_or_default()
    }
    pub fn is_ok(&self) -> bool {
        self.inner.is_some() && self.error.is_none()
    }
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

#[derive(Debug)]
pub enum Notification {
    Electrum(TxListenerNotif),
    Joinstr(PoolListenerNotif),
    AddressTipChanged,
    CoinUpdate,
    InvalidElectrumConfig,
    InvalidNostrConfig,
}

impl From<TxListenerNotif> for Notification {
    fn from(value: TxListenerNotif) -> Self {
        Notification::Electrum(value)
    }
}

impl From<PoolListenerNotif> for Notification {
    fn from(value: PoolListenerNotif) -> Self {
        Notification::Joinstr(value)
    }
}

impl Notification {
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
                PoolListenerNotif::Started => signal.set(SignalFlag::PoolListenerStarted),
                PoolListenerNotif::PoolUpdate => signal.set(SignalFlag::PoolUpdate),
                PoolListenerNotif::Stopped => signal.set(SignalFlag::PoolListenerStopped),
                PoolListenerNotif::Error(e) => {
                    signal.set(SignalFlag::PoolListenerError);
                    signal.set_error(format!("{e:?}"));
                }
                PoolListenerNotif::Stop => unreachable!(),
            },
            Notification::AddressTipChanged => signal.set(SignalFlag::AddressTipChanged),
            Notification::CoinUpdate => signal.set(SignalFlag::CoinUpdate),
            Notification::InvalidElectrumConfig => {
                signal.set(SignalFlag::AccountError);
                signal.set_error("Invalid electrum config".to_string());
            }
            Notification::InvalidNostrConfig => {
                signal.set(SignalFlag::AccountError);
                signal.set_error("Invalid nostr config".to_string());
            }
        }
        signal
    }
}

#[derive(Debug)]
pub enum Error {
    Nostr(nostr::error::Error),
}

impl From<nostr::error::Error> for Error {
    fn from(value: nostr::error::Error) -> Self {
        Error::Nostr(value)
    }
}

#[derive(Debug, Clone)]
pub enum TxListenerNotif {
    Started,
    Error(String),
    Stopped,
}

#[derive(Debug)]
pub enum PoolListenerNotif {
    Started,
    PoolUpdate,
    Stopped,
    Stop,
    Error(Error),
}

impl From<nostr::error::Error> for PoolListenerNotif {
    fn from(value: nostr::error::Error) -> Self {
        PoolListenerNotif::Error(Error::Nostr(value))
    }
}

#[derive(Debug)]
pub struct Account {
    coin_store: Arc<Mutex<CoinStore>>,
    pool_store: Arc<Mutex<PoolStore>>,
    receiver: mpsc::Receiver<Notification>,
    sender: mpsc::Sender<Notification>,
    tx_listener: Option<JoinHandle<()>>,
    pool_listener: Option<JoinHandle<()>>,
    config: Config,
    electrum_stop: Option<Arc<AtomicBool>>,
    nostr_stop: Option<Arc<AtomicBool>>,
}

// Rust only interface
impl Account {
    pub fn new(mnemonic: bip39::Mnemonic, config: Config) -> Self {
        assert!(!config.account.is_empty());
        let (sender, receiver) = mpsc::channel();
        // TODO: import saved state from local storage
        let coin_store = Arc::new(Mutex::new(CoinStore::new(
            config.network,
            mnemonic,
            sender.clone(),
            0,
            0,
            config.look_ahead,
        )));
        // TODO: use indexes from stored state
        let mut account = Account {
            coin_store,
            pool_store: Default::default(),
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

    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    fn start_listen_txs(
        &mut self,
        addr: String,
        port: u16,
    ) -> (mpsc::Sender<AddressTip>, Arc<AtomicBool>) {
        log::debug!("Account::start_poll_txs()");
        let (sender, address_tip) = mpsc::channel();
        let coin_store = self.coin_store.clone();
        let notification = self.sender.clone();
        let signer = self.signer();
        let stop = Arc::new(AtomicBool::new(false));
        let cloned_stop = stop.clone();
        let poller = thread::spawn(move || {
            listen_txs(
                addr,
                port,
                coin_store,
                signer,
                notification,
                address_tip,
                cloned_stop,
            );
        });
        self.tx_listener = Some(poller);
        (sender, stop)
    }

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

    pub fn signer(&self) -> WpkhHotSigner {
        self.coin_store.lock().expect("poisoned").signer()
    }

    pub fn coins(&self) -> BTreeMap<OutPoint, CoinEntry> {
        self.coin_store.lock().expect("poisoned").coins()
    }

    pub fn recv_at(&self, index: u32) -> bitcoin::Address {
        self.coin_store
            .lock()
            .expect("poisoned")
            .signer_ref()
            .recv_addr_at(index)
    }

    pub fn change_at(&self, index: u32) -> bitcoin::Address {
        self.coin_store
            .lock()
            .expect("poisoned")
            .signer_ref()
            .change_addr_at(index)
    }

    pub fn new_recv_addr(&mut self) -> bitcoin::Address {
        self.coin_store.lock().expect("poisoned").new_recv_addr()
    }
    pub fn new_change_addr(&mut self) -> bitcoin::Address {
        self.coin_store.lock().expect("poisoned").new_change_addr()
    }

    pub fn recv_watch_tip(&self) -> u32 {
        self.coin_store.lock().expect("poisoned").recv_watch_tip()
    }

    pub fn change_watch_tip(&self) -> u32 {
        self.coin_store.lock().expect("poisoned").change_watch_tip()
    }
}

// C++ shared interface
impl Account {
    pub fn spendable_coins(&self) -> Box<Coins> {
        match self.coin_store.try_lock() {
            Ok(lock) => Box::new(lock.spendable_coins()),
            Err(_) => {
                let mut coins = Coins::new();
                coins.set_error("CoinStore Locked".to_string());
                Box::new(coins)
            }
        }
    }

    pub fn pools(&self) -> Box<Pools> {
        match self.pool_store.try_lock() {
            Ok(lock) => Box::new(lock.available_pools()),
            Err(_) => {
                let mut pools = Pools::new();
                pools.set_error("PoolStore Locked".to_string());
                Box::new(pools)
            }
        }
    }

    pub fn create_pool(
        &mut self,
        _outpoint: String,
        _denomination: f64,
        _fee: u32,
        _max_duration: u64,
        _peers: usize,
    ) {
        todo!()
    }

    pub fn join_pool(&mut self, _outpoint: String, _pool_id: String) {
        todo!()
    }

    pub fn pool(&mut self, _pool_id: String) -> Box<Pool> {
        todo!()
    }

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

    pub fn start_electrum(&mut self) {
        if let (None, Some(addr), Some(port)) = (
            &self.tx_listener,
            self.config.electrum_url.clone(),
            self.config.electrum_port,
        ) {
            let (tx_poller, electrum_stop) = self.start_listen_txs(addr, port);
            self.coin_store.lock().expect("poisoned").init(tx_poller);
            self.electrum_stop = Some(electrum_stop);
        }
    }

    pub fn stop_electrum(&mut self) {
        if let Some(stop) = self.electrum_stop.as_mut() {
            stop.store(true, Ordering::Relaxed);
        }
        self.electrum_stop = None;
    }

    pub fn set_nostr(&mut self, url: String, back: String) {
        if let Ok(back) = back.parse::<u64>() {
            self.config.nostr_relay = Some(url);
            self.config.nostr_back = Some(back);
            self.config.to_file();
        } else {
            self.sender
                .send(Notification::InvalidNostrConfig)
                .expect("cannot fail");
        }
    }

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

    pub fn stop_nostr(&mut self) {
        if let Some(stop) = self.nostr_stop.as_mut() {
            stop.store(true, Ordering::Relaxed);
        }
        self.nostr_stop = None;
    }

    pub fn get_config(&self) -> Box<Config> {
        self.config.clone().boxed()
    }

    pub fn try_recv(&mut self) -> Box<Poll> {
        let mut poll = Poll::new();
        match self.receiver.try_recv() {
            Ok(notif) => {
                if let Notification::Electrum(TxListenerNotif::Stopped) = &notif {
                    self.electrum_stop = None;
                    self.tx_listener = None;
                } else if let Notification::Joinstr(PoolListenerNotif::Stopped) = &notif {
                    self.nostr_stop = None;
                    self.pool_listener = None;
                }
                poll.set(notif.to_signal());
            }
            Err(e) => match e {
                mpsc::TryRecvError::Disconnected => poll.set_error("Disconnected".to_string()),
                mpsc::TryRecvError::Empty => {}
            },
        }
        Box::new(poll)
    }

    pub fn recv_addr_at(&self, index: u32) -> String {
        self.coin_store
            .lock()
            .expect("poisoned")
            .signer_ref()
            .recv_addr_at(index)
            .to_string()
    }

    pub fn change_addr_at(&self, index: u32) -> String {
        self.coin_store
            .lock()
            .expect("poisoned")
            .signer_ref()
            .change_addr_at(index)
            .to_string()
    }

    pub fn new_addr(&mut self) -> Box<AddressEntry> {
        let addr = self.new_recv_addr();
        let index = self.coin_store.lock().expect("poisoned").recv_tip();
        Box::new(AddressEntry {
            status: AddressStatus::NotUsed,
            address: addr.as_unchecked().clone(),
            account: AddrAccount::Receive,
            index,
        })
    }

    pub fn create_dummy_pool(&self, denomination: u64, peers: usize, timeout: u64, fee: u32) {
        if let Some(nostr_relay) = &self.config.nostr_relay {
            let relay = nostr_relay.clone();
            let network = self.config.network;
            thread::spawn(move || {
                dummy_pool(relay, denomination, peers, timeout, fee, network);
            });
        }
    }

    pub fn relay(&self) -> String {
        self.config.nostr_relay.clone().unwrap()
    }
}

pub fn new_account(
    #[allow(clippy::boxed_local)] mnemonic: Box<Mnemonic>,
    account: String,
) -> Box<Account> {
    let config = Config::from_file(account);

    let account = Account::new((*mnemonic).into(), config);
    account.boxed()
}

macro_rules! send_notif {
    ($notification:expr, $stop:expr, $msg:expr) => {
        let res = $notification.send($msg.into());
        if res.is_err() {
            // stop detached client
            $stop.store(true, Ordering::Relaxed);
            return;
        }
    };
}

macro_rules! send_electrum {
    ($request:expr, $notification:expr, $stop:expr, $msg:expr) => {
        if $request.send($msg).is_err() {
            send_notif!($notification, $stop, TxListenerNotif::Stopped);
            return;
        }
    };
}

fn listen_txs<T: From<TxListenerNotif>>(
    addr: String,
    port: u16,
    coin_store: Arc<Mutex<CoinStore>>,
    signer: WpkhHotSigner,
    notification: mpsc::Sender<T>,
    address_tip: mpsc::Receiver<AddressTip>,
    stop_request: Arc<AtomicBool>,
) {
    let client = match joinstr::electrum::Client::new(&addr, port) {
        Ok(c) => c,
        Err(e) => {
            log::error!("listen_txs(): fail to create electrum client {}", e);
            send_notif!(
                notification,
                // dummy stop
                Arc::new(AtomicBool::new(false)),
                TxListenerNotif::Error(e.to_string())
            );
            return;
        }
    };

    // FIXME: here we can have a single map
    // TODO: this map should be stored to avoid poll every spk at each launch
    let mut paths = BTreeMap::<ScriptBuf, (u32, u32)>::new();
    let mut statuses = BTreeMap::<ScriptBuf, Option<String>>::new();

    let (request, response, stop) = client.listen::<CoinRequest, CoinResponse>();

    log::info!("listen_txs(): started");
    send_notif!(notification, stop, TxListenerNotif::Started);

    loop {
        // stop request from consumer side
        if stop_request.load(Ordering::Relaxed) {
            send_notif!(notification, stop, TxListenerNotif::Stopped);
            return;
        }

        let mut received = false;

        // listen for AddressTip update
        match address_tip.try_recv() {
            Ok(tip) => {
                log::debug!("tx_poller() receive {tip:?}");
                let AddressTip { recv, change } = tip;
                received = true;
                let mut sub = vec![];
                let r_spk = signer.recv_addr_at(recv).script_pubkey();
                if !statuses.contains_key(&r_spk) {
                    // FIXME: here we can be smart an not start at 0 but at `actual_tip`
                    for i in 0..recv {
                        let spk = signer.recv_addr_at(i).script_pubkey();
                        if !statuses.contains_key(&spk) {
                            paths.insert(spk.clone(), (0, i));
                            statuses.insert(spk.clone(), None);
                            sub.push(spk);
                        }
                    }
                }
                let c_spk = signer.change_addr_at(recv).script_pubkey();
                if !statuses.contains_key(&c_spk) {
                    // FIXME: here we can be smart an not start at 0 but at `actual_tip`
                    for i in 0..change {
                        let spk = signer.recv_addr_at(i).script_pubkey();
                        if !statuses.contains_key(&spk) {
                            paths.insert(spk.clone(), (1, i));
                            statuses.insert(spk.clone(), None);
                            sub.push(spk);
                        }
                    }
                }
                send_electrum!(request, notification, stop, CoinRequest::Subscribe(sub));
            }
            Err(e) => match e {
                mpsc::TryRecvError::Empty => {}
                mpsc::TryRecvError::Disconnected => {
                    log::error!("listen_txs(): address store disconnected");
                    send_notif!(
                        notification,
                        stop,
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
                log::debug!("tx_poller() receive {rsp:#?}");
                received = true;
                match rsp {
                    CoinResponse::Status(elct_status) => {
                        let mut history = vec![];
                        for (spk, status) in elct_status {
                            if let Some(s) = statuses.get_mut(&spk) {
                                if *s != status {
                                    history.push(spk);
                                    *s = status;
                                }
                            } else {
                                statuses.insert(spk.clone(), status);
                                history.push(spk);
                            }
                        }
                        if !history.is_empty() {
                            send_electrum!(
                                request,
                                notification,
                                stop,
                                CoinRequest::History(history)
                            );
                        }
                    }
                    CoinResponse::History(map) => {
                        let mut store = coin_store.lock().expect("poisoned");
                        let missing_txs = store.handle_history_response(map);
                        if !missing_txs.is_empty() {
                            send_electrum!(
                                request,
                                notification,
                                stop,
                                CoinRequest::Txs(missing_txs)
                            );
                        }
                    }
                    CoinResponse::Txs(txs) => {
                        let mut store = coin_store.lock().expect("poisoned");
                        store.handle_txs_response(txs);
                    }
                    CoinResponse::Stopped => {
                        send_notif!(notification, stop, TxListenerNotif::Stopped);
                        stop.store(true, Ordering::Relaxed);
                        return;
                    }
                    CoinResponse::Error(e) => {
                        send_notif!(notification, stop, TxListenerNotif::Error(e));
                    }
                }
            }
            Err(e) => match e {
                mpsc::TryRecvError::Empty => {}
                mpsc::TryRecvError::Disconnected => {
                    // NOTE: here the electrum client is dropped, we cannot continue
                    log::error!("listen_txs() electrum client stopped unexpectedly");
                    send_notif!(notification, stop, TxListenerNotif::Stopped);
                    stop.store(true, Ordering::Relaxed);
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

fn pool_listener<N: From<PoolListenerNotif> + Send + 'static>(
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
        let msg: PoolListenerNotif = e.into();
        let _ = sender.send(msg.into());
        return;
    }
    if let Err(e) = pool_listener.subscribe_pools(back) {
        log::error!("pool_listener() fail to subscribe to pool notifications: {e:?}");
        let msg: PoolListenerNotif = e.into();
        let _ = sender.send(msg.into());
        return;
    }

    loop {
        if stop_request.load(Ordering::Relaxed) {
            log::error!("pool_listener() stop requested");
            let msg = PoolListenerNotif::Stopped;
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
                        let msg: PoolListenerNotif = e.into();
                        let _ = sender.send(msg.into());
                        let msg = PoolListenerNotif::Stopped;
                        let _ = sender.send(msg.into());
                        return;
                    }
                    if let Err(e) = pool_listener.subscribe_pools(back) {
                        log::error!(
                            "pool_listener() fail to subscribe to pool notifications: {e:?}"
                        );
                        let msg: PoolListenerNotif = e.into();
                        let _ = sender.send(msg.into());
                        let msg = PoolListenerNotif::Stopped;
                        let _ = sender.send(msg.into());
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                e => {
                    log::error!("pool_listener() unexpected error: {e:?}");
                    let msg: PoolListenerNotif = e.into();
                    let _ = sender.send(msg.into());
                    let msg = PoolListenerNotif::Stopped;
                    let _ = sender.send(msg.into());
                    return;
                }
            },
        };
        {
            let mut store = pool_store.lock().expect("poisoned");
            store.update(pool, PoolStatus::Available);
            if sender.send(PoolListenerNotif::PoolUpdate.into()).is_err() {
                return;
            }
        } // release store lock
    }
}

fn dummy_pool(
    relay: String,
    denomination: u64,
    peers: usize,
    timeout: u64,
    fee: u32,
    network: bitcoin::Network,
) {
    let mut client = NostrClient::new("pool_listener")
        .relay(relay.clone())
        .unwrap()
        .keys(Keys::generate())
        .unwrap();
    if let Err(e) = client.connect_nostr() {
        println!("dummy_pool() fail connect nostr relay: {:?}", e);
    }
    let key = client.get_keys().expect("have keys").public_key();

    let pool = nostr::Pool::create(relay, denomination, peers, timeout, fee, network, key);
    if let Err(e) = client.post_event(pool.try_into().expect("valid pool")) {
        println!("dummy_pool() fail to broadcast pool: {:?}", e);
    }
}
