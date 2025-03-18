use crate::qt_joinstr::{Network, PoolConfig};
use joinstr::{interface, miniscript::bitcoin};

impl From<PoolConfig> for interface::PoolConfig {
    fn from(value: PoolConfig) -> Self {
        interface::PoolConfig {
            denomination: value.denomination,
            fee: value.fee,
            max_duration: value.max_duration,
            peers: value.peers,
            network: (*value.network).into(),
        }
    }
}

impl From<Network> for bitcoin::Network {
    fn from(value: Network) -> Self {
        match value {
            Network::Regtest => bitcoin::Network::Regtest,
            Network::Signet => bitcoin::Network::Signet,
            Network::Testnet => bitcoin::Network::Testnet,
            Network::Bitcoin => bitcoin::Network::Bitcoin,
            _ => unreachable!(),
        }
    }
}
