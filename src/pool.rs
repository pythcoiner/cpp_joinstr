use joinstr::nostr::{self, Fee, Timeline};

#[derive(Clone)]
/// A struct representing a pool in the Nostr protocol.
pub struct Pool {
    inner: nostr::Pool,
}

impl Pool {
    /// Returns the ID of the pool.
    pub fn id(&self) -> String {
        self.inner.id.clone()
    }

    /// Returns the denomination in satoshis.
    pub fn denomination_sat(&self) -> u64 {
        self.inner.payload.as_ref().unwrap().denomination.to_sat()
    }

    /// Returns the denomination in bitcoins.
    pub fn denomination_btc(&self) -> f64 {
        self.inner.payload.as_ref().unwrap().denomination.to_btc()
    }

    /// Returns the fees associated with the pool.
    pub fn fees(&self) -> u32 {
        if let Fee::Fixed(fee) = self.inner.payload.as_ref().unwrap().fee {
            return fee;
        }
        unreachable!()
    }

    /// Returns the timeout value for the pool.
    pub fn timeout(&self) -> u64 {
        if let Timeline::Simple(timeout) = self.inner.payload.as_ref().unwrap().timeout {
            return timeout;
        }
        unreachable!()
    }

    /// Returns the number of peers in the pool.
    pub fn peers(&self) -> usize {
        self.inner.payload.as_ref().unwrap().peers
    }

    /// Returns the relay address from the pool.
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
