use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::mpsc::{self},
};

use joinstr::{
    bip39::{self},
    miniscript::bitcoin::{self, bip32},
};

use crate::{
    config,
    cpp_joinstr::Network,
    signer::{HotSigner, JsonSigner, Signer, SignerNotif},
};

#[derive(Debug)]
pub struct SigningManager {
    receiver: mpsc::Receiver<SignerNotif>,
    sender: mpsc::Sender<SignerNotif>,
    hot_signers: BTreeMap<bip32::Fingerprint, HotSigner>,
    #[allow(unused)]
    signers: BTreeMap<bip32::Fingerprint, ()>,
}

impl Default for SigningManager {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            receiver,
            sender,
            hot_signers: Default::default(),
            signers: Default::default(),
        }
    }
}

impl SigningManager {
    pub fn path() -> PathBuf {
        let mut path = config::datadir();
        path.push(".signers");
        path
    }
    pub fn from_file() -> Self {
        if let Ok(mut file) = File::open(Self::path()) {
            let mut content = String::new();
            let _ = file.read_to_string(&mut content);
            let json_signers: Result<Vec<JsonSigner>, _> = serde_json::from_str(&content);
            if let Ok(signers) = json_signers {
                let hot_signers = signers
                    .into_iter()
                    .map(|s| {
                        let signer = HotSigner::from_json(s);
                        (signer.fingerprint(), signer)
                    })
                    .collect();
                let mut manager = SigningManager {
                    hot_signers,
                    ..Default::default()
                };
                let sender = manager.sender.clone();
                for signer in manager.hot_signers.values_mut() {
                    signer.init(sender.clone());
                }
                manager
            } else {
                Default::default()
            }
        } else {
            Default::default()
        }
    }

    pub fn persist(&self) {
        match File::create(Self::path()) {
            Ok(mut file) => {
                let content: Vec<_> = self
                    .hot_signers
                    .clone()
                    .into_values()
                    .map(|s| s.to_json())
                    .collect();
                let str_content = serde_json::to_string_pretty(&content).expect("cannot_fail");
                let _ = file.write(str_content.as_bytes());
            }
            Err(e) => {
                log::error!("SigningManager::persist() fail to open file: {e}");
            }
        }
    }

    pub fn poll(&self) -> Option<SignerNotif> {
        self.receiver.try_recv().ok()
    }

    pub fn new_hot_signer(&mut self, network: Network) {
        let mnemomic = bip39::Mnemonic::generate(12).unwrap();
        self.new_hot_signer_from_mnemonic(network, mnemomic.to_string());
    }

    pub fn new_hot_signer_from_mnemonic(&mut self, network: Network, mnemonic: String) {
        let signer = HotSigner::new_from_mnemonics(network.into(), &mnemonic).unwrap();
        self.hot_signers.insert(signer.fingerprint(), signer);
        self.persist();
    }
}
