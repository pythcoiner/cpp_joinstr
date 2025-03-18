use std::str::FromStr;

use joinstr::miniscript::bitcoin::{self, address::NetworkUnchecked};

use crate::result;

result!(Address, bitcoin::Address<NetworkUnchecked>);

pub fn address_from_string(value: String) -> Box<Address> {
    let mut res = Address::new();
    match bitcoin::Address::<NetworkUnchecked>::from_str(&value) {
        Ok(m) => res.set(m),
        Err(e) => res.set_error(e.to_string()),
    }
    Box::new(res)
}

impl From<Address> for bitcoin::Address<NetworkUnchecked> {
    fn from(value: Address) -> Self {
        value.unwrap()
    }
}
