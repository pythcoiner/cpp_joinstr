use joinstr::nostr::{self, Fee, Timeline};

#[derive(Clone)]
pub struct Pool {
    inner: nostr::Pool,
}

impl Pool {
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }

    pub fn denomination_sat(&self) -> u64 {
        self.inner.payload.as_ref().unwrap().denomination.to_sat()
    }

    pub fn denomination_btc(&self) -> f64 {
        self.inner.payload.as_ref().unwrap().denomination.to_btc()
    }

    pub fn fees(&self) -> u32 {
        if let Fee::Fixed(fee) = self.inner.payload.as_ref().unwrap().fee {
            return fee;
        }
        unreachable!()
    }

    pub fn timeout(&self) -> u64 {
        if let Timeline::Simple(timeout) = self.inner.payload.as_ref().unwrap().timeout {
            return timeout;
        }
        unreachable!()
    }

    pub fn peers(&self) -> usize {
        self.inner.payload.as_ref().unwrap().peers
    }

    pub fn relay(&self) -> String {
        self.inner
            .payload
            .as_ref()
            .unwrap()
            .relays
            .first()
            .unwrap()
            .to_string()
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
