use ::joinstr::signer;
use joinstr::miniscript::bitcoin::{self, Sequence, TxOut};
use serde::{Deserialize, Serialize};

use crate::cpp_joinstr::AddrAccount;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A struct that represents a coin in a Bitcoin transaction.
///
/// This struct contains information about the transaction output,
/// the outpoint, the sequence number, and the derivation path of the coin.
pub struct Coin {
    pub txout: TxOut,
    pub outpoint: bitcoin::OutPoint,
    pub sequence: Sequence,
    pub coin_path: (AddrAccount, u32),
}

impl From<Coin> for signer::Coin {
    fn from(value: Coin) -> Self {
        let (account, index) = value.coin_path;
        signer::Coin {
            txout: value.txout,
            outpoint: value.outpoint,
            sequence: value.sequence,
            coin_path: signer::CoinPath {
                depth: account.into(),
                index: Some(index),
            },
        }
    }
}
