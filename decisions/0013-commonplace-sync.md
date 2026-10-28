# 0013 Commonplace sync

## Context

[Decision 0002](0002-commonplace-library-storage.md) stores each library as a Loro document and
leaves the transport that carries its updates between devices to a later decision.
[Decision 0007](0007-commonplace-spaces.md) requires that devices pair each library on its own, that
a library holds no identifier or name, and that the work machine never receives the Personal
library.

The owner runs Commonplace on Linux machines and an Android phone. Any two of them should sync, at
home on one network or over the internet. A laptop sits behind a home router, and a phone on mobile
data sits behind carrier-grade Network Address Translation (NAT), so often neither device can accept
a connection from the other. Some path then has to forward traffic between them, and anything on
that path must learn nothing from it.

The owner wants to pair devices by entering their addresses, and to sync both by hand and on its own
in the background.

## Criteria

- **Private.** Encryption and authentication cover every byte between devices, end to end. Nothing
  on the path, a relay included, can read or change a library, and only devices the owner paired can
  sync.
- **Reachable.** Two devices sync whether they share a network or sit behind NAT on different ones,
  phones on mobile data included.
- **No server to run.** Sync works without a server that the owner has to set up or keep running,
  and the owner can still use their own instead of a public one.
- **Per-library pairing.** Each library syncs with the devices its owner chose for it, and with no
  others, as decision 0007 requires.
- **Both platforms.** One implementation in the Rust core serves Linux and Android.
- **Simple pairing.** Pairing two devices takes no typing. The owner taps a nearby device, scans a
  code, or opens a link on one device, then confirms on the other if it asks.

## Decision

Sync over [iroh], a Rust library that dials encrypted connections by public key. Each implementation
runs the transport in the core, behind a `sync` feature, and the
[sync spec](../commonplace/spec/sync.md) defines the protocol.

- **Device identity.** Each device makes an Ed25519 key pair the first time it syncs and keeps the
  secret key in its own store, outside every library. The public key is the device's address. Each
  device has a name of its own, which devices that pair with it learn, and the owner can rename the
  devices it knows, as they rename libraries.
- **Encryption.** iroh runs Transport Layer Security (TLS) 1.3 inside each connection, keyed by the
  two devices' key pairs. Each side proves it holds the secret key for its address, so a connection
  reaches the device the owner entered, and nothing else.
- **Trust.** A device syncs only with devices it trusts, and closes any connection from another.
  Pairing makes two devices trust each other. It works in three ways.
  - **Nearby.** While the owner has the Sync screen open, the device announces its address and name
    on the local network with multicast Domain Name System (DNS), and lists the other devices that
    do the same. Picking one sends it a pairing request. Both devices show a six-digit code, derived
    from both addresses, and the owner accepts the request on the other device once the codes match.
    A device ignores requests from devices it doesn't trust, unless its Sync screen is open.
  - **Invite.** A device makes an invite: a link that holds its address and a random secret, which
    works once, for 15 minutes. The device shows the link as a QR code for a phone to scan, and as
    text to copy to another device and open there. The device that opens it sends the secret, and
    the device that made it trusts it without asking. An invite can name libraries to share with the
    device that uses it.
  - **Address.** The owner enters another device's address by hand on each device, for when neither
    of the others works.
- **Shares.** Sharing a library with a device gives the library a share identifier, a random value
  that only the devices' own stores hold. The next connection offers the share to the other device,
  and its owner accepts it into a new library or an existing one, or declines it. A device can share
  the same library with more devices, which then sync under the same share identifier. This follows
  the folder offers of Syncthing, and keeps the libraries themselves free of identifiers, as
  decision 0007 requires.
- **Protocol.** A sync is one exchange per connection: each side sends its version vector for every
  share the two devices have in common, and the changes that the other side lacks. Each side checks
  the changes in a copy of the library before it merges them, so changes from a newer version of the
  format, or from a document that isn't a library, never reach the library.
- **First sync.** The first sync of a share on a device merges two libraries that may not share any
  history, as merging libraries does in decision 0007, so it repairs notes that the merge lost in
  the same way.
- **Connections.** iroh connects through a relay server first, then tries a direct path with hole
  punching, and moves the connection there once it works. When the devices can't reach each other
  directly, the relay keeps forwarding the encrypted packets. Most connections end up direct.
- **Relays.** By default, the transport uses the public relays and address lookup of n0, the company
  behind iroh, which cost nothing and need no account. A relay learns which addresses connect to
  each other, their IP addresses, and how much traffic passes, but no content. The owner can set the
  address of their own relay, which runs iroh's relay server, to keep even that private.
- **Automatic sync.** While a device runs Commonplace, it accepts connections from the devices it
  trusts. With automatic sync on, it also syncs with every device that shares a library with it
  shortly after each local change, when it starts, and every 15 minutes. With automatic sync off, it
  syncs only when the owner asks, but still answers devices that connect to it.
- **Missed changes.** A change reaches another device only if the sync after it succeeds, so a
  device that was asleep or offline misses it. Automatic sync retries a device after a failed sync,
  first after 30 seconds and then after twice the last wait, up to 10 minutes, until a sync in
  either direction succeeds. It also syncs with every device shortly after the network changes, and
  on Linux, after the system wakes from sleep. The 15-minute sync catches anything else.
- **Android.** The Android app syncs while it runs, and with automatic sync on, a periodic
  background job syncs at the interval Android allows, at least every 15 minutes.

Against the criteria:

- **Private:** each connection encrypts and authenticates traffic with TLS 1.3 and the devices'
  keys. Relays forward packets they can't decrypt. Trust and shares limit syncing to paired devices
  and chosen libraries.
- **Reachable:** relays reach every device that can make an outgoing connection, and hole punching
  makes most connections direct.
- **No server to run:** n0's public relays work out of the box, and the owner can switch to their
  own.
- **Per-library pairing:** a device syncs a library only under a share that the owner of each device
  accepted for that device.
- **Both platforms:** iroh is a Rust library that builds for Linux and Android, and the core exposes
  it to Kotlin through the existing bindings.
- **Simple pairing:** devices on one network pair with a tap and a glance at two codes, a phone
  pairs by scanning a code, and other devices pair by opening a link. Accepting a share takes a
  click.

## Rejected alternatives

- **Plain TCP connections to a host and port, encrypted with the Noise protocol.** It's small and
  private, but a device behind NAT can't accept a connection, so a phone on mobile data could never
  sync. It fails the reachable criterion, unless the project builds its own relay and hole punching,
  which is the part iroh already provides.
- **A file sync tool, such as Syncthing, carrying update files.** Loro updates merge in any order,
  so a shared folder of update files would work. But it needs another app on every device, the
  Syncthing project discontinued its Android app, and a shared folder shares every library in it. It
  fails the both platforms and per-library pairing criteria.
- **A virtual private network, such as Tailscale or WireGuard, carrying plain connections.** It
  solves reaching devices and encryption, but the owner has to run it on every device, and Android
  allows one virtual private network at a time. It fails the no server to run criterion.
- **A server that stores encrypted updates for devices to fetch.** It lets two devices sync without
  ever running at the same time, but someone has to run the server. It fails the no server to run
  criterion as the only path. A device that stays on, as described below, gives the same benefit
  without a new kind of server.
- **libp2p.** It also offers hole punching and relays, but it's a larger framework with more parts
  to configure, and its relays need more setup for the same result. iroh does the one job this
  decision needs.

## Consequences

- The core gains iroh and the Tokio runtime as optional dependencies, which the Linux app and the
  Android bindings enable. They add a few megabytes to each build.
- Two devices sync only while both run at the same time. A device that stays on, such as a desktop
  or a small server, can hold every shared library and pass changes between devices that rarely run
  together. A command-line daemon for that role is follow-on work.
- Each implementation needs screens to pair and name devices, share libraries, accept offers, switch
  automatic sync, sync by hand, and set a relay. Android needs a camera to scan invites, and holds a
  multicast lock while its Sync screen is open, so it can hear nearby devices.
- Nearby devices on the local network learn the device's name and address while its Sync screen is
  open, and never otherwise. The name never goes to n0's address lookup.
- Device stores now hold a secret key. Implementations keep it out of backups that leave the device,
  as Android already does for libraries.
- Moving a note between libraries still leaves the move to race with edits on other devices, as
  decision 0007 describes. Sync makes that race more likely, so the warning stays.
- Address lookup through n0 publishes each device's current relay under its address. Anyone who
  knows the address can learn which relay the device uses, but nothing more.

[iroh]: https://www.iroh.computer/
