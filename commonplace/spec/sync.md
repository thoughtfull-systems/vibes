# Sync

Version 1. This spec defines how two devices sync Commonplace libraries, so that every
implementation can sync with every other. [Decision 0013](../../decisions/0013-commonplace-sync.md)
explains the choice of transport.

## Devices

Each device has an Ed25519 key pair, which it makes the first time it syncs and never shares. Its
address is the public key, written as iroh writes an endpoint identifier: 64 lowercase hexadecimal
characters. The device keeps the secret key in its own store, outside every library, and keeps it
out of any backup that leaves the device.

Each device also has a name, which its owner chooses and the devices it pairs with learn. A device
syncs only with devices it trusts, and closes any other connection once the handshake names the
peer. Pairing makes two devices trust each other.

## Pairing

A device pairs with another by connecting to it under the protocol name `commonplace/pair/1`, with
the framing that [connections](#connections) describe, and sending a request: its own name, and the
secret of an invite if it has one. The other device answers with its own name if it accepts, or a
refusal. Once a device accepts, each trusts the other under the name it sent, and they sync at once.

A device accepts a request in these cases:

- **The secret matches an invite.** The device made the invite in the last 15 minutes, and no other
  device used it. The device accepts without asking its owner, and shares with the new device the
  libraries that the invite names, which reach it as offers.
- **The owner accepts.** The device shows the request, with the requesting device's name and the
  code below, and waits up to 2 minutes for its owner. A device asks only while its owner has its
  Sync screen open, and refuses any other request from a device it doesn't trust.

The code is six decimal digits, which both devices show while they pair. A device computes it from
the two addresses: it hashes the two public keys, the smaller first when compared byte by byte, with
BLAKE3, reads the first 4 bytes of the hash as a big-endian integer, and takes the remainder after
dividing by 1,000,000. A device that a third device sits between would show a different code.

An invite is a link of the form `commonplace:pair/<address>/<secret>`, where the secret is 16 random
bytes written as 32 lowercase hexadecimal characters. A device shows it as text and as a QR code,
and a device that opens it sends a request with the secret.

### Nearby devices

While its owner has its Sync screen open, a device announces itself on the local network with
multicast Domain Name System (DNS), as iroh's mDNS address lookup does, under the service name
`commonplace`. The announcement carries the device's addresses on the network, and its name as the
user data. The device also lists the devices it hears, except those it trusts, for its owner to pick
one to pair with. A device never announces itself at any other time.

### Addresses

The owner can also add a device by entering its address, on each device. Devices may show an address
in groups of characters, and ignore white space in one the owner enters. An address may end with `@`
and a comma-separated list of socket addresses, such as `@192.168.1.20:41641`, which the device
tries before it looks the address up.

## Shares

A share is a library that a device syncs with other devices. Its identifier takes the form of a note
identifier, as the [links spec](links.md#note-identifiers) defines, and the device that first shares
the library makes it. Devices keep shares in their own stores. A library holds no trace of them.

A device that shares a library with another device lists the share in each sync with it. The other
device, if it doesn't sync that share with the first, records an offer, which its owner accepts into
a new or existing library, or declines. A device syncs a share with another device only once each
lists the other for that share.

The first time a share syncs on a device, the device merges the incoming changes as merging
libraries does in [decision 0007](../../decisions/0007-commonplace-spaces.md): afterward, it writes
back each note that it or the other device held before the merge and that the merge lost.

## Connections

Devices connect with [iroh], under the protocol name `commonplace/sync/1`. iroh authenticates both
devices by their keys and encrypts the connection with Transport Layer Security (TLS) 1.3. Unless
the owner sets a relay server of their own, devices use n0's public relays and address lookup.

The device that connects opens one bidirectional stream. Each message on it starts with its length,
as a 32-bit little-endian integer, then holds that many bytes, encoded with [postcard]. A message is
one of these, in this order:

- **Hello,** from the device that connects: for each share it syncs with the other device, the
  share's identifier and the library's version vector, in Loro's encoding.
- **Reply:** the identifiers of the shares it syncs with the first device that the hello didn't
  list, as offers, then, for each share in the hello that it syncs with the first device, the
  share's identifier, its own version vector, and the updates the first device lacks, if any.
- **Changes,** from the device that connects: for each share in the reply, after it merges the
  reply's updates, the identifier and the updates that the other device lacks, if any.
- **Done,** once the other device merges the changes.

The Rust core's `sync` module defines the messages' fields and their order, which postcard encoding
depends on. Updates use Loro's update encoding, as the [library format](library-format.md#document)
specifies. A device merges updates into a copy of the library first, and refuses them if the result
isn't a library or has a `version` higher than any it knows.

A message longer than 1 GiB ends the sync. A device that receives a message out of order closes the
connection.

## When devices sync

A device accepts connections while it runs. With automatic sync on, it also syncs with every device
it trusts when it starts and every 5 minutes, and with the devices that share a library 3 seconds
after a change to it. With automatic sync off, it syncs only when its owner asks.

[iroh]: https://www.iroh.computer/
[postcard]: https://postcard.jamesmunns.com/wire-format
