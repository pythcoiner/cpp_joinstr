use joinstr::signer;

#[derive(Clone)]
pub struct Coin {
    inner: signer::Coin,
}

impl Coin {
    pub fn amount_sat(&self) -> u64 {
        self.inner.txout.value.to_sat()
    }

    pub fn amount_btc(&self) -> f64 {
        self.inner.txout.value.to_btc()
    }

    pub fn outpoint(&self) -> String {
        self.inner.outpoint.to_string()
    }
}

impl From<signer::Coin> for Coin {
    fn from(value: signer::Coin) -> Self {
        Coin { inner: value }
    }
}

impl From<Coin> for signer::Coin {
    fn from(value: Coin) -> Self {
        value.inner
    }
}
