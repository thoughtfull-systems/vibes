# 0002 Commonplace library storage

## Context

Commonplace is a personal knowledge management app. It holds four kinds of notes: memos, actions,
curios, and zettels. They link to each other. Actions form a hierarchy, zettels form ordered
sequences with branches, and every kind can take part in spaced repetition, which records a log of
reviews.

Per [decision 0001](0001-languages-and-technologies.md), each platform gets a native app, and the
apps share a Rust core. The README requires every app to work offline and to sync between devices
without depending on a server. One person uses Commonplace, but on more than one device, so the same
note can change on a phone and a laptop before either one syncs. The library's storage format is the
contract between those apps, so it's the hardest choice in the app to reverse.

## Criteria

- **Lossless merging.** Concurrent edits on two offline devices merge without losing either side,
  down to edits in the same note's text.
- **Structure merges well.** Concurrent moves and insertions in hierarchies and ordered sequences
  merge without cycles or duplicates. The action tree and the zettel sequences depend on this.
- **Every platform.** Rust, Kotlin, TypeScript in a browser, and later Swift can all read and write
  the format, offline.
- **No server.** Sync needs nothing more than a way to move bytes between devices.
- **Durable.** The data outlives the app: the format is open and stable, and the library can export
  to plain files.

## Decision

Store each library, one person's whole collection of notes, as a single document of the [Loro]
Conflict-free Replicated Data Type (CRDT) library. The
[library format specification](../commonplace/spec/library-format.md) defines the containers and
fields inside the document.

- Note text lives in Loro text containers, which merge concurrent edits character by character.
- Hierarchies and sequences use Loro's movable tree and fractional-index ordering when the specs for
  actions and zettels add them.
- Attachments, such as PDFs and images, stay out of the document. The document refers to them by
  content hash, and a later decision defines how they're stored and synced.
- Devices sync by exchanging Loro's binary updates. A later decision defines the transport. Until
  then, each device keeps its own library.
- Each implementation decides how to keep the document on its device, as long as it can load it,
  save it, and exchange updates in Loro's encoding.

Against the criteria:

- **Lossless merging:** Loro merges concurrent edits to text, maps, and lists automatically, and
  keeps the history that makes it possible.
- **Structure merges well:** Loro's movable tree resolves concurrent moves without cycles, and its
  fractional indexes order siblings without renumbering them.
- **Every platform:** Loro is a Rust library. The Rust core uses it directly and exposes it to
  Kotlin through UniFFI and to the web through WebAssembly. Loro also publishes its own JavaScript
  and Swift packages.
- **No server:** a Loro update is a self-contained byte string, so any channel can carry it: a
  synced folder, a file copied by hand, or a relay server.
- **Durable:** Loro's encoding has been stable since version 1.0, and the code is open source under
  the MIT license. A Markdown export, planned with the spec, keeps the notes readable without the
  app.

## Rejected alternatives

- **A folder of Markdown files with front matter, synced by a file sync tool.** This was the first
  recommendation, and it scores best on the durable criterion. It fails lossless merging: when two
  devices edit the same file, a file sync tool keeps one copy and sets the other aside as a
  conflict. It also fails structure merges well, because a hierarchy spread across files has no
  merge rule, and every platform, because a browser can't open a folder on the device without a
  server or a Chromium-only interface.
- **Automerge.** Automerge merges text and maps as well as Loro does and has an official Java
  wrapper for Android, still at version 0.0.x. It has no movable tree, so it fails the structure
  merges well criterion unless the app builds its own tree on top.
- **Yjs or its Rust port, Yrs.** Yjs is mature and widely used, but it also lacks a movable tree, so
  it fails the structure merges well criterion.
- **A SQLite database with a custom sync protocol.** SQLite runs everywhere and queries fast, but
  merging rows edited on two devices would need custom rules for every table. It fails lossless
  merging and structure merges well unless the app reinvents a CRDT.
- **One Loro document per note.** Small documents load and sync independently, but moving an action
  between projects or a zettel between branches would span documents, with no atomic merge. That
  fails structure merges well, and syncing thousands of version vectors adds work. Loro 1.0 loads
  snapshots lazily, which removes most of the cost of one large document.

## Consequences

- The Rust core depends on Loro, and the Kotlin bindings include it.
- Only one process at a time may write a library on a device, so each implementation guards against
  a second instance.
- The document keeps its full editing history, so it grows over time. A later revision may trim old
  history with Loro's shallow snapshots.
- Sync transport and attachment storage each need a follow-on decision.
- Field changes follow Loro's rules for concurrent edits: the last write to a map key wins, and
  deleting a note while another device edits it discards the edits. The spec moves notes to the
  trash before deleting them for this reason.

[loro]: https://loro.dev/
