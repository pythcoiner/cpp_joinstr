use joinstr::{
    bip39::Mnemonic,
    interface::{self},
    miniscript::bitcoin::{self, Address, Amount, OutPoint, ScriptBuf, Sequence, TxOut, Txid},
    signer::CoinPath,
};
use qt_joinstr::{Coin, CoinJoinResult, QString, QUrl};
use std::str::FromStr;

#[cxx_qt::bridge]
pub mod qt_joinstr {

    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
    }

    extern "Rust" {

        fn list_coins(
            mnemonics: QString,
            electrum_address: QUrl,
            start_index: u32,
            stop_index: u32,
            network: Network,
        ) -> ListCoinsResult;

        #[allow(clippy::too_many_arguments)]
        fn initiate_coinjoin(
            // Pool
            denomination: f64,
            fee: u32,
            max_duration: u64,
            peers: u8,
            network: Network,
            // Peer
            mnemonics: QString,
            electrum_address: QUrl,
            input: Coin,
            output: QString,
            relay: QString,
        ) -> CoinJoinResult;
    }

    struct Coin {
        pub txout_amount: u64,
        txout_script_pubkey: Vec<u8>,
        pub outpoint: QString,
        sequence: u32,
        coinpath_depth: u32,
        coinpath_index: u32,
    }

    struct CoinJoinResult {
        pub txid: QString,
        pub error: QString,
    }

    struct ListCoinsResult {
        pub coins: Vec<Coin>,
        pub error: QString,
    }

    enum Network {
        /// Mainnet Bitcoin.
        Bitcoin,
        /// Bitcoin's testnet network.
        Testnet,
        /// Bitcoin's signet network.
        Signet,
        /// Bitcoin's regtest network.
        Regtest,
    }
}

impl CoinJoinResult {
    pub fn error(msg: &str) -> Self {
        CoinJoinResult {
            txid: String::new().into(),
            error: msg.to_string().into(),
        }
    }

    pub fn ok(txid: Txid) -> Self {
        CoinJoinResult {
            txid: txid.to_string().into(),
            error: String::new().into(),
        }
    }
}

impl From<joinstr::signer::Coin> for qt_joinstr::Coin {
    fn from(value: joinstr::signer::Coin) -> Self {
        qt_joinstr::Coin {
            txout_amount: value.txout.value.to_sat(),
            txout_script_pubkey: value.txout.script_pubkey.into_bytes(),
            outpoint: value.outpoint.to_string().into(),
            sequence: value.sequence.to_consensus_u32(),
            coinpath_depth: value.coin_path.depth,
            coinpath_index: value.coin_path.index.unwrap(),
        }
    }
}

impl From<qt_joinstr::Coin> for joinstr::signer::Coin {
    fn from(value: qt_joinstr::Coin) -> Self {
        joinstr::signer::Coin {
            txout: TxOut {
                value: Amount::from_sat(value.txout_amount),
                script_pubkey: ScriptBuf::from_bytes(value.txout_script_pubkey),
            },
            outpoint: OutPoint::from_str(&value.outpoint.to_string()).unwrap(),
            sequence: Sequence::from_consensus(value.sequence),
            coin_path: CoinPath {
                depth: value.coinpath_depth,
                index: Some(value.coinpath_index),
            },
        }
    }
}

impl From<qt_joinstr::Network> for bitcoin::Network {
    fn from(value: qt_joinstr::Network) -> Self {
        match value {
            qt_joinstr::Network::Bitcoin => Self::Bitcoin,
            qt_joinstr::Network::Testnet => Self::Testnet,
            qt_joinstr::Network::Signet => Self::Signet,
            qt_joinstr::Network::Regtest => Self::Regtest,
            _ => unreachable!(),
        }
    }
}

impl From<bitcoin::Network> for qt_joinstr::Network {
    fn from(value: bitcoin::Network) -> Self {
        match value {
            bitcoin::Network::Bitcoin => Self::Bitcoin,
            bitcoin::Network::Testnet => Self::Testnet,
            bitcoin::Network::Signet => Self::Signet,
            bitcoin::Network::Regtest => Self::Regtest,
            _ => unreachable!(),
        }
    }
}

fn list_coins(
    mnemonics: QString,
    electrum_address: QUrl,
    start_index: u32,
    stop_index: u32,
    network: qt_joinstr::Network,
) -> qt_joinstr::ListCoinsResult {
    let electrum_port = electrum_address.port_or(-1);
    if electrum_port == -1 {
        return qt_joinstr::ListCoinsResult {
            coins: Vec::new(),
            error: "electrum_address.port must be specified!"
                .to_string()
                .into(),
        };
    }

    let res = interface::list_coins(
        mnemonics.to_string(),
        electrum_address.to_string(),
        electrum_port as u16,
        (start_index, stop_index),
        network.into(),
    );

    let mut result = qt_joinstr::ListCoinsResult {
        coins: Vec::new(),
        error: String::new().into(),
    };

    match res {
        Ok(r) => {
            for c in r {
                result.coins.push(c.into());
            }
        }
        Err(e) => result.error = format!("{:?}", e).into(),
    }

    result
}

#[allow(clippy::too_many_arguments)]
pub fn initiate_coinjoin(
    // Pool
    denomination: f64,
    fee: u32,
    max_duration: u64,
    peers: u8,
    network: qt_joinstr::Network,
    // Peer
    mnemonics: QString,
    electrum_address: QUrl,
    input: Coin,
    output: QString,
    relay: QString,
) -> CoinJoinResult {
    let electrum_port = electrum_address.port_or(-1);
    if electrum_port == -1 {
        return CoinJoinResult::error("electrum_address.port must be specified!");
    }
    let electrum_address = electrum_address.to_string();
    let electrum_port = electrum_port as u16;

    let pool = joinstr::interface::PoolConfig {
        denomination,
        fee,
        max_duration,
        peers: peers.into(),
        network: bitcoin::Network::from(network),
    };

    let mnemonics = match Mnemonic::from_str(&mnemonics.to_string()) {
        Ok(m) => m,
        Err(e) => {
            return CoinJoinResult::error(&format!("Invalid mnemonic: {e}"));
        }
    };

    let input = joinstr::signer::Coin::from(input);
    let output = match Address::from_str(&output.to_string()) {
        Ok(a) => a,
        Err(e) => return CoinJoinResult::error(&format!("Wrong address: {e}")),
    };

    let peer = joinstr::interface::PeerConfig {
        mnemonics,
        electrum_address,
        electrum_port,
        input,
        output,
        relay: relay.into(),
    };

    match interface::initiate_coinjoin(pool, peer) {
        Ok(txid) => CoinJoinResult::ok(txid),
        Err(e) => CoinJoinResult::error(&format!("{e:?}")),
    }
}
