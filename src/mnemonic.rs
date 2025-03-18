use crate::result;
use joinstr::bip39;
use std::str::FromStr;

result!(Mnemonic, bip39::Mnemonic);

pub fn mnemonic_from_string(value: String) -> Box<Mnemonic> {
    let mut res = Mnemonic::new();
    match bip39::Mnemonic::from_str(&value) {
        Ok(m) => res.set(m),
        Err(e) => res.set_error(e.to_string()),
    }
    Box::new(res)
}

impl From<Mnemonic> for bip39::Mnemonic {
    fn from(value: Mnemonic) -> Self {
        value.unwrap()
    }
}
