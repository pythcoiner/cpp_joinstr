use joinstr::interface;

use crate::qt_joinstr::PeerConfig;

impl From<PeerConfig> for interface::PeerConfig {
    fn from(value: PeerConfig) -> Self {
        interface::PeerConfig {
            mnemonics: (*value.mnemonics).into(),
            electrum_address: value.electrum_url,
            electrum_port: value.electrum_port,
            input: (*value.input).into(),
            output: (*value.output).into(),
            relay: value.relay,
        }
    }
}
