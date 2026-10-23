# 0007 Commonplace spaces

## Context

Commonplace's owner keeps work notes and personal notes. They use a personal machine and a work
machine. The personal machine should show both together, and the work machine should hold only the
work notes. Work notes change on both machines and need to sync between them, while personal notes
never reach the work machine. On either machine, the owner shares their screen at work, so the app
needs a quick way to hide personal notes from view.

[Decision 0002](0002-commonplace-library-storage.md) stores a library as one Loro document, which
devices sync whole, with its history. A filter in the app could hide personal notes on screen, but
syncing that document to the work machine would hand it every personal note. Decision 0002 also
rejected splitting a library into one document per note, because a move between documents has no
atomic merge.

The owner needs two spaces now, and may add another for a new job. Later, they may sync more than
one library between the same devices, so no device has one library that all its syncing goes to.

The library's `settings` map holds the Linux app's sidebar widths, filters, and the screen it last
showed. The [library format specification](../commonplace/spec/library-format.md) has them follow
the owner across devices, but they describe one device's screen and habits, and once the work
library syncs to the work machine, they'd carry the personal machine's choices with it.

## Criteria

- **Libraries stay separate.** A device receives only the libraries it syncs. Content that a note
  left behind in a library's history when it moved out, or that the owner captured there by mistake,
  is an accepted cost, as long as the app never shows it.
- **One view.** A device with more than one library shows them together, and the owner can hide any
  of them with one click.
- **Structure merges well.** Within a library, edits and moves made at the same time merge as
  decisions 0002 and [0004](0004-commonplace-actions.md) require.
- **Low friction.** A new note lands in the right library with no more than a click, and a device
  with one library shows nothing about spaces.
- **Any number of libraries.** Adding a library for a new job needs no change to the format, each
  library syncs with its own set of devices, and two libraries can merge into one.
- **Device settings stay put.** Preferences about one device's screen never reach another device.
- **Names stay the device's own.** Each device names its libraries as its owner likes, and renaming
  a library on one device never changes or conflicts with its name on another.

## Decision

A space is a library, shown alongside the device's other libraries. Each library stays one Loro
document, as decision 0002 defines, and the app opens every library on the device in one view. The
personal machine holds a Work and a Personal library, and the work machine holds the Work library
alone. Devices sync each library on its own, so the work machine and the personal machine exchange
only the Work library.

- **Names.** A library's name is a pet name that each device gives it, kept in the device's
  settings, and the library itself holds no name. A device knows its libraries by where it keeps
  them, so two devices sync whichever documents their owner pairs, and each shows the library under
  its own name.
- **Merging libraries.** Syncing works between two libraries on the same device too, so the owner
  merges one library into another by syncing it in and then deleting it. Loro merges documents that
  share no history, so the result keeps the notes and actions of both, under the target's name. A
  note that a move left in both libraries, or deleted from one of them, can come out of the merge
  doubled or lost, so the app checks every note from both libraries afterward and repairs the
  result.
- **Device settings.** Each device keeps its preferences, including its names for its libraries, in
  a store of its own, outside every library, that never syncs. Each implementation chooses its
  store's form. Preferences that belong to one note stay in the note's `settings`, as
  [decision 0005](0005-commonplace-note-settings.md) defines, and the library's `settings` stays for
  preferences that should follow the owner.
- **Notes stay whole.** Every note, its fields, and its settings live in one library. Each library
  has its own action tree, so a project and all its steps share a library. Each library has its own
  trash and its own exclusion calendars, as [decision 0006](0006-commonplace-recurring-actions.md)
  defines them, and a recurring action uses only the calendars in its own library.
- **Showing spaces.** With more than one library, the main window's top bar has a button for each
  library, in order of name, and `Alt+1` to `Alt+9` toggle them in that order. Every list, search,
  backlink, and review covers the libraries that are on, and with every library off, the window
  shows nothing. With one library, the app shows no space buttons anywhere.
- **New notes.** A step added to an action goes to the action's library, and a note made from the
  link picker goes to the library of the note the owner is editing. With more than one library, the
  capture window shows one button per library, only one of which is on at a time. It starts on the
  library the device last captured into, and `Alt+1` to `Alt+9` pick one. Capturing from the command
  line uses the last captured library too.
- **Moving notes between libraries.** Moving an action into a parent in another library moves it
  there, and with more than one library, the details sidebar of any note has a button per library
  that moves it. A move writes the note as it stands, with the same identifier, into the target
  library, then deletes it from the source. An action takes its subtree with it, and the move keeps
  its place in the new tree. The source skips the trash, so the note doesn't linger in the source
  library's trash. The app warns that the move discards any edits to the note from another device
  that haven't synced yet.
- **Links.** A link to a note in another library works on any device that holds both libraries. On a
  device that doesn't, it shows as a link to a missing note.

Against the criteria:

- **Libraries stay separate:** the work machine never holds the Personal library. A note moved from
  Personal to Work arrives without its history. A note moved out of Work leaves its old text in the
  Work library's history, which the app never shows.
- **One view:** the personal machine opens every library at once, and a button or `Alt` with a digit
  hides or shows one.
- **Structure merges well:** each library is a whole Loro document, so everything inside it merges
  as before. Only a move between libraries spans documents, and only a device that holds both can
  make one.
- **Low friction:** steps and links pick their library from where they're made, and capture
  remembers the last library, so most new notes need no choice at all. The work machine, with one
  library, looks as it does today.
- **Any number of libraries:** the app opens as many libraries as a device has, and each syncs with
  whichever devices its owner pairs, independent of the others. Merging two libraries is a sync
  followed by a delete.
- **Device settings stay put:** the device's own store never syncs, and no library carries its
  preferences.
- **Names stay the device's own:** names live in each device's store, so a rename never leaves the
  device, and a merge has no names to reconcile.

## Rejected alternatives

- **One document with a space field on each note.** Filters would work on screen, and moves would
  stay atomic, but the work machine would sync the whole document, personal notes included. Loro
  syncs whole documents, with no way to leave part of one out. It fails the libraries stay separate
  criterion.
- **One document with personal notes encrypted.** The work machine would still receive the encrypted
  text, the shape of the personal action tree, and when each personal note changed, and Loro can't
  merge text it can't read. It fails the libraries stay separate and structure merges well criteria.
- **Separate libraries, each in its own instance of the app.** Each library would sync on its own,
  but the owner would switch windows to see both, and moving a note would mean copying it by hand.
  It fails the one view and low friction criteria.
- **Device settings in a home library.** Each device could keep its preferences in the library it
  started with. But the work machine's only library is the Work library, which syncs with the
  personal machine, so its preferences would sync too, and a device that syncs more than one library
  has no library that's only its own. It fails the device settings stay put criterion.
- **An identifier for each library.** A `meta` key could name a library for sync and for device
  settings, but merging two existing libraries, as the migration does, would leave one identifier
  overwriting the other. Devices already know their libraries by where they keep them, so it adds a
  key that nothing needs.
- **A name in each library.** A `name` key in `meta` would sync, so a new device would show the
  library's name with nothing to set. But two devices that rename a library at the same time
  conflict, and Loro settles the conflict by its own order, not by time, so the earlier name can
  win. A merge would bring a second name along, and every device would have to use the same name. It
  fails the names stay the device's own criterion.

## Consequences

- The library format doesn't change. A version of the app that doesn't know spaces opens one library
  at a time, as it does today.
- Each device asks for a name when it creates a library or receives one, so a library has a name
  before its button appears.
- The Linux app moves its sidebar widths, filters, and last screen out of the library's `settings`
  into its own store on the device, and the spec stops listing them there. It copies the values the
  first time it opens a library that has them, and leaves the old keys alone.
- Each implementation opens a set of libraries, instead of one. The Linux app needs a place for them
  on the device, and a way to create, name, and pair them.
- Migrating takes these steps:
  - The personal machine's existing library becomes the Personal library, and the owner names it.
  - The owner creates a Work library there and moves their work notes into it.
  - If the work machine has no library, its owner copies the Work library to it by hand, until the
    sync transport exists. If it already has one, the first sync merges the two, as merging
    libraries does.
- The sync transport, which decision 0002 left for later, pairs devices per library, and lets one
  device sync any number of libraries.
- A move between libraries writes two documents. If the app stops between them, the note exists in
  both libraries, and the owner deletes one copy. Only a device that holds both libraries moves
  notes, so the owner's single personal machine can't race itself.
- Search, backlinks, and the link index cover more than one document at once.
