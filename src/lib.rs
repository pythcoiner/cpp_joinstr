pub mod account;
pub mod address_store;
pub mod coin;
pub mod coin_store;
pub mod config;
pub mod derivator;
pub mod label_store;
pub mod macros;
pub mod mnemonic;
pub mod pool_store;
pub mod signer;
pub mod signing_manager;
#[cfg(test)]
pub mod test_utils;
pub mod tx_store;

use std::fmt::Display;

use account::{new_account, Account, Poll, Signal};
use address_store::AddressEntry;
use coin_store::CoinEntry;
pub use config::{
    config_exists, config_from_file, is_descriptor_valid, list_configs, new_config, Config,
};
use joinstr::miniscript::bitcoin;
pub use mnemonic::{generate_mnemonic, mnemonic_from_string, Mnemonic};

#[cxx::bridge]
pub mod cpp_joinstr {

    pub struct TransactionTemplate {
        inputs: Vec<String /* outpoint */>,
        outputs: Vec<Output>,
    }

    pub struct Output {
        address: String,
        amount: u64,
    }

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
        Stopped,
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
        Unknown,
    }

    extern "Rust" {
        #[rust_name = Config]
        type RustConfig;
        fn electrum_url(&self) -> String;
        fn electrum_port(&self) -> String;
        fn nostr_url(&self) -> String;
        fn nostr_back(&self) -> String;
        fn look_ahead(&self) -> String;
        fn network(&self) -> Network;
        fn set_electrum_url(&mut self, url: String);
        fn set_electrum_port(&mut self, port: String);
        fn set_nostr_relay(&mut self, relay: String);
        fn set_nostr_back(&mut self, back: String);
        fn set_look_ahead(&mut self, look_ahead: String);
        fn set_network(&mut self, network: Network);
        fn set_mnemonic(&mut self, mnemonic: String);
        fn to_file(&self);
        fn config_from_file(account: String) -> Box<Config>;
        fn config_exists(account: String) -> bool;
        fn set_account(&mut self, name: String);
        fn is_descriptor_valid(descriptor: String) -> bool;
        fn new_config(descriptor: String) -> Box<Config>;
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
        fn rust_address(&self) -> Box<AddressEntry>;
        fn label(&self) -> String;
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
        fn generate_mnemonic() -> String;
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
        #[rust_name = AddressEntry]
        type RustAddress;
        fn status(&self) -> AddressStatus;
        fn value(&self) -> String;
        fn account(&self) -> AddrAccount;
        fn index(&self) -> u32;
        #[rust_name = clone_boxed]
        fn clone(&self) -> Box<AddressEntry>;
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

    #[derive(Debug, Clone)]
    pub struct RustPool {
        denomination: u64,
        peers: usize,
        relay: String,
        fees: u32,
        id: String,
        timeout: u64,
    }

    extern "Rust" {
        type PoolsResult;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn value(&self) -> Vec<RustPool>;
        fn error(&self) -> String;
    }

    extern "Rust" {
        type PsbtResult;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn value(&self) -> String;
        fn error(&self) -> String;
    }

    extern "Rust" {
        type Account;
        fn spendable_coins(&self) -> Box<Coins>;
        fn generate_coins(&mut self);
        fn edit_coin_label(&self, outpoint: String, label: String);
        fn recv_addr_at(&self, index: u32) -> String;
        fn change_addr_at(&self, index: u32) -> String;
        fn prepare_transaction(&mut self, tx_template: TransactionTemplate) -> Box<PsbtResult>;
        fn pools(&self) -> Box<PoolsResult>;
        fn create_pool(
            &mut self,
            outpoint: String,
            denomination: f64,
            fee: u32,
            max_duration: u64,
            peers: usize,
        );
        fn join_pool(&mut self, outpoint: String, pool_id: String);
        fn pool(&mut self, pool_id: String) -> Box<RustPool>;
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
        fn new_account(account: String) -> Box<Account>;
        fn stop(&mut self);
    }

    extern "Rust" {
        fn init_rust_logger(level: LogLevel);
    }

    extern "Rust" {
        fn list_configs() -> Vec<String>;
    }
}

use cpp_joinstr::{AddrAccount, LogLevel, Network, RustPool, SignalFlag};

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

results!(Coins, Vec<Box<CoinEntry>>);
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

results!(Addresses, Vec<Box<AddressEntry>>);
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

results!(Txid, String);
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

impl From<AddrAccount> for u32 {
    fn from(value: AddrAccount) -> Self {
        match value {
            AddrAccount::Receive => 0,
            AddrAccount::Change => 2,
            _ => panic!(),
        }
    }
}

impl From<u32> for AddrAccount {
    fn from(value: u32) -> Self {
        match value {
            0 => AddrAccount::Receive,
            1 => AddrAccount::Change,
            _ => unimplemented!(),
        }
    }
}

result!(PsbtResult, String);

result!(PoolsResult, Vec<RustPool>);
