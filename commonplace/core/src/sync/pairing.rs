// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Pairing devices by invite, by a nearby device's request, or by hand, as the sync spec defines.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, MutexGuard};
use std::time::{Duration, Instant};

use iroh::address_lookup::AddressLookup;
use iroh::endpoint::Connection;
use iroh::endpoint_info::{EndpointData, UserData};
use iroh::{EndpointAddr, EndpointId, TransportAddr};
use iroh_mdns_address_lookup::{DiscoveryEvent, MdnsAddressLookup};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_stream::StreamExt;

use super::{CONNECT_TIMEOUT, Inner, PAIR_ALPN, Syncer, read, sync_error, unexpected, write};
use crate::Error;

const INVITE_LIFETIME: Duration = Duration::from_secs(15 * 60);
const LINK_PREFIX: &str = "commonplace:pair/";
/// How many requests may wait for the owner at once, so a noisy network can't pile them up.
const MAX_REQUESTS: usize = 4;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const SERVICE: &str = "commonplace";

/// What a device keeps about pairing while it runs, and forgets when it stops.
#[derive(Default)]
pub(super) struct Pairing {
    invites: Vec<Invite>,
    nearby: Option<Listening>,
    outgoing: Option<Outgoing>,
    requests: Vec<Waiting>,
}

struct Invite {
    expires: Instant,
    libraries: Vec<String>,
    secret: String,
}

struct Listening {
    devices: BTreeMap<String, Nearby>,
    lookup: Option<MdnsAddressLookup>,
    task: Option<JoinHandle<()>>,
}

impl Drop for Listening {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

struct Waiting {
    answer: Option<oneshot::Sender<bool>>,
    request: Request,
}

/// A device on the local network that has its Sync screen open.
#[derive(Clone, Debug, PartialEq)]
pub struct Nearby {
    pub address: String,
    pub direct: Vec<SocketAddr>,
    pub name: String,
}

/// A request to pair that waits for the owner to accept or decline it.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub address: String,
    pub code: String,
    pub name: String,
}

/// The pairing that this device asked for most recently.
#[derive(Clone, Debug, PartialEq)]
pub struct Outgoing {
    pub address: String,
    /// The code that the other device shows too, for an owner to compare. Pairing by invite needs
    /// none.
    pub code: Option<String>,
    pub state: OutgoingState,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OutgoingState {
    Waiting,
    /// The other device accepted, under the name it gave.
    Paired(String),
    Failed(String),
}

#[derive(Deserialize, Serialize)]
enum Message {
    Request { name: String, secret: Option<String> },
    Accepted { name: String },
    Refused,
}

impl Syncer {
    /// This device's name, which devices that pair with it learn.
    pub fn name(&self) -> String {
        self.inner.config().name.clone()
    }

    pub fn set_name(&self, name: &str) -> Result<(), Error> {
        let name = clean_name(name);
        self.inner.update(|config| {
            config.name = name;
            Ok(())
        })?;
        if let Some(lookup) = self.inner.pairing().nearby.as_ref().and_then(|n| n.lookup.as_ref()) {
            self.announce(lookup);
        }
        Ok(())
    }

    /// Tells the local network this device's addresses and name.
    fn announce(&self, lookup: &MdnsAddressLookup) {
        let own = self.inner.endpoint.addr();
        let mut data = EndpointData::new(own.ip_addrs().copied().map(TransportAddr::Ip).collect());
        if let Ok(name) = self.name().parse::<UserData>() {
            data = data.with_user_data(name);
        }
        lookup.publish(&data);
    }

    /// Makes an invite link that pairs the device that opens it within 15 minutes, once, and shares
    /// `libraries` with it.
    pub fn invite(&self, libraries: &[String]) -> Result<String, Error> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(sync_error)?;
        let secret: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut pairing = self.inner.pairing();
        let now = Instant::now();
        pairing.invites.retain(|invite| invite.expires > now);
        pairing.invites.push(Invite {
            expires: now + INVITE_LIFETIME,
            libraries: libraries.to_vec(),
            secret: secret.clone(),
        });
        Ok(format!("{LINK_PREFIX}{}/{secret}", self.address()))
    }

    /// Whether `text` is an invite link.
    pub fn is_invite(text: &str) -> bool {
        text.trim().starts_with(LINK_PREFIX)
    }

    /// Opens an invite link from another device, and pairs with it in the background.
    pub fn open_invite(&self, link: &str) -> Result<(), Error> {
        let invalid = || Error::InvalidInvite(link.to_owned());
        let rest = link.trim().strip_prefix(LINK_PREFIX).ok_or_else(invalid)?;
        let (address, secret) = rest.split_once('/').ok_or_else(invalid)?;
        let id: EndpointId = address.parse().map_err(|_| invalid())?;
        if id == self.inner.endpoint.id() {
            return Err(invalid());
        }
        let secret = secret.trim_end_matches('/');
        if secret.len() != 32 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        self.inner.pair(EndpointAddr::new(id), Some(secret.to_ascii_lowercase()));
        Ok(())
    }

    /// Announces this device on the local network and listens for others, until `stop_nearby`.
    /// Owners call it while the Sync screen is open, and only then can a device without an invite
    /// ask to pair.
    pub fn start_nearby(&self) -> Result<(), Error> {
        if self.inner.pairing().nearby.is_some() {
            return Ok(());
        }
        let _runtime = self.inner.handle.enter();
        let lookup = MdnsAddressLookup::builder()
            .service_name(SERVICE)
            .build(self.inner.endpoint.id())
            .map_err(sync_error)?;
        self.announce(&lookup);
        let events = self.inner.handle.block_on(lookup.subscribe());
        let task = self.inner.handle.spawn(listen(Arc::downgrade(&self.inner), events));
        self.inner.pairing().nearby =
            Some(Listening { devices: BTreeMap::new(), lookup: Some(lookup), task: Some(task) });
        Ok(())
    }

    /// Stops announcing this device, and refuses requests that wait for an answer.
    pub fn stop_nearby(&self) {
        let waiting = {
            let mut pairing = self.inner.pairing();
            pairing.nearby = None;
            std::mem::take(&mut pairing.requests)
        };
        for mut waiting in waiting {
            if let Some(answer) = waiting.answer.take() {
                let _ = answer.send(false);
            }
        }
        self.inner.host.changed();
    }

    /// The nearby devices that this device doesn't trust yet, in order of name.
    pub fn nearby(&self) -> Vec<Nearby> {
        let config = self.inner.config().clone();
        let pairing = self.inner.pairing();
        let mut nearby: Vec<Nearby> = pairing
            .nearby
            .iter()
            .flat_map(|listening| listening.devices.values())
            .filter(|device| config.device(&device.address).is_none())
            .cloned()
            .collect();
        nearby.sort_by_key(|device| device.name.to_lowercase());
        nearby
    }

    /// Asks a nearby device to pair, which its owner accepts there.
    pub fn pair_nearby(&self, address: &str) -> Result<(), Error> {
        let device = self
            .inner
            .pairing()
            .nearby
            .as_ref()
            .and_then(|listening| listening.devices.get(address).cloned())
            .ok_or_else(|| Error::NoSuchDevice(address.to_owned()))?;
        let id: EndpointId = device.address.parse().map_err(sync_error)?;
        let mut addr = EndpointAddr::new(id);
        for socket in device.direct {
            addr = addr.with_ip_addr(socket);
        }
        self.inner.pair(addr, None);
        Ok(())
    }

    /// The requests to pair that wait for the owner.
    pub fn requests(&self) -> Vec<Request> {
        self.inner.pairing().requests.iter().map(|waiting| waiting.request.clone()).collect()
    }

    /// Accepts or declines the request from `address`.
    pub fn answer(&self, address: &str, accept: bool) {
        let answer = {
            let mut pairing = self.inner.pairing();
            pairing
                .requests
                .iter_mut()
                .find(|waiting| waiting.request.address == address)
                .and_then(|waiting| waiting.answer.take())
        };
        if let Some(answer) = answer {
            let _ = answer.send(accept);
        }
    }

    pub fn outgoing(&self) -> Option<Outgoing> {
        self.inner.pairing().outgoing.clone()
    }

    /// Forgets the outgoing pairing once the owner has seen how it went.
    pub fn clear_outgoing(&self) {
        let mut pairing = self.inner.pairing();
        if pairing.outgoing.as_ref().is_some_and(|out| out.state != OutgoingState::Waiting) {
            pairing.outgoing = None;
        }
    }

    #[cfg(test)]
    pub(super) fn listen_quietly(&self) {
        self.inner.pairing().nearby =
            Some(Listening { devices: BTreeMap::new(), lookup: None, task: None });
    }

    #[cfg(test)]
    pub(super) fn hear(&self, device: Nearby) {
        if let Some(listening) = &mut self.inner.pairing().nearby {
            listening.devices.insert(device.address.clone(), device);
        }
    }
}

impl Inner {
    pub(super) fn pairing(&self) -> MutexGuard<'_, Pairing> {
        self.pairing.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_outgoing(&self, state: OutgoingState) {
        if let Some(outgoing) = &mut self.pairing().outgoing {
            outgoing.state = state;
        }
        self.host.changed();
    }

    /// Asks the device at `addr` to pair, with an invite's secret if there is one.
    pub(super) fn pair(self: &Arc<Self>, addr: EndpointAddr, secret: Option<String>) {
        let address = addr.id.to_string();
        let code =
            secret.is_none().then(|| pairing_code(&self.endpoint.id().to_string(), &address));
        self.pairing().outgoing =
            Some(Outgoing { address: address.clone(), code, state: OutgoingState::Waiting });
        self.host.changed();
        let inner = self.clone();
        self.handle.spawn(async move {
            let direct: Vec<SocketAddr> = addr.ip_addrs().copied().collect();
            let state = match ask(&inner, addr, secret).await {
                Ok(Some(name)) => match inner.trust(&address, &name, direct) {
                    Ok(()) => {
                        inner.sync_device(&address);
                        OutgoingState::Paired(name)
                    }
                    Err(error) => OutgoingState::Failed(error.to_string()),
                },
                Ok(None) => OutgoingState::Failed("The other device declined".to_owned()),
                Err(error) => OutgoingState::Failed(error.to_string()),
            };
            inner.set_outgoing(state);
        });
    }

    /// Whether an invite with `secret` is still good, and if so, uses it up and returns the
    /// libraries it shares.
    fn take_invite(&self, secret: &str) -> Option<Vec<String>> {
        let mut pairing = self.pairing();
        let now = Instant::now();
        pairing.invites.retain(|invite| invite.expires > now);
        let index = pairing.invites.iter().position(|invite| invite.secret == secret)?;
        Some(pairing.invites.remove(index).libraries)
    }
}

/// Sends a request to pair, and returns the other device's name if it accepts.
async fn ask(
    inner: &Arc<Inner>,
    addr: EndpointAddr,
    secret: Option<String>,
) -> Result<Option<String>, Error> {
    let address = addr.id.to_string();
    let connection = timeout(CONNECT_TIMEOUT, inner.endpoint.connect(addr, PAIR_ALPN))
        .await
        .map_err(|_| Error::Sync("the device didn't answer".to_owned()))?
        .map_err(sync_error)?;
    let (mut send, mut recv) = connection.open_bi().await.map_err(sync_error)?;
    let name = inner.config().name.clone();
    write(&mut send, &Message::Request { name, secret }).await?;
    send.finish().map_err(sync_error)?;
    // The owner of the other device may take as long as it waits for them.
    let reply = timeout(REQUEST_TIMEOUT + CONNECT_TIMEOUT, read(&mut recv))
        .await
        .map_err(|_| Error::Sync("the other device didn't answer in time".to_owned()))??;
    connection.close(0u32.into(), b"done");
    match reply {
        Message::Accepted { name } => Ok(Some(name_or_address(&name, &address))),
        Message::Refused => Ok(None),
        Message::Request { .. } => Err(unexpected()),
    }
}

/// Answers a request to pair, from any device.
pub(super) async fn respond(inner: &Arc<Inner>, connection: &Connection) {
    let address = connection.remote_id().to_string();
    let Ok((mut send, mut recv)) = connection.accept_bi().await else {
        return;
    };
    let Ok(Ok(Message::Request { name, secret })) = timeout(CONNECT_TIMEOUT, read(&mut recv)).await
    else {
        return;
    };
    let name = name_or_address(&name, &address);
    let accepted = match secret {
        Some(secret) => match inner.take_invite(&secret) {
            Some(libraries) => {
                let trusted = inner.trust(&address, &name, Vec::new()).is_ok();
                for library in libraries {
                    let _ = inner.share(&library, &address);
                }
                trusted
            }
            None => false,
        },
        None => ask_owner(inner, &address, &name).await,
    };
    let reply = if accepted {
        Message::Accepted { name: inner.config().name.clone() }
    } else {
        Message::Refused
    };
    if write(&mut send, &reply).await.is_ok() && send.finish().is_ok() {
        let _ = timeout(CONNECT_TIMEOUT, connection.closed()).await;
    }
    if accepted {
        inner.sync_device(&address);
    }
}

/// Shows a request from a device without an invite, if the Sync screen is open, and returns
/// whether the owner accepts it in time.
async fn ask_owner(inner: &Arc<Inner>, address: &str, name: &str) -> bool {
    let answer = {
        let mut pairing = inner.pairing();
        let full = pairing.requests.len() >= MAX_REQUESTS;
        let repeated = pairing.requests.iter().any(|waiting| waiting.request.address == address);
        if pairing.nearby.is_none() || full || repeated {
            return false;
        }
        let (sender, receiver) = oneshot::channel();
        let code = pairing_code(&inner.endpoint.id().to_string(), address);
        let request = Request { address: address.to_owned(), code, name: name.to_owned() };
        pairing.requests.push(Waiting { answer: Some(sender), request });
        receiver
    };
    inner.host.changed();
    let accepted = matches!(timeout(REQUEST_TIMEOUT, answer).await, Ok(Ok(true)));
    inner.pairing().requests.retain(|waiting| waiting.request.address != address);
    let accepted = accepted && inner.trust(address, name, Vec::new()).is_ok();
    inner.host.changed();
    accepted
}

/// Keeps the list of nearby devices up to date while the Sync screen is open.
async fn listen(
    inner: std::sync::Weak<Inner>,
    mut events: impl tokio_stream::Stream<Item = DiscoveryEvent> + Unpin,
) {
    while let Some(event) = events.next().await {
        let Some(inner) = inner.upgrade() else {
            return;
        };
        {
            let mut pairing = inner.pairing();
            let Some(listening) = &mut pairing.nearby else {
                return;
            };
            match event {
                DiscoveryEvent::Discovered { endpoint_info, .. } => {
                    let address = endpoint_info.endpoint_id.to_string();
                    let name = endpoint_info
                        .user_data()
                        .map(|name| clean_name(name.as_ref()))
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| address.chars().take(8).collect());
                    let direct = endpoint_info.ip_addrs().copied().collect();
                    listening.devices.insert(address.clone(), Nearby { address, direct, name });
                }
                DiscoveryEvent::Expired { endpoint_id } => {
                    listening.devices.remove(&endpoint_id.to_string());
                }
                _ => continue,
            }
        }
        inner.host.changed();
    }
}

/// The six-digit code that two devices show while they pair, the same on both.
pub fn pairing_code(first: &str, second: &str) -> String {
    let key = |address: &str| address.parse::<EndpointId>().map(|id| *id.as_bytes());
    let (Ok(first), Ok(second)) = (key(first), key(second)) else {
        return String::new();
    };
    let (low, high) = if first <= second { (first, second) } else { (second, first) };
    let mut hasher = blake3::Hasher::new();
    hasher.update(&low);
    hasher.update(&high);
    let hash = hasher.finalize();
    let number = u32::from_be_bytes(hash.as_bytes()[..4].try_into().unwrap());
    format!("{:06}", number % 1_000_000)
}

/// The modules of a QR code for `text`, row by row, and the number in each row.
pub fn qr_code(text: &str) -> Result<(usize, Vec<bool>), Error> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(sync_error)?;
    let width = code.width();
    let modules = code.to_colors().into_iter().map(|color| color == qrcode::Color::Dark).collect();
    Ok((width, modules))
}

/// A device's name as it sent it, or the start of its address if it sent none.
fn name_or_address(name: &str, address: &str) -> String {
    let name = clean_name(name);
    if name.is_empty() { address.chars().take(8).collect() } else { name }
}

/// A device name as it shows, on one line and short enough for the local network's records.
fn clean_name(name: &str) -> String {
    let name: String = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut end = name.len().min(UserData::MAX_LENGTH).min(64);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_owned()
}
