pub mod address;
pub mod coin;
pub mod coin_store;
pub mod macros;
pub mod mnemonic;
pub mod pool;
pub mod pool_store;
pub mod wallet;

pub use address::{address_from_string, Address};
pub use coin::Coin;
use joinstr::miniscript::bitcoin;
pub use mnemonic::{mnemonic_from_string, Mnemonic};
pub use pool::Pool;
use wallet::{new_wallet, Poll, Signal, Wallet};

#[cxx::bridge]
pub mod cpp_joinstr {

    #[derive(Debug, Clone)]
    pub enum SignalFlag {
        UpdateCoins,
        UpdateWallet,
        Error,
    }

    pub enum Network {
        Regtest,
        Signet,
        Testnet,
        Bitcoin,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum CoinStatus {
        Unconfirmed,
        Confirmed,
        BeingSpend,
        Spend,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum PoolStatus {
        Available,
        Processing,
        Joined,
        Closed,
    }

    extern "Rust" {
        #[rust_name = Coin]
        type RustCoin;
        fn amount_sat(&self) -> u64;
        fn amount_btc(&self) -> f64;
        fn outpoint(&self) -> String;
    }

    extern "Rust" {
        type Poll;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
    }

    extern "Rust" {
        type Signal;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
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
        #[rust_name = Coins]
        type RustCoins;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn count(&self) -> usize;
        fn is_empty(&self) -> bool;
        fn get(&self, index: usize) -> Box<Coin>;
    }

    extern "Rust" {
        #[rust_name = Pools]
        type RustPools;
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
        #[rust_name = Pool]
        type RustPool;
        fn denomination_sat(&self) -> u64;
        fn denomination_btc(&self) -> f64;
        fn peers(&self) -> usize;
        fn relay(&self) -> String;
        fn fee(&self) -> u32;
    }

    extern "Rust" {
        type Wallet;
        fn spendable_coins(&self) -> Box<Coins>;
        fn recv_addr_at(&self, index: u32) -> String;
        fn change_addr_at(&self, index: u32) -> String;
        fn pools(&self) -> Box<Pools>;
        fn create_pool(
            &mut self,
            outpoint: String,
            denomination: f64,
            fee: u32,
            max_duration: u64,
            peers: usize,
        );
        fn join_pool(&mut self, outpoint: String, pool_id: String);
        fn pool(&mut self, pool_id: String) -> Box<Pool>;
        fn create_dummy_pool(&self, denomination: u64, peers: usize, timeout: u64, fee: u32);
        fn try_recv(&mut self) -> Box<Poll>;

        fn new_wallet(
            mnemonic: Box<Mnemonic>,
            network: Network,
            addr: String,
            port: u16,
            relay: String,
            back: u64,
        ) -> Box<Wallet>;
    }
}

use cpp_joinstr::Network;

impl Network {
    pub fn boxed(&self) -> Box<Network> {
        Box::new(*self)
    }
}

impl From<Network> for bitcoin::Network {
    fn from(value: Network) -> Self {
        match value {
            Network::Signet => bitcoin::Network::Signet,
            Network::Regtest => bitcoin::Network::Regtest,
            Network::Testnet => bitcoin::Network::Testnet,
            Network::Bitcoin => bitcoin::Network::Bitcoin,
            _ => unreachable!(),
        }
    }
}

impl From<bitcoin::Network> for Network {
    fn from(value: bitcoin::Network) -> Self {
        match value {
            bitcoin::Network::Signet => Network::Signet,
            bitcoin::Network::Regtest => Network::Regtest,
            bitcoin::Network::Testnet => Network::Testnet,
            bitcoin::Network::Bitcoin => Network::Bitcoin,
            _ => unreachable!(),
        }
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

result!(Txid, String);

impl Txid {
    pub fn value(&self) -> String {
        self.unwrap()
    }
}
