use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

fn datadir() -> PathBuf {
    #[cfg(target_os = "linux")]
    let dir = {
        let mut dir = dirs::home_dir().unwrap();
        dir.push(".escrow");
        dir
    };

    #[cfg(not(target_os = "linux"))]
    let dir = {
        let mut dir = dirs::config_dir().unwrap();
        dir.push("Escrow");
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
    account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    electrum_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    electrum_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nostr_relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nostr_back: Option<u64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            account: "main".to_string(),
            electrum_url: None,
            electrum_port: None,
            nostr_relay: None,
            nostr_back: None,
        }
    }
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
        let mut path = Self::path(account);
        path.push("config.json");

        let mut file = File::open(path).unwrap();
        let mut content = String::new();
        let _ = file.read_to_string(&mut content);
        serde_json::from_str(&content).ok().unwrap_or_default()
    }
}

pub fn config_from_file(account: String) -> Box<Config> {
    Config::from_file(account).boxed()
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
    pub fn set_electrum_url(&mut self, url: String) {
        self.electrum_url = Some(url);
    }
    pub fn set_electrum_port(&mut self, port: u16) {
        self.electrum_port = Some(port);
    }
    pub fn set_nostr_relay(&mut self, relay: String) {
        self.nostr_relay = Some(relay);
    }
    pub fn set_nostr_back(&mut self, back: u64) {
        self.nostr_back = Some(back);
    }
    pub fn to_file(&self) {
        let mut path = Self::path(self.account.clone());
        maybe_create_dir(&path);
        path.push("config.json");

        let mut file = File::create(path).unwrap();
        let content = serde_json::to_string_pretty(&self).unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }
}
