use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
    str::FromStr,
};

use joinstr::{
    bip39::Mnemonic,
    miniscript::{
        bitcoin::{self, ScriptBuf},
        Descriptor, DescriptorPublicKey,
    },
};
use serde::{Deserialize, Serialize};

use crate::cpp_joinstr::Network;

const CONFIG_FILENAME: &str = "config.json";

fn datadir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = {
        let mut dir = dirs::home_dir().unwrap();
        dir.push(".qoinstr");
        dir
    };

    #[cfg(not(target_os = "linux"))]
    let dir = {
        let mut dir = dirs::config_dir().unwrap();
        dir.push("Qoinstr");
        dir
    };

    maybe_create_dir(&dir);

    dir
}

fn maybe_create_dir(dir: &PathBuf) {
    if !dir.exists() {
        #[cfg(unix)]
        {
            use std::fs::DirBuilder;
            use std::os::unix::fs::DirBuilderExt;

            let mut builder = DirBuilder::new();
            builder.mode(0o700).recursive(true).create(dir).unwrap();
        }

        #[cfg(not(unix))]
        std::fs::create_dir_all(dir).unwrap();
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(skip)]
    pub account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrum_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub electrum_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nostr_relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nostr_back: Option<u64>,
    pub network: bitcoin::Network,
    pub look_ahead: u32,
    pub mnemonic: String,
    pub descriptor: Descriptor<DescriptorPublicKey>,
}

pub fn list_configs() -> Vec<String> {
    let path = datadir();
    let mut out = vec![];
    if let Ok(folders) = fs::read_dir(path) {
        folders.for_each(|account| {
            if let Ok(entry) = account {
                if let Ok(md) = entry.metadata() {
                    if md.is_dir() {
                        let acc_name = entry.file_name().to_str().unwrap().to_string();
                        let parsed = Config::from_file(acc_name.clone());
                        if !parsed.account.is_empty() {
                            out.push(acc_name);
                        }
                    };
                }
            }
        });
    }

    out
}

pub fn config_exists(account: String) -> bool {
    let mut path = Config::path(account.clone());
    path.push(CONFIG_FILENAME);
    path.exists()
}

impl Config {
    pub fn path(account: String) -> PathBuf {
        let mut dir = datadir();
        dir.push(account);
        dir
    }

    pub fn boxed(&self) -> Box<Self> {
        Box::new(self.clone())
    }

    pub fn from_file(account: String) -> Self {
        let mut path = Self::path(account.clone());
        path.push(CONFIG_FILENAME);

        let mut file = File::open(path).unwrap();
        let mut content = String::new();
        let _ = file.read_to_string(&mut content);
        let mut conf: Config = serde_json::from_str(&content).unwrap();
        let mnemo = Mnemonic::from_str(&conf.mnemonic);
        if mnemo.is_ok() {
            conf.account = account;
        }
        conf
    }

    pub fn transactions_path(&self) -> PathBuf {
        let mut path = Self::path(self.account.clone());
        path.push("transactions.json");
        path
    }

    pub fn statuses_path(&self) -> PathBuf {
        let mut path = Self::path(self.account.clone());
        path.push("statuses.json");
        path
    }

    pub fn tip_path(&self) -> PathBuf {
        let mut path = Self::path(self.account.clone());
        path.push("tip.json");
        path
    }

    pub fn persist_tip(&self, receive: u32, change: u32) {
        let file = File::create(self.tip_path());
        match file {
            Ok(mut file) => {
                let tip = Tip { receive, change };
                let content = serde_json::to_string_pretty(&tip).expect("cannot fail");
                let _ = file.write(content.as_bytes());
            }
            Err(e) => {
                log::error!("Config::persist_tip() fail to open file: {e}");
            }
        }
    }

    pub fn tip_from_file(&self) -> Tip {
        if let Ok(mut file) = File::open(self.tip_path()) {
            let mut content = String::new();
            let _ = file.read_to_string(&mut content);
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Default::default()
        }
    }

    // let mut statuses = BTreeMap::<ScriptBuf, (Option<String>, u32, u32)>::new();
    //
    pub fn persist_statuses(&self, statuses: &BTreeMap<ScriptBuf, (Option<String>, u32, u32)>) {
        let file = File::create(self.statuses_path());
        match file {
            Ok(mut file) => {
                let content = serde_json::to_string_pretty(statuses).expect("cannot fail");
                let _ = file.write(content.as_bytes());
            }
            Err(e) => {
                log::error!("Config::statuses() fail to open file: {e}");
            }
        }
    }

    pub fn statuses_from_file(&self) -> BTreeMap<ScriptBuf, (Option<String>, u32, u32)> {
        if let Ok(mut file) = File::open(self.statuses_path()) {
            let mut content = String::new();
            let _ = file.read_to_string(&mut content);
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Default::default()
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Tip {
    pub receive: u32,
    pub change: u32,
}

pub fn config_from_file(account: String) -> Box<Config> {
    Config::from_file(account).boxed()
}

pub fn is_descriptor_valid(descriptor: String) -> bool {
    Descriptor::<DescriptorPublicKey>::from_str(&descriptor).is_ok()
}

pub fn new_config(descriptor: String) -> Box<Config> {
    let descriptor = Descriptor::from_str(&descriptor).expect("must be checked");

    Config {
        account: String::new(),
        electrum_url: None,
        electrum_port: None,
        nostr_relay: None,
        nostr_back: None,
        network: bitcoin::Network::Bitcoin,
        look_ahead: 20,
        mnemonic: String::new(),
        descriptor,
    }
    .boxed()
}

// c++ interface
impl Config {
    pub fn electrum_url(&self) -> String {
        self.electrum_url.clone().unwrap_or_default()
    }
    pub fn electrum_port(&self) -> String {
        self.electrum_port
            .map(|v| format!("{v}"))
            .unwrap_or_default()
    }
    pub fn nostr_url(&self) -> String {
        self.nostr_relay.clone().unwrap_or_default()
    }
    pub fn nostr_back(&self) -> String {
        self.nostr_back.map(|v| format!("{v}")).unwrap_or_default()
    }
    pub fn look_ahead(&self) -> String {
        self.look_ahead.to_string()
    }
    pub fn network(&self) -> Network {
        self.network.into()
    }
    pub fn set_electrum_url(&mut self, url: String) {
        self.electrum_url = Some(url);
    }
    pub fn set_electrum_port(&mut self, port: String) {
        self.electrum_port = port.parse::<u16>().ok();
    }
    pub fn set_nostr_relay(&mut self, relay: String) {
        self.nostr_relay = Some(relay);
    }
    pub fn set_nostr_back(&mut self, back: String) {
        self.nostr_back = back.parse::<u64>().ok();
    }
    pub fn set_look_ahead(&mut self, look_ahead: String) {
        if let Ok(la) = look_ahead.parse::<u32>() {
            self.look_ahead = la;
        }
    }
    pub fn set_network(&mut self, network: Network) {
        self.network = network.into();
    }
    pub fn set_mnemonic(&mut self, mnemonic: String) {
        self.mnemonic = mnemonic;
    }
    pub fn set_account(&mut self, name: String) {
        self.account = name;
    }
    pub fn to_file(&self) {
        let mut path = Self::path(self.account.clone());
        maybe_create_dir(&path);
        path.push(CONFIG_FILENAME);

        log::warn!("Config::to_file() {:?}", path);

        let mut file = File::create(path).unwrap();
        let content = serde_json::to_string_pretty(&self).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }
}
