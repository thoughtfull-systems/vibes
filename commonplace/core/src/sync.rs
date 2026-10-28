// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Syncs libraries with other devices over iroh, as decision 0013 describes and the sync spec
//! defines.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use iroh::endpoint::{Connection, ConnectionError, RecvStream, SendStream, presets};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Notify;
use tokio::task::AbortHandle;
use tokio::time::{sleep, timeout};

use crate::{Error, NoteId};

mod pairing;

pub use pairing::{Nearby, Outgoing, OutgoingState, Request, pairing_code, qr_code};

pub const ALPN: &[u8] = b"commonplace/sync/1";
pub const PAIR_ALPN: &[u8] = b"commonplace/pair/1";
const CONFIG: &str = "sync.json";
const KEY: &str = "sync.key";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const DEBOUNCE: Duration = Duration::from_secs(3);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_MESSAGE: usize = 1 << 30;
const MAX_RETRY: Duration = Duration::from_secs(10 * 60);
const MIN_RETRY: Duration =
    if cfg!(test) { Duration::from_millis(200) } else { Duration::from_secs(30) };
const PERIOD: Duration = Duration::from_secs(15 * 60);
/// The code with which a device closes a connection from a device it doesn't trust.
const REFUSED: u32 = 1;

/// What a device keeps about sync in its own store, outside every library.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct Config {
    pub automatic: bool,
    pub devices: Vec<Device>,
    /// This device's own name, which devices that pair with it learn.
    pub name: String,
    pub offers: Vec<Offer>,
    pub relay: Option<String>,
    pub shares: Vec<Share>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            automatic: true,
            devices: Vec::new(),
            name: String::new(),
            offers: Vec::new(),
            relay: None,
            shares: Vec::new(),
        }
    }
}

impl Config {
    pub fn device(&self, address: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.address == address)
    }

    pub fn share(&self, id: &str) -> Option<&Share> {
        self.shares.iter().find(|share| share.id == id)
    }

    /// The shares that this device syncs with `device`.
    pub fn shares_with<'a>(&'a self, device: &'a str) -> impl Iterator<Item = &'a Share> {
        self.shares.iter().filter(move |share| share.devices.iter().any(|d| d == device))
    }

    /// The offers that wait for the owner to accept or decline them.
    pub fn open_offers(&self) -> impl Iterator<Item = &Offer> {
        self.offers.iter().filter(|offer| !offer.declined)
    }
}

/// Another device that this device trusts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Device {
    /// The device's public key, in iroh's text form.
    pub address: String,
    /// Addresses on which to try the device before looking it up.
    #[serde(default)]
    pub direct: Vec<SocketAddr>,
    /// The name this device gives it.
    pub name: String,
}

/// A library that this device syncs with other devices.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Share {
    pub devices: Vec<String>,
    pub id: String,
    /// The key of the library on this device.
    pub library: String,
    /// Whether the share has synced on this device, after which merges stop repairing notes.
    #[serde(default)]
    pub synced: bool,
}

/// A share that another device offered this one.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Offer {
    #[serde(default)]
    pub declined: bool,
    pub device: String,
    pub share: String,
}

/// How the last syncs with a device went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub error: Option<String>,
    /// When the device last synced, as a timestamp.
    pub synced: Option<i64>,
    pub syncing: bool,
}

/// The owner of a device's libraries, which a sync reads and writes through. A sync calls it from
/// a background thread, and it may block until the libraries are free.
pub trait Host: Send + Sync + 'static {
    fn version(&self, library: &str) -> Result<Vec<u8>, Error>;

    fn updates_since(&self, library: &str, version: &[u8]) -> Result<Option<Vec<u8>>, Error>;

    fn merge_updates(&self, library: &str, updates: &[u8], repair: bool) -> Result<(), Error>;

    /// Reports that the configuration or a device's status changed.
    fn changed(&self);
}

#[derive(Clone, Copy)]
enum Network {
    Internet,
    #[cfg(test)]
    Loopback,
}

/// Syncs a device's libraries in the background for as long as it lives.
pub struct Syncer {
    inner: Arc<Inner>,
    runtime: Option<Runtime>,
}

impl Syncer {
    /// Starts syncing, with the device's key and configuration kept in `dir`.
    pub fn start(dir: &Path, host: Arc<dyn Host>) -> Result<Self, Error> {
        Self::start_on(dir, host, Network::Internet)
    }

    fn start_on(dir: &Path, host: Arc<dyn Host>, network: Network) -> Result<Self, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("commonplace-sync")
            .worker_threads(2)
            .build()?;
        let config = load_config(dir)?;
        let key = load_key(dir)?;
        let endpoint = runtime.block_on(bind(key, &config, network))?;
        let inner = Arc::new(Inner {
            config: Mutex::new(config),
            dir: dir.to_owned(),
            endpoint,
            handle: runtime.handle().clone(),
            host,
            pairing: Mutex::default(),
            pending: Mutex::default(),
            reconnect: Notify::new(),
            statuses: Mutex::default(),
            wake: Notify::new(),
        });
        runtime.spawn(accept(inner.clone()));
        runtime.spawn(schedule(inner.clone()));
        Ok(Self { inner, runtime: Some(runtime) })
    }

    /// This device's address, for the owner to enter on other devices.
    pub fn address(&self) -> String {
        self.inner.endpoint.id().to_string()
    }

    pub fn config(&self) -> Config {
        self.inner.config().clone()
    }

    pub fn status(&self, device: &str) -> Status {
        self.inner.statuses().get(device).map(|entry| entry.status.clone()).unwrap_or_default()
    }

    /// Trusts the device at `address`, which is a public key, optionally followed by `@` and a
    /// comma-separated list of socket addresses to try first. Returns the key in its usual form.
    pub fn add_device(&self, address: &str, name: &str) -> Result<String, Error> {
        let (key, direct) = parse_address(address)?;
        if key == self.address() {
            return Err(Error::InvalidAddress(address.to_owned()));
        }
        self.inner.trust(&key, name, direct)?;
        self.inner.sync_device(&key);
        Ok(key)
    }

    pub fn rename_device(&self, address: &str, name: &str) -> Result<(), Error> {
        self.inner.update(|config| {
            let device = config
                .devices
                .iter_mut()
                .find(|device| device.address == address)
                .ok_or_else(|| Error::NoSuchDevice(address.to_owned()))?;
            device.name = name.to_owned();
            Ok(())
        })
    }

    /// Stops trusting a device, and stops sharing every library with it.
    pub fn remove_device(&self, address: &str) -> Result<(), Error> {
        self.inner.update(|config| {
            config.devices.retain(|device| device.address != address);
            config.offers.retain(|offer| offer.device != address);
            for share in &mut config.shares {
                share.devices.retain(|device| device != address);
            }
            config.shares.retain(|share| !share.devices.is_empty());
            Ok(())
        })?;
        self.inner.statuses().remove(address);
        Ok(())
    }

    /// Shares `library` with `device`, which then receives an offer.
    pub fn share(&self, library: &str, device: &str) -> Result<(), Error> {
        self.inner.share(library, device)?;
        self.inner.sync_device(device);
        Ok(())
    }

    /// Stops syncing `library` with `device`.
    pub fn unshare(&self, library: &str, device: &str) -> Result<(), Error> {
        self.inner.update(|config| {
            for share in config.shares.iter_mut().filter(|share| share.library == library) {
                share.devices.retain(|d| d != device);
            }
            config.shares.retain(|share| !share.devices.is_empty());
            Ok(())
        })
    }

    /// Accepts a share that `device` offered, to sync with `library`. If this device already
    /// syncs the share with others, `library` has to be the one it syncs.
    pub fn accept(&self, share: &str, device: &str, library: &str) -> Result<(), Error> {
        self.inner.update(|config| {
            if config.device(device).is_none() {
                return Err(Error::NoSuchDevice(device.to_owned()));
            }
            match config.shares.iter_mut().find(|s| s.id == share) {
                Some(existing) if existing.library != library => {
                    return Err(Error::ShareInOtherLibrary(share.to_owned()));
                }
                Some(existing) => {
                    if !existing.devices.iter().any(|d| d == device) {
                        existing.devices.push(device.to_owned());
                    }
                }
                None => config.shares.push(Share {
                    devices: vec![device.to_owned()],
                    id: share.to_owned(),
                    library: library.to_owned(),
                    synced: false,
                }),
            }
            config.offers.retain(|offer| offer.share != share || offer.device != device);
            Ok(())
        })?;
        self.inner.sync_device(device);
        Ok(())
    }

    /// Declines an offer, which then stays out of sight until the owner accepts it after all.
    pub fn decline(&self, share: &str, device: &str) -> Result<(), Error> {
        self.inner.update(|config| {
            for offer in &mut config.offers {
                if offer.share == share && offer.device == device {
                    offer.declined = true;
                }
            }
            Ok(())
        })
    }

    /// Stops syncing a library that the device no longer holds.
    pub fn forget_library(&self, library: &str) -> Result<(), Error> {
        self.inner.update(|config| {
            config.shares.retain(|share| share.library != library);
            Ok(())
        })
    }

    pub fn set_automatic(&self, automatic: bool) -> Result<(), Error> {
        self.inner.update(|config| {
            config.automatic = automatic;
            Ok(())
        })?;
        if automatic {
            self.sync_now();
        }
        Ok(())
    }

    /// Sets the address of the relay server to use instead of the public ones, from the next
    /// start.
    pub fn set_relay(&self, relay: Option<&str>) -> Result<(), Error> {
        let relay = match relay.map(str::trim).filter(|relay| !relay.is_empty()) {
            Some(relay) => {
                let url: RelayUrl =
                    relay.parse().map_err(|_| Error::InvalidRelay(relay.to_owned()))?;
                Some(url.to_string())
            }
            None => None,
        };
        self.inner.update(|config| {
            config.relay = relay;
            Ok(())
        })
    }

    /// Syncs with every trusted device now, whether or not sync is automatic.
    pub fn sync_now(&self) {
        let devices: Vec<String> =
            self.inner.config().devices.iter().map(|device| device.address.clone()).collect();
        for device in devices {
            self.inner.sync_device(&device);
        }
    }

    /// Reports a change to `library`, which automatic sync sends to the devices that share it.
    pub fn changed(&self, library: &str) {
        self.inner.pending().insert(library.to_owned());
        self.inner.wake.notify_one();
    }

    /// Reports that the network changed or the system woke, so devices that were out of reach may
    /// answer now. Automatic sync then syncs with every device.
    pub fn reconnect(&self) {
        self.inner.reconnect.notify_one();
    }

    #[cfg(test)]
    fn sockets(&self) -> Vec<SocketAddr> {
        self.inner.endpoint.bound_sockets()
    }
}

impl Drop for Syncer {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        let endpoint = self.inner.endpoint.clone();
        runtime.block_on(async {
            let _ = timeout(Duration::from_secs(2), endpoint.close()).await;
        });
        // A host call can wait for the thread that drops the syncer, so waiting for it would never
        // end.
        runtime.shutdown_timeout(Duration::from_secs(1));
    }
}

struct Inner {
    config: Mutex<Config>,
    dir: PathBuf,
    endpoint: Endpoint,
    handle: Handle,
    host: Arc<dyn Host>,
    pairing: Mutex<pairing::Pairing>,
    /// The libraries that changed since automatic sync last ran.
    pending: Mutex<HashSet<String>>,
    reconnect: Notify,
    statuses: Mutex<HashMap<String, Entry>>,
    wake: Notify,
}

#[derive(Default)]
struct Entry {
    /// Whether another sync should follow the one running, because something changed during it.
    again: bool,
    /// How many syncs this device started with the device failed in a row.
    failures: u32,
    retry: Option<AbortHandle>,
    status: Status,
}

impl Entry {
    fn cancel_retry(&mut self) {
        if let Some(retry) = self.retry.take() {
            retry.abort();
        }
    }
}

impl Inner {
    fn config(&self) -> MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pending(&self) -> MutexGuard<'_, HashSet<String>> {
        self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn statuses(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.statuses.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Changes the configuration and saves it, keeping the old one if `change` fails.
    fn update(&self, change: impl FnOnce(&mut Config) -> Result<(), Error>) -> Result<(), Error> {
        {
            let mut config = self.config();
            let mut changed = config.clone();
            change(&mut changed)?;
            if changed == *config {
                return Ok(());
            }
            save_config(&self.dir, &changed)?;
            *config = changed;
        }
        self.host.changed();
        Ok(())
    }

    /// Trusts the device at `address` under `name`, with `direct` addresses to try first.
    fn trust(&self, address: &str, name: &str, direct: Vec<SocketAddr>) -> Result<(), Error> {
        self.update(|config| {
            match config.devices.iter_mut().find(|device| device.address == address) {
                Some(device) => {
                    device.direct = direct;
                    device.name = name.to_owned();
                }
                None => config.devices.push(Device {
                    address: address.to_owned(),
                    direct,
                    name: name.to_owned(),
                }),
            }
            Ok(())
        })
    }

    fn share(&self, library: &str, device: &str) -> Result<(), Error> {
        self.update(|config| {
            if config.device(device).is_none() {
                return Err(Error::NoSuchDevice(device.to_owned()));
            }
            match config.shares.iter_mut().find(|share| share.library == library) {
                Some(share) if share.devices.iter().any(|d| d == device) => {}
                Some(share) => share.devices.push(device.to_owned()),
                None => config.shares.push(Share {
                    devices: vec![device.to_owned()],
                    id: NoteId::generate().to_string(),
                    library: library.to_owned(),
                    synced: false,
                }),
            }
            Ok(())
        })
    }

    /// Starts a sync with `device`, or asks the one running to go again.
    fn sync_device(self: &Arc<Self>, device: &str) {
        {
            let mut statuses = self.statuses();
            let entry = statuses.entry(device.to_owned()).or_default();
            if entry.status.syncing {
                entry.again = true;
                return;
            }
            entry.status.syncing = true;
            entry.cancel_retry();
        }
        self.host.changed();
        let inner = self.clone();
        let device = device.to_owned();
        self.handle.spawn(async move {
            loop {
                let result = match timeout(EXCHANGE_TIMEOUT, initiate(&inner, &device)).await {
                    Ok(result) => result,
                    Err(_) => Err(Error::Sync("the device took too long to answer".to_owned())),
                };
                let retry = result.is_err() && {
                    let config = inner.config();
                    config.automatic && config.device(&device).is_some()
                };
                let again = {
                    let mut statuses = inner.statuses();
                    let entry = statuses.entry(device.clone()).or_default();
                    entry.failures = if result.is_ok() { 0 } else { entry.failures + 1 };
                    record(&mut entry.status, result);
                    let again = std::mem::take(&mut entry.again);
                    entry.status.syncing = again;
                    if retry && !again {
                        entry.retry = Some(inner.retry(&device, entry.failures));
                    }
                    again
                };
                inner.host.changed();
                if !again {
                    break;
                }
            }
        });
    }

    /// Syncs with `device` again after a delay that doubles with each failure in a row, so a change
    /// that it missed while out of reach arrives soon after it's back.
    fn retry(self: &Arc<Self>, device: &str, failures: u32) -> AbortHandle {
        let doublings = failures.saturating_sub(1).min(16);
        let delay = MIN_RETRY.saturating_mul(1 << doublings).min(MAX_RETRY);
        let inner = self.clone();
        let device = device.to_owned();
        self.handle
            .spawn(async move {
                sleep(delay).await;
                let trusted = {
                    let config = inner.config();
                    config.automatic && config.device(&device).is_some()
                };
                if trusted {
                    inner.sync_device(&device);
                }
            })
            .abort_handle()
    }

    /// Notes offers from `device` for shares that this device doesn't sync with it.
    fn record_offers(&self, device: &str, shares: impl IntoIterator<Item = String>) {
        let _ = self.update(|config| {
            for share in shares {
                let syncing = config.shares_with(device).any(|s| s.id == share);
                let known = config.offers.iter().any(|o| o.share == share && o.device == device);
                if !syncing && !known {
                    config.offers.push(Offer { declined: false, device: device.to_owned(), share });
                }
            }
            Ok(())
        });
    }

    fn mark_synced(&self, shares: &[String]) {
        let _ = self.update(|config| {
            for share in config.shares.iter_mut().filter(|share| shares.contains(&share.id)) {
                share.synced = true;
            }
            Ok(())
        });
    }

    /// Calls the host on a thread where it may block.
    async fn call<T: Send + 'static>(
        &self,
        call: impl FnOnce(&dyn Host) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, Error> {
        let host = self.host.clone();
        tokio::task::spawn_blocking(move || call(&*host)).await.map_err(sync_error)?
    }
}

fn record(status: &mut Status, result: Result<(), Error>) {
    match result {
        Ok(()) => {
            status.error = None;
            status.synced = Some(now());
        }
        Err(error) => status.error = Some(error.to_string()),
    }
}

#[derive(Deserialize, Serialize)]
enum Message {
    Hello { shares: Vec<Version> },
    Reply { offers: Vec<String>, shares: Vec<Changes> },
    Changes { shares: Vec<Updates> },
    Done,
}

#[derive(Deserialize, Serialize)]
struct Version {
    share: String,
    version: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
struct Changes {
    share: String,
    updates: Option<Vec<u8>>,
    version: Vec<u8>,
}

#[derive(Deserialize, Serialize)]
struct Updates {
    share: String,
    updates: Option<Vec<u8>>,
}

/// A share as one sync sees it: its identifier, its library, and whether it has synced before.
type Shared = (String, String, bool);

fn shared_with(config: &Config, device: &str) -> Vec<Shared> {
    config
        .shares_with(device)
        .map(|share| (share.id.clone(), share.library.clone(), share.synced))
        .collect()
}

/// Syncs with `device` over a connection this device opens.
async fn initiate(inner: &Arc<Inner>, device: &str) -> Result<(), Error> {
    let (target, shares) = {
        let config = inner.config();
        let target = config.device(device).cloned().ok_or(Error::NoSuchDevice(device.into()))?;
        (target, shared_with(&config, device))
    };
    let id: EndpointId = target.address.parse().map_err(sync_error)?;
    let mut addr = EndpointAddr::new(id);
    for socket in &target.direct {
        addr = addr.with_ip_addr(*socket);
    }
    let connection = timeout(CONNECT_TIMEOUT, inner.endpoint.connect(addr, ALPN))
        .await
        .map_err(|_| Error::Sync("the device didn't answer".to_owned()))?
        .map_err(sync_error)?;
    let result = exchange(inner, &connection, device, &shares).await;
    if let Some(ConnectionError::ApplicationClosed(close)) = connection.close_reason()
        && close.error_code == REFUSED.into()
    {
        return Err(Error::Sync("the device hasn't added this one's address yet".to_owned()));
    }
    result?;
    connection.close(0u32.into(), b"done");
    Ok(())
}

async fn exchange(
    inner: &Arc<Inner>,
    connection: &Connection,
    device: &str,
    shares: &[Shared],
) -> Result<(), Error> {
    let (mut send, mut recv) = connection.open_bi().await.map_err(sync_error)?;
    let mut hello = Vec::new();
    for (share, library, _) in shares {
        let library = library.clone();
        let version = inner.call(move |host| host.version(&library)).await?;
        hello.push(Version { share: share.clone(), version });
    }
    write(&mut send, &Message::Hello { shares: hello }).await?;
    let Message::Reply { offers, shares: replies } = read(&mut recv).await? else {
        return Err(unexpected());
    };
    inner.record_offers(device, offers);
    let mut changes = Vec::new();
    let mut synced = Vec::new();
    for reply in replies {
        let Some((share, library, done)) = shares.iter().find(|(id, ..)| *id == reply.share) else {
            continue;
        };
        if let Some(updates) = reply.updates {
            let (library, repair) = (library.clone(), !done);
            inner.call(move |host| host.merge_updates(&library, &updates, repair)).await?;
        }
        let library = library.clone();
        let updates = inner.call(move |host| host.updates_since(&library, &reply.version)).await?;
        changes.push(Updates { share: share.clone(), updates });
        synced.push(share.clone());
    }
    write(&mut send, &Message::Changes { shares: changes }).await?;
    send.finish().map_err(sync_error)?;
    let Message::Done = read(&mut recv).await? else {
        return Err(unexpected());
    };
    inner.mark_synced(&synced);
    Ok(())
}

/// Accepts connections for as long as the endpoint stays open.
async fn accept(inner: Arc<Inner>) {
    while let Some(incoming) = inner.endpoint.accept().await {
        let inner = inner.clone();
        tokio::spawn(async move {
            let Ok(connection) = incoming.await else {
                return;
            };
            if connection.alpn() == PAIR_ALPN {
                pairing::respond(&inner, &connection).await;
                return;
            }
            let device = connection.remote_id().to_string();
            if inner.config().device(&device).is_none() {
                connection.close(REFUSED.into(), b"unknown device");
                return;
            }
            let result =
                match timeout(EXCHANGE_TIMEOUT, respond(&inner, &connection, &device)).await {
                    Ok(result) => result,
                    Err(_) => Err(Error::Sync("the device took too long to answer".to_owned())),
                };
            {
                let mut statuses = inner.statuses();
                let entry = statuses.entry(device).or_default();
                // The exchange carried this device's changes too, so a retry has nothing to add.
                if result.is_ok() {
                    entry.failures = 0;
                    entry.cancel_retry();
                }
                record(&mut entry.status, result);
            }
            inner.host.changed();
        });
    }
}

/// Syncs with `device` over a connection it opened.
async fn respond(inner: &Arc<Inner>, connection: &Connection, device: &str) -> Result<(), Error> {
    let (mut send, mut recv) = connection.accept_bi().await.map_err(sync_error)?;
    let Message::Hello { shares: hellos } = read(&mut recv).await? else {
        return Err(unexpected());
    };
    let shares = shared_with(&inner.config(), device);
    let offered: Vec<String> = hellos
        .iter()
        .filter(|hello| !shares.iter().any(|(id, ..)| *id == hello.share))
        .map(|hello| hello.share.clone())
        .collect();
    inner.record_offers(device, offered);
    let offers = shares
        .iter()
        .filter(|(id, ..)| !hellos.iter().any(|hello| hello.share == *id))
        .map(|(id, ..)| id.clone())
        .collect();
    let mut replies = Vec::new();
    for hello in hellos {
        let Some((share, library, _)) = shares.iter().find(|(id, ..)| *id == hello.share) else {
            continue;
        };
        let library = library.clone();
        let (version, updates) = inner
            .call(move |host| {
                Ok((host.version(&library)?, host.updates_since(&library, &hello.version)?))
            })
            .await?;
        replies.push(Changes { share: share.clone(), updates, version });
    }
    let replied: Vec<String> = replies.iter().map(|reply| reply.share.clone()).collect();
    write(&mut send, &Message::Reply { offers, shares: replies }).await?;
    let Message::Changes { shares: changes } = read(&mut recv).await? else {
        return Err(unexpected());
    };
    for change in changes {
        if !replied.contains(&change.share) {
            continue;
        }
        let Some((_, library, done)) = shares.iter().find(|(id, ..)| *id == change.share) else {
            continue;
        };
        if let Some(updates) = change.updates {
            let (library, repair) = (library.clone(), !done);
            inner.call(move |host| host.merge_updates(&library, &updates, repair)).await?;
        }
    }
    write(&mut send, &Message::Done).await?;
    send.finish().map_err(sync_error)?;
    inner.mark_synced(&replied);
    let _ = timeout(CONNECT_TIMEOUT, connection.closed()).await;
    Ok(())
}

/// Runs automatic sync: with every device at the start, shortly after a reconnect, and every
/// `PERIOD`, and with the devices that share a library shortly after it changes.
async fn schedule(inner: Arc<Inner>) {
    let mut everyone = true;
    loop {
        let devices: Vec<String> = {
            let config = inner.config();
            let changed = std::mem::take(&mut *inner.pending());
            if !config.automatic {
                Vec::new()
            } else if everyone {
                config.devices.iter().map(|device| device.address.clone()).collect()
            } else {
                let mut devices: Vec<String> = config
                    .shares
                    .iter()
                    .filter(|share| changed.contains(&share.library))
                    .flat_map(|share| share.devices.iter().cloned())
                    .collect();
                devices.sort();
                devices.dedup();
                devices
            }
        };
        for device in devices {
            inner.sync_device(&device);
        }
        everyone = tokio::select! {
            () = inner.wake.notified() => {
                sleep(DEBOUNCE).await;
                false
            }
            () = inner.reconnect.notified() => {
                // A network change arrives as a burst of events while the interfaces settle.
                sleep(DEBOUNCE).await;
                inner.endpoint.network_change().await;
                true
            }
            () = sleep(PERIOD) => true,
        };
    }
}

async fn bind(key: SecretKey, config: &Config, network: Network) -> Result<Endpoint, Error> {
    let builder = match network {
        Network::Internet => {
            let builder = Endpoint::builder(presets::N0);
            match &config.relay {
                Some(relay) => {
                    let url: RelayUrl =
                        relay.parse().map_err(|_| Error::InvalidRelay(relay.clone()))?;
                    builder.relay_mode(RelayMode::Custom(url.into()))
                }
                None => builder,
            }
        }
        #[cfg(test)]
        Network::Loopback => Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
            .clear_ip_transports()
            .bind_addr("127.0.0.1:0")
            .map_err(sync_error)?,
    };
    builder
        .alpns(vec![ALPN.to_vec(), PAIR_ALPN.to_vec()])
        .secret_key(key)
        .bind()
        .await
        .map_err(sync_error)
}

async fn write(send: &mut SendStream, message: &impl Serialize) -> Result<(), Error> {
    let bytes = postcard::to_stdvec(message).map_err(sync_error)?;
    let length = u32::try_from(bytes.len()).map_err(sync_error)?;
    send.write_all(&length.to_le_bytes()).await.map_err(sync_error)?;
    send.write_all(&bytes).await.map_err(sync_error)
}

async fn read<T: DeserializeOwned>(recv: &mut RecvStream) -> Result<T, Error> {
    let mut header = [0; 4];
    recv.read_exact(&mut header).await.map_err(sync_error)?;
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_MESSAGE {
        return Err(Error::Sync("the device sent too much at once".to_owned()));
    }
    let mut bytes = vec![0; length];
    recv.read_exact(&mut bytes).await.map_err(sync_error)?;
    postcard::from_bytes(&bytes).map_err(sync_error)
}

fn parse_address(address: &str) -> Result<(String, Vec<SocketAddr>), Error> {
    let invalid = || Error::InvalidAddress(address.to_owned());
    let (key, direct) = match address.trim().split_once('@') {
        Some((key, direct)) => (key, Some(direct)),
        None => (address.trim(), None),
    };
    // Devices show addresses in groups, which owners may copy with the spaces between them.
    let key: String = key.chars().filter(|c| !c.is_whitespace()).collect();
    let key: EndpointId = key.parse().map_err(|_| invalid())?;
    let direct = match direct {
        Some(direct) => direct
            .split(',')
            .map(|socket| socket.trim().parse().map_err(|_| invalid()))
            .collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    Ok((key.to_string(), direct))
}

fn load_config(dir: &Path) -> Result<Config, Error> {
    match fs::read(dir.join(CONFIG)) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(sync_error),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Config::default()),
        Err(error) => Err(error.into()),
    }
}

fn save_config(dir: &Path, config: &Config) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(config).map_err(sync_error)?;
    replace(dir, CONFIG, &bytes)
}

fn load_key(dir: &Path) -> Result<SecretKey, Error> {
    match fs::read(dir.join(KEY)) {
        Ok(bytes) => {
            let bytes: [u8; 32] = bytes
                .try_into()
                .map_err(|_| Error::Sync(format!("{} is corrupt", dir.join(KEY).display())))?;
            Ok(SecretKey::from_bytes(&bytes))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let key = SecretKey::generate();
            replace(dir, KEY, &key.to_bytes())?;
            Ok(key)
        }
        Err(error) => Err(error.into()),
    }
}

/// Writes `bytes` to the file `name` in `dir` all at once, readable by its owner alone.
fn replace(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let temporary = dir.join(format!("{name}.new"));
    let mut options = File::options();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, dir.join(name))?;
    Ok(())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as i64)
}

fn unexpected() -> Error {
    Error::Sync("the device broke the sync protocol".to_owned())
}

fn sync_error(error: impl ToString) -> Error {
    Error::Sync(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::Spaces;

    struct TestHost(Mutex<Spaces>);

    impl TestHost {
        fn spaces(&self) -> MutexGuard<'_, Spaces> {
            self.0.lock().unwrap()
        }
    }

    impl Host for TestHost {
        fn version(&self, library: &str) -> Result<Vec<u8>, Error> {
            let spaces = self.spaces();
            Ok(spaces.library(library).ok_or(Error::NoSuchLibrary(library.into()))?.version())
        }

        fn updates_since(&self, library: &str, version: &[u8]) -> Result<Option<Vec<u8>>, Error> {
            let spaces = self.spaces();
            spaces
                .library(library)
                .ok_or(Error::NoSuchLibrary(library.into()))?
                .updates_since(version)
        }

        fn merge_updates(&self, library: &str, updates: &[u8], repair: bool) -> Result<(), Error> {
            self.spaces().library_mut(library)?.merge_updates(updates, repair)
        }

        fn changed(&self) {}
    }

    struct Side {
        _dir: tempfile::TempDir,
        host: Arc<TestHost>,
        syncer: Syncer,
    }

    fn side(library: &str) -> Side {
        let dir = tempfile::tempdir().unwrap();
        let host = Arc::new(TestHost(Mutex::new(Spaces::in_memory(&[library]))));
        let syncer = Syncer::start_on(dir.path(), host.clone(), Network::Loopback).unwrap();
        Side { _dir: dir, host, syncer }
    }

    fn address(side: &Side) -> String {
        let sockets: Vec<String> = side.syncer.sockets().iter().map(|s| s.to_string()).collect();
        format!("{}@{}", side.syncer.address(), sockets.join(","))
    }

    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(start.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn bodies(side: &Side, library: &str) -> Vec<String> {
        let spaces = side.host.spaces();
        let mut bodies: Vec<String> =
            spaces.library(library).unwrap().notes().into_iter().map(|note| note.body).collect();
        bodies.sort();
        bodies
    }

    #[test]
    fn syncs_a_shared_library_both_ways() {
        let a = side("a");
        let b = side("b");
        a.host.spaces().create_memo("a", "from a").unwrap();
        b.host.spaces().create_memo("b", "already on b").unwrap();
        let b_key = a.syncer.add_device(&address(&b), "B").unwrap();
        let a_key = b.syncer.add_device(&address(&a), "A").unwrap();

        a.syncer.share("a", &b_key).unwrap();
        wait_for("the offer", || b.syncer.config().open_offers().count() == 1);
        let offer = b.syncer.config().offers[0].clone();
        assert_eq!(offer.device, a_key);
        b.syncer.accept(&offer.share, &a_key, "b").unwrap();
        wait_for("the first sync", || bodies(&a, "a").len() == 2 && bodies(&b, "b").len() == 2);
        assert_eq!(bodies(&a, "a"), ["already on b", "from a"]);
        assert_eq!(bodies(&b, "b"), bodies(&a, "a"));

        b.host.spaces().create_memo("b", "later on b").unwrap();
        b.syncer.changed("b");
        wait_for("automatic sync", || bodies(&a, "a").len() == 3);
    }

    #[test]
    fn retries_a_device_that_failed() {
        let a = side("a");
        let b = side("b");
        let b_key = a.syncer.add_device(&address(&b), "B").unwrap();
        a.syncer.share("a", &b_key).unwrap();
        wait_for("the refusal", || a.syncer.status(&b_key).error.is_some());
        let a_key = a.syncer.address();
        b.syncer.inner.trust(&a_key, "A", vec![]).unwrap();
        wait_for("the retry", || a.syncer.status(&b_key).error.is_none());
        assert_eq!(b.syncer.config().open_offers().count(), 1);
    }

    #[test]
    fn ignores_unknown_devices() {
        let a = side("a");
        let b = side("b");
        a.host.spaces().create_memo("a", "private").unwrap();
        let b_key = a.syncer.add_device(&address(&b), "B").unwrap();
        a.syncer.share("a", &b_key).unwrap();
        wait_for("the refusal", || a.syncer.status(&b_key).error.is_some());
        let error = a.syncer.status(&b_key).error.unwrap();
        assert!(error.contains("hasn't added"), "{error}");
        assert!(b.syncer.config().offers.is_empty());
        assert!(bodies(&b, "b").is_empty());
    }

    #[test]
    fn parses_addresses() {
        let key = SecretKey::generate().public().to_string();
        assert_eq!(parse_address(&key).unwrap(), (key.clone(), vec![]));
        let grouped = format!("{} {}", &key[..8], &key[8..]);
        assert_eq!(parse_address(&grouped).unwrap().0, key);
        let (_, direct) = parse_address(&format!(" {key}@127.0.0.1:4, [::1]:5 ")).unwrap();
        assert_eq!(direct, ["127.0.0.1:4".parse().unwrap(), "[::1]:5".parse().unwrap()]);
        assert!(parse_address("nonsense").is_err());
    }

    #[test]
    fn keeps_its_key_and_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_key(dir.path()).unwrap();
        assert_eq!(load_key(dir.path()).unwrap().public(), first.public());
        let config = Config { automatic: false, ..Config::default() };
        save_config(dir.path(), &config).unwrap();
        assert_eq!(load_config(dir.path()).unwrap(), config);
    }

    /// The address that reaches `side` over loopback, which tests use instead of a lookup.
    fn endpoint_addr(side: &Side) -> EndpointAddr {
        let mut addr = EndpointAddr::new(side.syncer.inner.endpoint.id());
        for socket in side.syncer.sockets() {
            addr = addr.with_ip_addr(socket);
        }
        addr
    }

    fn trusts(side: &Side, other: &Side) -> bool {
        side.syncer.config().device(&other.syncer.address()).is_some()
    }

    #[test]
    fn pairs_by_invite_and_offers_its_libraries() {
        let a = side("a");
        let b = side("b");
        a.syncer.set_name("Laptop").unwrap();
        b.syncer.set_name("Phone").unwrap();
        a.host.spaces().create_memo("a", "from a").unwrap();
        let link = a.syncer.invite(&["a".to_owned()]).unwrap();
        assert!(Syncer::is_invite(&link));
        let secret = link.rsplit('/').next().unwrap().to_owned();
        b.syncer.inner.pair(endpoint_addr(&a), Some(secret.clone()));
        wait_for("the pairing", || trusts(&a, &b) && trusts(&b, &a));
        assert_eq!(a.syncer.config().device(&b.syncer.address()).unwrap().name, "Phone");
        assert_eq!(b.syncer.config().device(&a.syncer.address()).unwrap().name, "Laptop");
        wait_for("the offer", || b.syncer.config().open_offers().count() == 1);

        let c = side("c");
        c.syncer.inner.pair(endpoint_addr(&a), Some(secret));
        wait_for("the refusal", || {
            matches!(c.syncer.outgoing().map(|out| out.state), Some(OutgoingState::Failed(_)))
        });
        assert!(!trusts(&a, &c));
    }

    #[test]
    fn pairs_with_a_nearby_device_once_its_owner_accepts() {
        let a = side("a");
        let b = side("b");
        b.syncer.set_name("Work laptop").unwrap();
        b.syncer.listen_quietly();
        a.syncer.listen_quietly();
        let direct = b.syncer.sockets();
        let address = b.syncer.address();
        a.syncer.hear(Nearby { address: address.clone(), direct, name: "Work laptop".to_owned() });
        assert_eq!(a.syncer.nearby().len(), 1);
        a.syncer.pair_nearby(&address).unwrap();
        wait_for("the request", || b.syncer.requests().len() == 1);
        let request = b.syncer.requests()[0].clone();
        assert_eq!(Some(request.code.clone()), a.syncer.outgoing().unwrap().code);
        b.syncer.answer(&request.address, true);
        wait_for("the pairing", || {
            a.syncer.outgoing().map(|out| out.state)
                == Some(OutgoingState::Paired("Work laptop".to_owned()))
        });
        assert!(trusts(&b, &a));
        assert!(a.syncer.nearby().is_empty());
    }

    #[test]
    fn refuses_requests_while_the_sync_screen_is_closed() {
        let a = side("a");
        let b = side("b");
        a.syncer.inner.pair(endpoint_addr(&b), None);
        wait_for("the refusal", || {
            matches!(a.syncer.outgoing().map(|out| out.state), Some(OutgoingState::Failed(_)))
        });
        assert!(b.syncer.requests().is_empty());
        assert!(!trusts(&a, &b) && !trusts(&b, &a));
    }

    #[test]
    fn computes_the_same_code_on_both_devices() {
        let a = SecretKey::generate().public().to_string();
        let b = SecretKey::generate().public().to_string();
        let code = pairing_code(&a, &b);
        assert_eq!(code, pairing_code(&b, &a));
        assert_eq!(code.len(), 6);
        assert!(code.bytes().all(|byte| byte.is_ascii_digit()));
    }
}
