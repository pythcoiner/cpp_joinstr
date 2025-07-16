use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    str::FromStr,
    sync::mpsc::{self},
};

use joinstr::{
    bip39::{self},
    miniscript::bitcoin::{
        bip32::{self, DerivationPath},
        Psbt,
    },
};

use crate::{
    config,
    cpp_joinstr::Network,
    signer::{wpkh, HotSigner, JsonSigner, Signer, SignerNotif},
};

#[derive(Debug, Clone)]
pub enum Error {
    ParsePsbt,
}

/// A manager for handling hot signers and their notifications.
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
    /// Returns the path to the signers' data directory.
    pub fn path() -> PathBuf {
        let mut path = config::datadir();
        path.push(".signers");
        path
    }
    /// Creates a `SigningManager` instance from a file.
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

    /// Persists the current state of the signers to a file.
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

    /// Polls for a new signer notification.
    ///
    /// # Returns
    /// An `Option<SignerNotif>` which is `Some` if a notification is available,
    /// or `None` if there are no new notifications.
    pub fn poll(&self) -> Option<SignerNotif> {
        self.receiver.try_recv().ok()
    }

    /// Creates a new hot signer with a generated mnemonic.
    ///
    /// # Parameters
    /// - `network`: The network for which the hot signer is created.
    pub fn new_hot_signer(&mut self, network: Network) {
        let mnemomic = bip39::Mnemonic::generate(12).unwrap();
        self.new_hot_signer_from_mnemonic(network, mnemomic.to_string());
    }

    /// Creates a new hot signer from a given mnemonic.
    ///
    /// # Parameters
    /// - `network`: The network for which the hot signer is created.
    /// - `mnemonic`: The mnemonic used to create the hot signer.
    pub fn new_hot_signer_from_mnemonic(&mut self, network: Network, mnemonic: String) {
        let mut signer = HotSigner::new_from_mnemonics(network.into(), &mnemonic).unwrap();
        signer.init(self.sender.clone());
        self.hot_signers.insert(signer.fingerprint(), signer);
    }

    pub fn sign(&self, network: Network, psbt: String) {
        let psbt = match Psbt::from_str(&psbt) {
            Ok(p) => p,
            Err(_) => {
                if self
                    .sender
                    .send(SignerNotif::Manager(Error::ParsePsbt))
                    .is_err()
                {
                    log::error!("SigningManager::sign() fails to send notif")
                }
                return;
            }
        };

        let signer = self
            .hot_signers
            .iter()
            .next()
            .expect("at least one signer")
            .1;

        let n_path = match network {
            Network::Bitcoin => 0,
            _ => 1,
        };
        let deriv_path = DerivationPath::from_str(&format!("m/84'/{}'/0'", n_path)).unwrap();
        let xpub = signer.xpub(&deriv_path);
        let descriptor = wpkh(xpub);

        signer.sign(psbt, descriptor);
    }
}

#[cfg(test)]
mod tests {
    use bip32::Fingerprint;

    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_manager_hot_signer() {
        let mut manager = SigningManager::default();
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".to_string();
        manager.new_hot_signer_from_mnemonic(Network::Regtest, mnemonic);
        if let SignerNotif::Info(fg, _info) = manager.poll().unwrap() {
            assert_eq!(fg, Fingerprint::from_str("73c5da0a").unwrap());
        } else {
            panic!("expect info");
        }
    }
}
