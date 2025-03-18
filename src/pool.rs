use joinstr::nostr::{self, Fee};

#[derive(Clone)]
pub struct Pool {
    inner: nostr::Pool,
}

impl Pool {
    pub fn denomination_sat(&self) -> u64 {
        self.inner
            .payload
            .as_ref()
            .clone()
            .unwrap()
            .denomination
            .to_sat()
    }

    pub fn denomination_btc(&self) -> f64 {
        self.inner
            .payload
            .as_ref()
            .clone()
            .unwrap()
            .denomination
            .to_btc()
    }

    pub fn peers(&self) -> usize {
        self.inner.payload.as_ref().clone().unwrap().peers
    }

    pub fn relay(&self) -> String {
        self.inner
            .payload
            .as_ref()
            .clone()
            .unwrap()
            .relays
            .first()
            .unwrap()
            .to_string()
    }

    pub fn fee(&self) -> u32 {
        self.inner
            .payload
            .as_ref()
            .map(|p| {
                if let Fee::Fixed(fee) = p.fee {
                    fee
                } else {
                    unreachable!()
                }
            })
            .unwrap()
    }
}

impl From<nostr::Pool> for Pool {
    fn from(value: nostr::Pool) -> Self {
        Self { inner: value }
    }
}

impl From<Pool> for nostr::Pool {
    fn from(value: Pool) -> Self {
        value.inner
    }
}
