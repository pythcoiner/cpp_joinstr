use std::{
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

use joinstr::{
    bip39,
    miniscript::bitcoin::{self, Sequence},
    signer::{self, CoinPath, WpkhHotSigner},
};

use crate::{
    coin_store::CoinStore,
    cpp_joinstr::{CoinStatus, Network, SignalFlag},
    result, Coins, Mnemonic,
};

result!(Poll, Signal);

result!(Signal, SignalFlag);

#[derive(Debug)]
pub struct Wallet {
    coin_store: Arc<Mutex<CoinStore>>,
    signals: mpsc::Receiver<Signal>,
    sender: mpsc::Sender<Signal>,
    signer: WpkhHotSigner,
    poller: Option<JoinHandle<()>>,
}

// Rust only interface
impl Wallet {
    fn new(mnemonic: bip39::Mnemonic, network: bitcoin::Network, addr: String, port: u16) -> Self {
        let (sender, signals) = mpsc::channel();
        let mut wallet = Wallet {
            coin_store: Default::default(),
            signals,
            sender,
            signer: WpkhHotSigner::new_from_mnemonics(network, &mnemonic.to_string())
                .expect("valid mnemonic"),
            poller: None,
        };
        wallet.start_poll(addr, port);
        wallet
    }

    fn boxed(self) -> Box<Self> {
        Box::new(self)
    }

    fn start_poll(&mut self, addr: String, port: u16) {
        println!("Wallet::start_poll()");
        let coin_store = self.coin_store.clone();
        let sender = self.sender.clone();
        let signer = self.signer.clone();
        let poller = thread::spawn(move || {
            wallet_poll(addr, port, coin_store, signer, sender);
        });
        self.poller = Some(poller);
    }
}

// C++ shared interface
impl Wallet {
    pub fn spendable_coins(&self) -> Box<Coins> {
        match self.coin_store.try_lock() {
            Ok(lock) => Box::new(lock.spendable_coins()),
            Err(_) => {
                let mut coins = Coins::new();
                coins.set_error("Locked".to_string());
                Box::new(coins)
            }
        }
    }

    pub fn poll(&mut self) -> Box<Poll> {
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
}

pub fn new_wallet(
    #[allow(clippy::boxed_local)] mnemonic: Box<Mnemonic>,
    network: Network,
    addr: String,
    port: u16,
) -> Box<Wallet> {
    Wallet::new((*mnemonic).into(), network.into(), addr, port).boxed()
}

fn wallet_poll(
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
