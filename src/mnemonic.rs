use crate::result;
use joinstr::bip39;
use std::str::FromStr;

result!(Mnemonic, bip39::Mnemonic);

pub fn mnemonic_from_string(value: String) -> Box<Mnemonic> {
    match bip39::Mnemonic::from_str(&value) {
        Ok(m) => Mnemonic::ok(m),
        Err(e) => Mnemonic::err(&e.to_string()),
    }
    .boxed()
}

impl From<Mnemonic> for bip39::Mnemonic {
    fn from(value: Mnemonic) -> Self {
        value.value()
    }
}

pub fn generate_mnemonic() -> String {
    bip39::Mnemonic::generate(12).unwrap().to_string()
}
