pub mod address;
pub mod coin;
pub mod macros;
pub mod mnemonic;
pub mod peer_config;
pub mod pool;
pub mod pool_config;

pub use address::{address_from_string, Address};
pub use coin::Coin;
use joinstr::{bip39, interface};
pub use mnemonic::{mnemonic_from_string, Mnemonic};
pub use pool::Pool;

#[cxx::bridge]
pub mod qt_joinstr {

    pub enum Network {
        Regtest,
        Signet,
        Testnet,
        Bitcoin,
    }

    extern "Rust" {
        type Coin;
        fn amount_sat(&self) -> u64;
        fn amount_btc(&self) -> f64;
        fn outpoint(&self) -> String;
    }

    extern "Rust" {
        type Mnemonic;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn mnemonic_from_string(value: String) -> Box<Mnemonic>;
    }

    extern "Rust" {
        type Address;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn address_from_string(value: String) -> Box<Address>;
    }

    extern "Rust" {
        type Coins;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn count(&self) -> usize;
        fn is_empty(&self) -> bool;
        fn get(&self, index: usize) -> Box<Coin>;
    }

    extern "Rust" {
        type Pools;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn count(&self) -> usize;
        fn is_empty(&self) -> bool;
        fn get(&self, index: usize) -> Box<Pool>;
    }

    extern "Rust" {
        type Txid;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn value(&self) -> String;
    }

    extern "Rust" {
        type Pool;
        fn denomination_sat(&self) -> u64;
        fn denomination_btc(&self) -> f64;
        fn peers(&self) -> usize;
        fn relay(&self) -> String;
        fn fee(&self) -> u32;
    }

    pub struct PeerConfig {
        pub mnemonics: Box<Mnemonic>,
        pub electrum_url: String,
        pub electrum_port: u16,
        pub input: Box<Coin>,
        pub output: Box<Address>,
        pub relay: String,
    }

    pub struct PoolConfig {
        pub denomination: f64,
        pub fee: u32,
        pub max_duration: u64,
        pub peers: usize,
        pub network: Box<Network>,
    }

    extern "Rust" {

        fn list_coins(
            mnemonics: Box<Mnemonic>,
            electrum_url: String,
            electrum_port: u16,
            range_start: u32,
            range_end: u32,
            network: Box<Network>,
        ) -> Box<Coins>;

        fn list_pools(back: u64, timeout: u64, relay: String) -> Box<Pools>;

        fn initiate_coinjoin(config: PoolConfig, peer: PeerConfig) -> Box<Txid>;

        fn join_coinjoin(pool: Box<Pool>, peer: PeerConfig) -> Box<Txid>;

    }
}

use qt_joinstr::{Network, PeerConfig, PoolConfig};

impl Network {
    pub fn boxed(&self) -> Box<Network> {
        Box::new(*self)
    }
}

result!(Coins, Vec<Box<Coin>>);

impl Coins {
    pub fn count(&self) -> usize {
        self.inner.as_ref().map(|v| v.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.count() != 0
    }

    pub fn get(&self, index: usize) -> Box<Coin> {
        self.inner.as_ref().unwrap().get(index).unwrap().clone()
    }
}

result!(Pools, Vec<Box<Pool>>);

impl Pools {
    pub fn count(&self) -> usize {
        self.inner.as_ref().map(|v| v.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.count() != 0
    }

    pub fn get(&self, index: usize) -> Box<Pool> {
        self.inner.as_ref().unwrap().get(index).unwrap().clone()
    }
}

pub fn list_coins(
    #[allow(clippy::boxed_local)] mnemonics: Box<Mnemonic>,
    electrum_url: String,
    electrum_port: u16,
    range_start: u32,
    range_end: u32,
    #[allow(clippy::boxed_local)] network: Box<Network>,
) -> Box<Coins> {
    let range = (range_start, range_end);
    let mut res = Coins::new();

    let mnemonics: bip39::Mnemonic = (*mnemonics).into();
    let mnemonics = mnemonics.to_string();

    match interface::list_coins(
        mnemonics,
        electrum_url,
        electrum_port,
        range,
        (*network).into(),
    ) {
        Ok(r) => {
            let coins = r.into_iter().map(|c| Box::new(c.into())).collect();
            res.set(coins);
        }
        Err(e) => res.set_error(e.to_string()),
    }

    Box::new(res)
}

pub fn list_pools(back: u64, timeout: u64, relay: String) -> Box<Pools> {
    let mut res = Pools::new();

    match interface::list_pools(back, timeout, relay) {
        Ok(pools) => {
            let pools = pools.into_iter().map(|p| Box::new(p.into())).collect();
            res.set(pools);
        }
        Err(e) => res.set_error(format!("{e}")),
    }

    Box::new(res)
}

result!(Txid, String);

impl Txid {
    pub fn value(&self) -> String {
        self.unwrap()
    }
}

pub fn initiate_coinjoin(config: PoolConfig, peer: PeerConfig) -> Box<Txid> {
    let mut res = Txid::new();
    match interface::initiate_coinjoin(config.into(), peer.into()) {
        Ok(txid) => res.set(txid.to_string()),
        Err(e) => res.set_error(format!("{e}")),
    }

    Box::new(res)
}

pub fn join_coinjoin(pool: Box<Pool>, peer: PeerConfig) -> Box<Txid> {
    let mut res = Txid::new();
    match interface::join_coinjoin((*pool).into(), peer.into()) {
        Ok(txid) => res.set(txid.to_string()),
        Err(e) => res.set_error(format!("{e}")),
    }

    Box::new(res)
}
