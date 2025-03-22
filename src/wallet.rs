use std::{
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use joinstr::{
    bip39,
    miniscript::bitcoin::{self, Sequence},
    nostr::{self, error, sync::NostrClient},
    signer::{self, CoinPath, WpkhHotSigner},
    simple_nostr_client::nostr::key::Keys,
};

use crate::{
    address_store::AddressStore,
    coin_store::CoinStore,
    cpp_joinstr::{CoinStatus, Network, PoolStatus, SignalFlag},
    pool_store::PoolStore,
    result, Coins, Mnemonic, Pool, Pools,
};

result!(Poll, Signal);

result!(Signal, SignalFlag);

#[derive(Debug)]
pub enum Notification {
    Electrum(CoinPollerMsg),
    Joinstr(PoolPollerMsg),
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
    WatchSpk {
        account: u32,
        index: u32,
    },
    CoinUpdate {
        coin: signer::Coin,
        status: CoinStatus,
    },
    Started(mpsc::Sender<PoolPollerMsg>),
    Stop,
    Error(joinstr::electrum::Error),
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
    nostr_addr: String,
    network: bitcoin::Network,
}

// Rust only interface
impl Wallet {
    fn new(
        mnemonic: bip39::Mnemonic,
        network: bitcoin::Network,
        addr: String,
        port: u16,
        relay: String,
        back: u64,
    ) -> Self {
        let (signal_sender, signals) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        // TODO: import saved state from local storage
        let signer = WpkhHotSigner::new_from_mnemonics(network, &mnemonic.to_string())
            .expect("valid mnemonic");
        let address_store = Arc::new(Mutex::new(AddressStore::new(signer.clone(), 20, 20)));
        let coin_store = Arc::new(Mutex::new(CoinStore::new(
            signer.clone(),
            address_store.clone(),
        )));
        // TODO: use indexes from stored state
        let mut wallet = Wallet {
            coin_store,
            pool_store: Default::default(),
            address_store,
            signals,
            signal_sender,
            signer,
            coin_poller: None,
            pool_poller: None,
            electrum_url: addr,
            electrum_port: port,
            nostr_addr: relay,
            network,
            receiver,
            sender,
            electrum_channel: None,
            pool_listener_channel: None,
        };
        wallet.start_poll_coins();
        wallet.start_poll_pools(back);
        wallet
    }

    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    fn start_poll_coins(&mut self) {
        println!("Wallet::start_poll_coins()");
        let coin_store = self.coin_store.clone();
        let sender = self.sender.clone();
        let signer = self.signer.clone();
        let addr = self.electrum_url.clone();
        let port = self.electrum_port;
        let poller = thread::spawn(move || {
            coin_poller(addr, port, coin_store, signer, sender);
        });
        self.coin_poller = Some(poller);
    }

    fn start_poll_pools(&mut self, back: u64) {
        println!("Wallet::start_poll_pools()");
        let relay = self.nostr_addr.clone();
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
        let relay = self.nostr_addr.clone();
        let network = self.network;
        thread::spawn(move || {
            dummy_pool(relay, denomination, peers, timeout, fee, network);
        });
    }

    pub fn relay(&self) -> String {
        self.nostr_addr.clone()
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

fn listener<T: From<CoinPollerMsg>>(
    addr: String,
    port: u16,
    coin_store: Arc<Mutex<CoinStore>>,
    signer: WpkhHotSigner,
    sender: mpsc::Sender<T>,
) {
    let (channel, receiver) = mpsc::channel();

    let mut client = match joinstr::electrum::Client::new(&addr, port) {
        Ok(c) => c,
        Err(e) => {
            println!("coin_poll(): fail to create electrum client {}", e);
            sender.send(CoinPollerMsg::Error(e).into()).unwrap();
            return;
        }
    };

    sender
        .send(CoinPollerMsg::Started(channel).into())
        .expect("wallet thread stopped");
}

fn coin_poller<T: From<CoinPollerMsg>>(
    addr: String,
    port: u16,
    coin_store: Arc<Mutex<CoinStore>>,
    signer: WpkhHotSigner,
    sender: mpsc::Sender<T>,
) {
    let mut client = match joinstr::electrum::Client::new(&addr, port) {
        Ok(c) => c,
        Err(e) => {
            println!("wallet_poll(): fail to create electrum client {}", e);
            sender.send(CoinPollerMsg::Error(e).into()).unwrap();
            return;
        }
    };

    let mut watched_coins = Vec::new();
    // watch 30 coins
    for i in 0..30 {
        let recv = CoinPath {
            index: Some(i),
            depth: 0,
        };
        let change = CoinPath {
            index: Some(i),
            depth: 0,
        };
        watched_coins.push((signer.address_at(&recv).unwrap().script_pubkey(), recv));
        watched_coins.push((signer.address_at(&change).unwrap().script_pubkey(), change));
    }

    loop {
        let mut coin_buff = Vec::new();
        for (script, coin_path) in &watched_coins {
            match client.get_coins_at(script) {
                Ok((coins, _txs)) => {
                    let mut coins: Vec<_> = coins
                        .into_iter()
                        .map(|(txout, op)| {
                            let sequence = Sequence(0);
                            signer::Coin {
                                txout,
                                outpoint: op,
                                sequence,
                                coin_path: *coin_path,
                            }
                        })
                        .collect();
                    coin_buff.append(&mut coins);
                }
                Err(e) => {
                    println!("wallet_poll(): fail to get_coins_at() {}", e);
                    sender.send(CoinPollerMsg::Error(e).into()).unwrap();
                }
            }
        }
        {
            let mut store = coin_store.lock().expect("poisoned");
            for coin in coin_buff {
                store.update(coin, CoinStatus::Confirmed);
            }
        } // release store lock
        thread::sleep(Duration::from_secs(5));
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
