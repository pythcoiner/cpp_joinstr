pub mod account;
pub mod address_store;
pub mod coin_store;
pub mod config;
pub mod macros;
pub mod mnemonic;
pub mod pool;
pub mod pool_store;
pub mod tx_store;

use std::fmt::Display;

use account::{new_account, Account, Poll, Signal};
use address_store::AddressEntry;
use coin_store::CoinEntry;
pub use config::{config_from_file, Config};
use joinstr::miniscript::bitcoin;
pub use mnemonic::{mnemonic_from_string, Mnemonic};
pub use pool::Pool;

#[cxx::bridge]
pub mod cpp_joinstr {

    #[derive(Debug, Clone, Copy)]
    pub enum LogLevel {
        Off,
        Error,
        Warn,
        Info,
        Debug,
        Trace,
    }

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
        #[rust_name = Config]
        type RustConfig;
        fn electrum_url(&self) -> String;
        fn electrum_port(&self) -> String;
        fn nostr_url(&self) -> String;
        fn nostr_back(&self) -> String;
        fn set_electrum_url(&mut self, url: String);
        fn set_electrum_port(&mut self, port: String);
        fn set_nostr_relay(&mut self, relay: String);
        fn set_nostr_back(&mut self, back: String);
        fn to_file(&self);
        fn config_from_file(account: String) -> Box<Config>;
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
        fn set_electrum(&mut self, url: String, port: String);
        fn start_electrum(&mut self);
        fn stop_electrum(&mut self);
        fn set_nostr(&mut self, url: String, back: String);
        fn start_nostr(&mut self);
        fn stop_nostr(&mut self);
        fn set_look_ahead(&mut self, look_ahead: String);
        fn get_config(&self) -> Box<Config>;

        fn new_account(mnemonic: Box<Mnemonic>, account: String) -> Box<Account>;
    }

    extern "Rust" {
        fn init_rust_logger(level: LogLevel);
    }
}

use cpp_joinstr::{LogLevel, Network, SignalFlag};

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

impl From<LogLevel> for log::LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Off => Self::Off,
            LogLevel::Error => Self::Error,
            LogLevel::Warn => Self::Warn,
            LogLevel::Info => Self::Info,
            LogLevel::Debug => Self::Debug,
            LogLevel::Trace => Self::Trace,
            _ => unreachable!(),
        }
    }
}

impl From<log::LevelFilter> for LogLevel {
    fn from(value: log::LevelFilter) -> Self {
        match value {
            log::LevelFilter::Off => Self::Off,
            log::LevelFilter::Error => Self::Error,
            log::LevelFilter::Warn => Self::Warn,
            log::LevelFilter::Info => Self::Info,
            log::LevelFilter::Debug => Self::Debug,
            log::LevelFilter::Trace => Self::Trace,
        }
    }
}

pub fn init_rust_logger(level: LogLevel) {
    let level = level.into();
    env_logger::builder().filter_level(level).init();
    log::info!("init_rust_logger()");
}
