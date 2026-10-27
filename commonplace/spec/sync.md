# Sync

Version 1. This spec defines how two devices sync Commonplace libraries, so that every
implementation can sync with every other. [Decision 0013](../../decisions/0013-commonplace-sync.md)
explains the choice of transport.

## Devices

Each device has an Ed25519 key pair, which it makes the first time it syncs and never shares. Its
address is the public key, written as iroh writes an endpoint identifier: 64 lowercase hexadecimal
characters. The device keeps the secret key in its own store, outside every library, and keeps it
out of any backup that leaves the device.

The owner adds another device by entering its address. An address may end with `@` and a
comma-separated list of socket addresses, such as `@192.168.1.20:41641`, which the device tries
before it looks the address up. A device syncs only with devices whose addresses its owner added,
and closes any other connection once the handshake names the peer.

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
