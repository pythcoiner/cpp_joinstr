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
pub use config::{
    config_exists, config_from_file, is_descriptor_valid, list_configs, new_config, Config,
};
use joinstr::miniscript::bitcoin;
pub use mnemonic::{generate_mnemonic, mnemonic_from_string, Mnemonic};

#[cxx::bridge]
pub mod cpp_joinstr {

    pub struct TransactionTemplate {
        inputs: Vec<RustCoin>,
        outputs: Vec<Output>,
        fee: u64, // fee in sats (NOT sats/vb)
    }

    pub struct Output {
        address: String,
        amount: u64, // amount in sats
        label: String,
        max: bool, // if max == true, amount is not taken in account,
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
        Error,
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
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
        Error,
    }

    extern "Rust" {
        fn pool_status_to_string(status: PoolStatus) -> String;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
    pub enum AddrAccount {
        Receive,
        Change,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
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
        fn new_config(mnemonic: String, account: String, network: Network) -> Box<Config>;
    }

    extern "Rust" {
        type Poll;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn error(&self) -> String;
        fn signal(&self) -> Box<Signal>;
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

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub struct CoinState {
        coins: Vec<RustCoin>,
        confirmed_coins: usize,
        confirmed_balance: u64,
        unconfirmed_coins: usize,
        unconfirmed_balance: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub struct RustCoin {
        value: u64,
        height: u64,
        confirmed: bool,
        status: CoinStatus,
        outpoint: String,
        address: RustAddress,
        label: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub struct RustAddress {
        address: String,
        status: AddressStatus,
        account: AddrAccount,
        index: u32,
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
        total_peers: usize,
        current_peers: usize,
        relay: String,
        fees: u32,
        id: String,
        status: PoolStatus,
        timeout: u64,
    }

    extern "Rust" {
        type PoolsResult;
        fn is_ok(&self) -> bool;
        fn is_err(&self) -> bool;
        fn value(&self) -> Vec<RustPool>;
        fn error(&self) -> String;
        fn relay(&self) -> String;
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
        fn spendable_coins(&self) -> CoinState;
        fn generate_coins(&mut self);
        fn edit_coin_label(&self, outpoint: String, label: String);
        fn recv_addr_at(&self, index: u32) -> String;
        fn change_addr_at(&self, index: u32) -> String;
        fn prepare_transaction(&mut self, tx_template: TransactionTemplate) -> Box<PsbtResult>;
        fn pools(&self) -> Box<PoolsResult>;
        fn create_pool(
            &mut self,
            outpoint: String,
            denomination: u64,
            fee: u32,
            max_duration: u64,
            peers: usize,
        );
        fn join_pool(&mut self, outpoint: String, pool_id: String);
        fn pool(&mut self, pool_id: String) -> Box<RustPool>;
        fn try_recv(&mut self) -> Box<Poll>;
        fn relay(&self) -> String;
        fn new_addr(&mut self) -> RustAddress;
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

use cpp_joinstr::{AddrAccount, LogLevel, Network, PoolStatus, RustPool, SignalFlag};

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

result!(AddressesResult, Vec<AddressEntry>);

result!(Txid, String);

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

#[derive(Debug, Clone)]
pub struct PoolsResult {
    relay: String,
    value: Option<Vec<RustPool>>,
    error: Option<String>,
}

impl PoolsResult {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            value: None,
            error: None,
            relay: String::new(),
        }
    }
    pub fn ok(value: Vec<RustPool>) -> Self {
        Self {
            value: Some(value),
            error: None,
            relay: String::new(),
        }
    }
    pub fn err(error: &str) -> Self {
        Self {
            value: None,
            error: Some(error.into()),
            relay: String::new(),
        }
    }
    pub fn is_ok(&self) -> bool {
        self.value.is_some() && self.error.is_none()
    }
    pub fn is_err(&self) -> bool {
        self.value.is_none() && self.error.is_some()
    }
    pub fn value(&self) -> Vec<RustPool> {
        self.value.clone().unwrap()
    }
    pub fn error(&self) -> String {
        self.error.clone().unwrap()
    }
    pub fn boxed(&self) -> Box<Self> {
        Box::new(self.clone())
    }
    pub fn relay(&self) -> String {
        self.relay.clone()
    }
}
impl From<&str> for Box<PoolsResult> {
    fn from(value: &str) -> Box<PoolsResult> {
        Box::new(PoolsResult {
            value: None,
            error: Some(value.into()),
            relay: String::new(),
        })
    }
}

impl Display for PoolStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

pub fn pool_status_to_string(status: PoolStatus) -> String {
    status.to_string()
}
