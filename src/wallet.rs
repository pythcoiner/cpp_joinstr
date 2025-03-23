use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use joinstr::{
    bip39,
    electrum::{CoinRequest, CoinResponse},
    miniscript::bitcoin::{self, ScriptBuf},
    nostr::{self, error, sync::NostrClient},
    signer::{self, WpkhHotSigner},
    simple_nostr_client::nostr::key::Keys,
};

use crate::{
    address_store::{AddressStore, AddressTip},
    coin_store::CoinStore,
    cpp_joinstr::{CoinStatus, Network, PoolStatus, SignalFlag},
    pool_store::PoolStore,
    result,
    tx_store::TxStore,
    Coins, Mnemonic, Pool, Pools,
};

result!(Poll, Signal);

result!(Signal, SignalFlag);

#[derive(Debug)]
pub enum Notification {
    Electrum(CoinPollerMsg),
    Joinstr(PoolPollerMsg),
    AddressTipChanged,
}

impl From<CoinPollerMsg> for Notification {
    fn from(value: CoinPollerMsg) -> Self {
        Notification::Electrum(value)
    }
}

impl From<PoolPollerMsg> for Notification {
    fn from(value: PoolPollerMsg) -> Self {
        Notification::Joinstr(value)
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
pub enum CoinPollerMsg {
    CoinUpdate {
        coin: signer::Coin,
        status: CoinStatus,
    },
    Started(mpsc::Sender<PoolPollerMsg>),
    Stop,
    Error(String),
    Stopped,
}

#[derive(Debug)]
pub enum PoolPollerMsg {
    Started(mpsc::Sender<PoolPollerMsg>),
    Stop,
    Error(Error),
}

impl From<nostr::error::Error> for PoolPollerMsg {
    fn from(value: nostr::error::Error) -> Self {
        PoolPollerMsg::Error(Error::Nostr(value))
    }
}

#[derive(Debug)]
pub struct Wallet {
    coin_store: Arc<Mutex<CoinStore>>,
    pool_store: Arc<Mutex<PoolStore>>,
    address_store: Arc<Mutex<AddressStore>>,
    tx_store: Arc<Mutex<TxStore>>,
    signals: mpsc::Receiver<Signal>,
    signal_sender: mpsc::Sender<Signal>,
    receiver: mpsc::Receiver<Notification>,
    sender: mpsc::Sender<Notification>,
    electrum_channel: Option<mpsc::Sender<CoinPollerMsg>>,
    pool_listener_channel: Option<mpsc::Sender<PoolPollerMsg>>,
    signer: WpkhHotSigner,
    coin_poller: Option<JoinHandle<()>>,
    pool_poller: Option<JoinHandle<()>>,
    electrum_url: String,
    electrum_port: u16,
    nostr_relay: String,
    network: bitcoin::Network,
}

// Rust only interface
impl Wallet {
    fn new(
        mnemonic: bip39::Mnemonic,
        network: bitcoin::Network,
        electrum_url: String,
        electrum_port: u16,
        nostr_relay: String,
        back: u64,
    ) -> Self {
        let (signal_sender, signals) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        // TODO: import saved state from local storage
        let signer = WpkhHotSigner::new_from_mnemonics(network, &mnemonic.to_string())
            .expect("valid mnemonic");
        let address_store = Arc::new(Mutex::new(AddressStore::new(
            signer.clone(),
            sender.clone(),
            20,
            20,
        )));
        let tx_store = Arc::new(Mutex::new(TxStore::new()));
        let coin_store = Arc::new(Mutex::new(CoinStore::new(
            signer.clone(),
            address_store.clone(),
            tx_store.clone(),
        )));
        // TODO: use indexes from stored state
        let mut wallet = Wallet {
            coin_store,
            pool_store: Default::default(),
            address_store,
            tx_store,
            signals,
            signal_sender,
            signer,
            coin_poller: None,
            pool_poller: None,
            electrum_url,
            electrum_port,
            nostr_relay,
            network,
            receiver,
            sender,
            electrum_channel: None,
            pool_listener_channel: None,
        };
        let tx_poller = wallet.start_poll_txs();
        wallet.start_poll_pools(back);
        wallet
            .address_store
            // FIXME: should we loop try_lock() instead
            .lock()
            .expect("poisoned")
            .init(tx_poller);
        wallet
    }

    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    fn start_poll_txs(&mut self) -> mpsc::Sender<AddressTip> {
        println!("Wallet::start_poll_txs()");
        let (sender, address_tip) = mpsc::channel();
        let coin_store = self.coin_store.clone();
        let tx_store = self.tx_store.clone();
        let notification = self.sender.clone();
        let signer = self.signer.clone();
        let addr = self.electrum_url.clone();
        let port = self.electrum_port;
        let poller = thread::spawn(move || {
            tx_poller(
                addr,
                port,
                coin_store,
                tx_store,
                signer,
                notification,
                address_tip,
            );
        });
        self.coin_poller = Some(poller);
        sender
    }

    fn start_poll_pools(&mut self, back: u64) {
        println!("Wallet::start_poll_pools()");
        let relay = self.nostr_relay.clone();
        let pool_store = self.pool_store.clone();
        let sender = self.sender.clone();

        let poller = thread::spawn(move || {
            pool_poller(relay, pool_store, sender, back);
        });
        self.pool_poller = Some(poller);
    }
}

// C++ shared interface
impl Wallet {
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

    pub fn try_recv(&mut self) -> Box<Poll> {
        let mut poll = Poll::new();
        match self.signals.try_recv() {
            Ok(signal) => poll.set(signal),
            Err(e) => match e {
                mpsc::TryRecvError::Disconnected => poll.set_error("Disconnected".to_string()),
                mpsc::TryRecvError::Empty => {}
            },
        }
        Box::new(poll)
    }

    pub fn recv_addr_at(&self, index: u32) -> String {
        self.signer.recv_addr_at(index).to_string()
    }

    pub fn change_addr_at(&self, index: u32) -> String {
        self.signer.change_addr_at(index).to_string()
    }

    pub fn create_dummy_pool(&self, denomination: u64, peers: usize, timeout: u64, fee: u32) {
        let relay = self.nostr_relay.clone();
        let network = self.network;
        thread::spawn(move || {
            dummy_pool(relay, denomination, peers, timeout, fee, network);
        });
    }

    pub fn relay(&self) -> String {
        self.nostr_relay.clone()
    }
}

pub fn new_wallet(
    #[allow(clippy::boxed_local)] mnemonic: Box<Mnemonic>,
    network: Network,
    addr: String,
    port: u16,
    relay: String,
    back: u64,
) -> Box<Wallet> {
    Wallet::new((*mnemonic).into(), network.into(), addr, port, relay, back).boxed()
}

fn tx_poller<T: From<CoinPollerMsg>>(
    addr: String,
    port: u16,
    coin_store: Arc<Mutex<CoinStore>>,
    tx_store: Arc<Mutex<TxStore>>,
    signer: WpkhHotSigner,
    notification: mpsc::Sender<T>,
    address_tip: mpsc::Receiver<AddressTip>,
) {
    let mut client = match joinstr::electrum::Client::new(&addr, port) {
        Ok(c) => c,
        Err(e) => {
            println!("wallet_poll(): fail to create electrum client {}", e);
            notification
                .send(CoinPollerMsg::Error(e.to_string()).into())
                .unwrap();
            return;
        }
    };

    // FIXME: here we can have a single map
    // TODO: this map should be stored to avoid poll every spk at each launch
    let mut paths = BTreeMap::<ScriptBuf, (u32, u32)>::new();
    let mut statuses = BTreeMap::<ScriptBuf, Option<String>>::new();

    let (request, response) = client.listen::<CoinRequest, CoinResponse>();

    loop {
        let mut received = false;

        // listen for AddressTip update
        match address_tip.try_recv() {
            Ok(AddressTip { recv, change }) => {
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
                request.send(CoinRequest::Subscribe(sub)).unwrap();
            }
            Err(e) => match e {
                mpsc::TryRecvError::Empty => {}
                mpsc::TryRecvError::Disconnected => {
                    // FIXME: what should we do there?
                    // it's AddressStore being dropped, but she should keep upating
                    // the actual spk set even if it cannot grow anymore
                }
            },
        }

        // TODO: listen for response
        match response.try_recv() {
            Ok(rsp) => {
                received = true;
                match rsp {
                    CoinResponse::Status(elct_status) => {
                        let mut history = vec![];
                        for (spk, status) in elct_status {
                            statuses.entry(spk.clone()).and_modify(|e| {
                                if *e != status {
                                    *e = status;
                                    history.push(spk);
                                }
                            });
                            // FIXME: should we or_insert() just in case?
                        }
                        // TODO: do not unwrap
                        request.send(CoinRequest::History(history)).unwrap();
                    }
                    CoinResponse::History(map) => {
                        // TODO:
                    }
                    CoinResponse::Txs(items) => {
                        // TODO:
                    }
                    CoinResponse::Stopped => {
                        notification.send(CoinPollerMsg::Stopped.into()).unwrap();
                        return;
                    }
                    CoinResponse::Error(e) => {
                        // TODO: do not unwrap
                        notification.send(CoinPollerMsg::Error(e).into()).unwrap();
                    }
                }
            }
            Err(e) => match e {
                mpsc::TryRecvError::Empty => {}
                mpsc::TryRecvError::Disconnected => {
                    // NOTE: here the electrum client is dropped, we cannot continue
                    // TODO: do not unwrap
                    notification.send(CoinPollerMsg::Stopped.into()).unwrap();
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

fn pool_poller<N: From<PoolPollerMsg> + Send + 'static>(
    relay: String,
    pool_store: Arc<Mutex<PoolStore>>,
    sender: mpsc::Sender<N>,
    back: u64,
) {
    let mut pool_listener = NostrClient::new("pool_listener")
        .relay(relay.clone())
        .unwrap()
        .keys(Keys::generate())
        .unwrap();
    if let Err(e) = pool_listener.connect_nostr() {
        let msg: PoolPollerMsg = e.into();
        sender.send(msg.into()).unwrap();
    }
    if let Err(e) = pool_listener.subscribe_pools(back) {
        let msg: PoolPollerMsg = e.into();
        sender.send(msg.into()).unwrap();
    }

    loop {
        let pool = match pool_listener.receive_pool_notification() {
            Ok(Some(pool)) => pool,
            Ok(None) => {
                thread::sleep(Duration::from_millis(300));
                continue;
            }
            Err(e) => match e {
                error::Error::Disconnected | error::Error::NotConnected => {
                    println!("pool_poller() connexion lost: {e:?}");
                    // connexion lost try to reconnect
                    pool_listener = NostrClient::new("pool_listener")
                        .relay(relay.clone())
                        .unwrap()
                        .keys(Keys::generate())
                        .unwrap();
                    if let Err(e) = pool_listener.connect_nostr() {
                        let msg: PoolPollerMsg = e.into();
                        sender.send(msg.into()).unwrap();
                    }
                    if let Err(e) = pool_listener.subscribe_pools(back) {
                        let msg: PoolPollerMsg = e.into();
                        sender.send(msg.into()).unwrap();
                    }
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                e => {
                    println!("pool_poller(): {:?}", e);
                    let msg: PoolPollerMsg = e.into();
                    sender.send(msg.into()).unwrap();
                    return;
                }
            },
        };
        {
            let mut store = pool_store.lock().expect("poisoned");
            store.update(pool, PoolStatus::Available);
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
