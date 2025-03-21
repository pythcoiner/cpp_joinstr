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
    coin_store::CoinStore,
    cpp_joinstr::{CoinStatus, Network, PoolStatus, SignalFlag},
    pool_store::PoolStore,
    result, Coins, Mnemonic, Pool, Pools,
};

result!(Poll, Signal);

result!(Signal, SignalFlag);

#[derive(Debug)]
pub struct Wallet {
    coin_store: Arc<Mutex<CoinStore>>,
    pool_store: Arc<Mutex<PoolStore>>,
    signals: mpsc::Receiver<Signal>,
    sender: mpsc::Sender<Signal>,
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
        let (sender, signals) = mpsc::channel();
        let mut wallet = Wallet {
            coin_store: Default::default(),
            pool_store: Default::default(),
            signals,
            sender,
            signer: WpkhHotSigner::new_from_mnemonics(network, &mnemonic.to_string())
                .expect("valid mnemonic"),
            coin_poller: None,
            pool_poller: None,
            electrum_url: addr,
            electrum_port: port,
            nostr_addr: relay,
            network,
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
        self.signer
            .recv_addr_at(index)
            .expect("valid path")
            .to_string()
    }

    pub fn change_addr_at(&self, index: u32) -> String {
        self.signer
            .change_addr_at(index)
            .expect("valid path")
            .to_string()
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

fn coin_poller(
    addr: String,
    port: u16,
    coin_store: Arc<Mutex<CoinStore>>,
    signer: WpkhHotSigner,
    sender: mpsc::Sender<Signal>,
) {
    let mut client = match joinstr::electrum::Client::new(&addr, port) {
        Ok(c) => c,
        Err(e) => {
            let mut signal = Signal::new();
            signal.set_error(e.to_string());
            println!("wallet_poll(): fail to create electrum client {}", e);
            sender.send(signal).unwrap();
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

fn pool_poller(
    relay: String,
    pool_store: Arc<Mutex<PoolStore>>,
    sender: mpsc::Sender<Signal>,
    back: u64,
) {
    let mut pool_listener = NostrClient::new("pool_listener")
        .relay(relay.clone())
        .unwrap()
        .keys(Keys::generate())
        .unwrap();
    if let Err(e) = pool_listener.connect_nostr() {
        let error = format!("pool_poller() fail to connect: {e:?}");
        let mut signal = Signal::new();
        signal.set_error(error);
        sender.send(signal).unwrap();
    }
    if let Err(e) = pool_listener.subscribe_pools(back) {
        let error = format!("pool_poller() fail to subscribe pool: {e:?}");
        let mut signal = Signal::new();
        signal.set_error(error);
        sender.send(signal).unwrap();
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
                        let error = format!("pool_poller() fail to re-connect: {e:?}");
                        let mut signal = Signal::new();
                        signal.set_error(error);
                        sender.send(signal).unwrap();
                    }
                    if let Err(e) = pool_listener.subscribe_pools(back) {
                        let error = format!("pool_poller() fail to re-subscribe: {e:?}");
                        let mut signal = Signal::new();
                        signal.set_error(error);
                        sender.send(signal).unwrap();
                    }
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                e => {
                    println!("pool_poller(): {:?}", e);
                    let mut signal = Signal::new();
                    signal.set_error(format!("pool_poller: {:?}", e));
                    sender.send(signal).unwrap();
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
