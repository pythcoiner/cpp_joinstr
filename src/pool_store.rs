use crate::{
    account::{Error, JoinstrNotif, Notification},
    coin::Coin,
    cpp_joinstr::{PoolRole, PoolStatus, RustPool},
};
use joinstr::{
    electrum::short_string,
    joinstr::{Joinstr, Step},
    miniscript::bitcoin::{address::NetworkUnchecked, Address, Network},
    nostr::{self, Pool},
    simple_nostr_client::nostr::key::Keys,
    utils::now,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

/// A structure to manage a collection of pools.
#[derive(Debug, Default)]
pub struct PoolStore {
    store: BTreeMap<String, PoolEntry>,
}

impl PoolStore {
    /// Creates a new instance of `PoolStore`.
    pub fn new() -> Self {
        Self {
            store: BTreeMap::default(),
        }
    }

    /// Updates the status of a pool in the store.
    ///
    /// Returns `true` if the status was changed, `false` otherwise.
    pub fn update(&mut self, pool: nostr::Pool, status: PoolStatus) -> bool /* updated */ {
        let mut updated = false;
        self.store
            .entry(pool.id.clone())
            .and_modify(|e| {
                // NOTE: if `role` is assigned or there is an handle we do not update here
                // as we can overwrite more relevant updates done by the peer/initiator thread
                if !(e.role != PoolRole::None || e.handle.is_some()) && e.status != status {
                    e.status = status;
                    updated = true;
                }
            })
            .or_insert(PoolEntry {
                pool,
                status,
                role: PoolRole::None,
                step: None,
                handle: None,
            });
        updated
    }

    pub fn get(&self, id: &str) -> Option<PoolEntry> {
        self.store.get(id).cloned()
    }

    /// Retrieves all pools with the specified status.
    ///
    /// # Arguments
    /// * `status` - The status to filter pools by.
    pub fn get_by_status(&self, status: PoolStatus) -> Vec<RustPool> {
        self.store
            .clone()
            .into_iter()
            .filter_map(|(_, pool)| {
                if pool.status == status {
                    Some(pool.into())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    /// Retrieves all available pools.
    pub fn available_pools(&self) -> Vec<RustPool> {
        self.store
            .clone()
            .into_iter()
            .filter_map(|(_, entry)| match entry.status {
                PoolStatus::Available
                | PoolStatus::Posting
                | PoolStatus::Connecting
                | PoolStatus::RegisterOutputs
                | PoolStatus::RegisterInputs
                | PoolStatus::Broadcast => Some(entry.into()),
                _ => None,
            })
            .collect()
    }

    /// Initiate a new pool with a given coin.
    #[allow(clippy::complexity)]
    pub fn create_pool(
        denomination: f64,
        fee: u32,
        timeout: u64,
        peers: usize,
        coin: Coin,
        address: Address<NetworkUnchecked>,
        mnemonic: String,
        relay: String,
        electrum: (String, u16),
        network: Network,
        store: Arc<Mutex<PoolStore>>,
        sender: mpsc::Sender<Notification>,
    ) {
        log::debug!("PoolStore::create_pool()");
        let cloned_store = store.clone();
        let (id_sender, id_recv) = mpsc::channel::<Option<String>>();
        let cloned_sender = sender.clone();
        let signer = match joinstr::signer::WpkhHotSigner::new_from_mnemonics(network, &mnemonic) {
            Ok(s) => s,
            Err(e) => {
                log::error!("PoolStore::create_pool() fail to create signer: {e}");
                let _ = id_sender.send(None);
                let _ = sender.send(e.into());
                return;
            }
        };
        let handle = thread::spawn(move || {
            let mut j = match initiator(
                denomination,
                fee,
                timeout,
                peers,
                relay,
                coin,
                address,
                electrum,
                network,
            ) {
                Ok(j) => j,
                Err(e) => {
                    log::error!("PoolStore::create_pool() fail create initiator: {e:?}");
                    let _ = id_sender.send(None);
                    let _ = sender.send(e.into());
                    return;
                }
            };
            log::info!("PoolStore::create_pool() start coinjoin...");
            j.start_coinjoin(None, Some(signer));
            let pool = loop {
                match j.state() {
                    Some(s) => break s.pool.clone(),
                    None => {
                        log::debug!(
                            "PoolStore::create_pool() initiator not yet have state, waiting..."
                        );
                        thread::sleep(Duration::from_millis(100))
                    }
                }
            };
            let pool_id = pool.id.clone();
            let short_id = short_string(pool_id.clone());
            log::info!("PoolStore::create_pool() pool created with id {short_id}");
            let pool_entry = PoolEntry {
                status: PoolStatus::Available,
                pool,
                role: PoolRole::Initiator,
                step: None,
                handle: None,
            };

            store
                .lock()
                .expect("poisoned")
                .store
                .insert(pool_id.clone(), pool_entry);
            let _ = id_sender.send(Some(pool_id.clone()));

            let mut last_step: Option<Step> = None;

            loop {
                thread::sleep(Duration::from_millis(300));
                let step = j.state().expect("must have a state").step;
                let update_step = match last_step {
                    None => true,
                    Some(s) => s != step,
                };
                if update_step {
                    log::info!("PoolStore::create_pool() step for pool {short_id} changed: {last_step:?} => {step:?} ");
                    last_step = Some(step);
                    store
                        .lock()
                        .expect("poisoned")
                        .store
                        .get_mut(&pool_id)
                        .expect("present")
                        .update_step(step);
                    let _ = sender.send(JoinstrNotif::PoolUpdate.into());
                }
                if matches!(step, Step::Mined) {
                    break;
                }
            }
        });
        let handle = Arc::new(Mutex::new(handle));
        if let Ok(Some(pool_id)) = id_recv.recv() {
            cloned_store
                .lock()
                .expect("poisoned")
                .store
                .get_mut(&pool_id)
                .expect("pool must exists")
                .handle = Some(handle);
        } else {
            let _ = cloned_sender.send(Notification::Joinstr(JoinstrNotif::Error(
                Error::CreatePool,
            )));
        }
    }

    /// Join a pool with a given coin.
    #[allow(clippy::complexity)]
    pub fn join_pool(
        relay: String,
        electrum: (String, u16),
        pool: Pool,
        mnemonic: String,
        network: Network,
        store: Arc<Mutex<PoolStore>>,
        sender: mpsc::Sender<Notification>,
        coin: Coin,
        address: Address<NetworkUnchecked>,
    ) {
        log::debug!("PoolStore::join_pool()");
        let cloned_store = store.clone();
        let signer = match joinstr::signer::WpkhHotSigner::new_from_mnemonics(network, &mnemonic) {
            Ok(s) => s,
            Err(e) => {
                log::error!("PoolStore::join_pool() fail to create signer: {e}");
                let _ = sender.send(e.into());
                return;
            }
        };
        let cloned_pool = pool.clone();
        let short_id = short_string(pool.clone().id);
        let handle = thread::spawn(move || {
            let pool_id = pool.id.clone();
            let mut j = match peer(pool.clone(), relay, coin, electrum, network, address) {
                Ok(j) => j,
                Err(e) => {
                    log::error!("PoolStore::join_pool() fail to create peer: {e:?}");
                    let _ = sender.send(e.into());
                    return;
                }
            };
            log::info!("PoolStore::join_pool() start coinjoin...");
            j.start_coinjoin(Some(pool.clone()), Some(signer));
            let pool_entry = PoolEntry {
                status: PoolStatus::Available,
                pool,
                role: PoolRole::Peer,
                step: None,
                handle: None,
            };

            store
                .lock()
                .expect("poisoned")
                .store
                .insert(pool_id.clone(), pool_entry);

            let mut last_step: Option<Step> = None;

            loop {
                thread::sleep(Duration::from_millis(300));
                let step = j.state().expect("must have a state").step;
                let update_step = match last_step {
                    None => true,
                    Some(s) => s != step,
                };
                if update_step {
                    log::info!("PoolStore::join_pool() step for pool {short_id} changed: {last_step:?} => {step:?} ");
                    last_step = Some(step);
                    store
                        .lock()
                        .expect("poisoned")
                        .store
                        .get_mut(&pool_id)
                        .expect("present")
                        .update_step(step);
                    let _ = sender.send(JoinstrNotif::PoolUpdate.into());
                }
                if matches!(step, Step::Mined) {
                    break;
                }
            }
        });
        let pool_id = cloned_pool.id.clone();
        let handle = Arc::new(Mutex::new(handle));
        cloned_store
            .lock()
            .expect("poisoned")
            .store
            .get_mut(&pool_id)
            .expect("pool must exists")
            .handle = Some(handle);
    }
}

#[allow(clippy::complexity)]
pub fn initiator(
    denomination: f64,
    fee: u32,
    timeout: u64,
    peers: usize,
    relay: String,
    coin: Coin,
    address: Address<NetworkUnchecked>,
    electrum: (String, u16),
    network: Network,
) -> Result<Joinstr<'static>, joinstr::joinstr::Error> {
    let keys = Keys::generate();
    let timestamp = now() + timeout;
    let electrum_server = (electrum.0.as_str(), electrum.1);
    let mut j = Joinstr::new_initiator(keys, relay, electrum_server, network, "initiator")?
        .denomination(denomination)?
        .fee(fee)?
        .simple_timeout(timestamp)?
        .min_peers(peers)?;
    let coin = coin.into();
    j.set_coin(coin)?;
    j.set_address(address)?;
    Ok(j)
}

pub fn peer(
    pool: Pool,
    relay: String,
    coin: Coin,
    electrum: (String, u16),
    network: Network,
    output: Address<NetworkUnchecked>,
) -> Result<Joinstr<'static>, joinstr::joinstr::Error> {
    let coin = coin.into();
    let electrum_server = (electrum.0.as_str(), electrum.1);
    Joinstr::new_peer_with_electrum(relay, &pool, electrum_server, coin, output, network, "peer")
}

/// Represents a single pool entry with its status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntry {
    status: PoolStatus,
    pool: nostr::Pool,
    role: PoolRole,
    step: Option<Step>,
    #[serde(skip)]
    handle: Option<Arc<Mutex<JoinHandle<()>>>>,
}

impl PoolEntry {
    /// Returns the ID of the pool.
    pub fn pool_id(&self) -> String {
        self.pool.id.clone()
    }
    /// Returns the status of the pool.
    pub fn status(&self) -> PoolStatus {
        self.status
    }
    /// Returns the a clone of the pool
    pub fn pool(&self) -> nostr::Pool {
        self.pool.clone()
    }
    /// Update the PoolEntry given the
    pub fn update_step(&mut self, step: Step) {
        self.step = Some(step);
        let status = match step {
            Step::Posting => Some(PoolStatus::Posting),
            Step::Connecting => Some(PoolStatus::Connecting),
            Step::OutputRegistration => Some(PoolStatus::RegisterOutputs),
            Step::InputRegistration => Some(PoolStatus::RegisterInputs),
            Step::Broadcast => Some(PoolStatus::Broadcast),
            Step::Mined => Some(PoolStatus::Mined),
            Step::Failed => Some(PoolStatus::Closed),
            _ => None,
        };
        if let Some(status) = status {
            self.status = status;
        }
    }
}

impl From<PoolEntry> for RustPool {
    fn from(value: PoolEntry) -> Self {
        // let payload = value.payload.expect("have a payload");
        let payload = value.pool().payload.expect("have a payload");
        RustPool {
            denomination: payload.denomination.to_sat(),
            total_peers: payload.peers,
            current_peers: 0,
            relay: payload
                .relays
                .first()
                .expect("always have a relay")
                .to_string(),
            fees: match payload.fee {
                nostr::Fee::Fixed(f) => f,
                nostr::Fee::Provider(_) => unreachable!(),
            },
            id: value.pool_id(),
            status: value.status(),
            role: value.role,
            timeout: match payload.timeout {
                nostr::Timeline::Simple(t) => t,
                _ => unreachable!(),
            },
        }
    }
}
