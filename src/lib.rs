pub mod account;
pub mod address_store;
pub mod coin_store;
pub mod macros;
pub mod mnemonic;
pub mod pool;
pub mod pool_store;
pub mod tx_store;

use std::fmt::Display;

use account::{new_wallet, Account, Poll, Signal};
use address_store::AddressEntry;
use coin_store::CoinEntry;
use joinstr::miniscript::bitcoin;
pub use mnemonic::{mnemonic_from_string, Mnemonic};
pub use pool::Pool;

#[cxx::bridge]
pub mod cpp_joinstr {

    #[derive(Debug, Clone)]
    pub enum SignalFlag {
        TxListenerStarted,
        TxListenerStopped,
        TxListenerError,
        PoolListenerStarted,
        PoolUpdate,
        PoolListenerStopped,
        PoolListenerError,
        AddressTipChanged,
        CoinUpdate,
        AccountError,
    }

    extern "Rust" {
        fn signal_flag_to_string(signal: SignalFlag) -> String;
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
        Spent,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum PoolStatus {
        Available,
        Processing,
        Joined,
        Closed,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
    pub enum AddrAccount {
        Receive,
        Change,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum AddressStatus {
        NotUsed,
        Used,
        Reused,
    }

    extern "Rust" {
        #[rust_name = CoinEntry]
        type RustCoin;
        fn amount_sat(&self) -> u64;
        fn amount_btc(&self) -> f64;
        #[cxx_name = outpoint]
        fn outpoint_str(&self) -> String;
        #[cxx_name = status]
        fn status_str(&self) -> String;
        fn boxed(&self) -> Box<CoinEntry>;
        fn address(&self) -> String;
    }

    extern "Rust" {
        type Poll;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn boxed(&self) -> Box<Signal>;
    }

    extern "Rust" {
        type Signal;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn unwrap(&self) -> SignalFlag;
    }

    extern "Rust" {
        type Mnemonic;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn mnemonic_from_string(value: String) -> Box<Mnemonic>;
    }

    extern "Rust" {
        #[rust_name = Coins]
        type RustCoins;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn count(&self) -> usize;
        fn is_empty(&self) -> bool;
        fn get(&self, index: usize) -> Box<CoinEntry>;
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
        #[rust_name = AddressEntry]
        type RustAddress;
        fn status(&self) -> AddressStatus;
        fn value(&self) -> String;
        fn account(&self) -> AddrAccount;
        fn index(&self) -> u32;
    }

    extern "Rust" {
        #[rust_name = Addresses]
        type AddressList;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn count(&self) -> usize;
        fn is_empty(&self) -> bool;
        fn get(&self, index: usize) -> Box<AddressEntry>;
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
        fn fees(&self) -> u32;
        fn id(&self) -> String;
        fn timeout(&self) -> u64;
    }

    extern "Rust" {
        type Account;
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
        fn relay(&self) -> String;
        fn new_addr(&mut self) -> Box<AddressEntry>;

        fn new_wallet(
            mnemonic: Box<Mnemonic>,
            network: Network,
            addr: String,
            port: u16,
            relay: String,
            back: u64,
        ) -> Box<Account>;
    }
}

use cpp_joinstr::{Network, SignalFlag};

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

result!(Coins, Vec<Box<CoinEntry>>);
impl Coins {
    pub fn count(&self) -> usize {
        self.inner.as_ref().map(|v| v.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.count() != 0
    }

    pub fn get(&self, index: usize) -> Box<CoinEntry> {
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

result!(Addresses, Vec<Box<AddressEntry>>);
impl Addresses {
    pub fn count(&self) -> usize {
        self.inner.as_ref().map(|v| v.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.count() != 0
    }

    pub fn get(&self, index: usize) -> Box<AddressEntry> {
        self.inner.as_ref().unwrap().get(index).unwrap().clone()
    }
}

result!(Txid, String);
impl Txid {
    pub fn value(&self) -> String {
        self.unwrap()
    }
}

impl Display for SignalFlag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            SignalFlag::TxListenerStarted => write!(f, "TxListenerStarted"),
            SignalFlag::TxListenerStopped => write!(f, "TxListenerStopped"),
            SignalFlag::TxListenerError => write!(f, "TxListenerError"),
            SignalFlag::PoolListenerStarted => write!(f, "PoolListenerStarted"),
            SignalFlag::PoolListenerStopped => write!(f, "PoolListenerStopped"),
            SignalFlag::PoolListenerError => write!(f, "PoolListenerError"),
            SignalFlag::AddressTipChanged => write!(f, "AddressTipChanged"),
            SignalFlag::CoinUpdate => write!(f, "CoinUpdate"),
            SignalFlag::AccountError => write!(f, "AccountError"),
            SignalFlag::PoolUpdate => write!(f, "PoolUpdate"),
            _ => write!(f, "unexpected SignalFlag"),
        }
    }
}

pub fn signal_flag_to_string(signal: SignalFlag) -> String {
    signal.to_string()
}
