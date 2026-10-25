# 0009 Commonplace archived notes

## Context

[Decision 0004](0004-commonplace-actions.md) lets the owner archive an action, which sets it aside
and out of every list while leaving it open and unchanged. Version 2 of the
[library format specification](../commonplace/spec/library-format.md) stores this as `archived` in
the action's fields. Memos have no archive, so a memo that no longer matters either stays in the
Memos tab or goes to the trash, and emptying the trash deletes it for good.

Journals, which a later decision defines, need the archive for memos too. Converting a memo into a
journal entry leaves the memo behind with a link to the entry, so that links to the memo still lead
somewhere, and the memo should stay out of the way where the trash never deletes it.

## Criteria

- **Every kind.** Memos, and the kinds that later versions add, go to the archive the way actions
  do.
- **One field.** Every kind keeps its archive time in the same place, so readers, writers, and the
  app's archive button treat all notes alike.
- **Leaves the note's content alone.** Archiving doesn't change `modified`, as archiving an action
  doesn't.
- **No archive lost.** Archives made before the change, or on a device that hasn't updated yet,
  survive it.

## Decision

Give every note an optional `archived` timestamp beside `trashed`, and move actions' `archived` out
of their action fields to it. Writers set and remove it without changing `modified`. The change
raises the format to version 3, so a device still on version 2 opens an upgraded library read-only
instead of missing the archive.

On every open and every import, an implementation moves any `archived` it finds in an action's
fields to the note. A device on version 2 can archive an action offline and sync it later, after
another device has upgraded the library, and the next open or import moves that archive too.

In the Linux app, every note's details sidebar ends with an Archive button, and the same button
takes a note back out of the archive. The Memos tab hides archived memos. All Notes lists them, and
ordering it by archive date lists only archived notes.

- **Every kind:** the key sits on the note, so new kinds get the archive with no change to the
  format.
- **One field:** after the move, version 3 data holds `archived` only on notes, and readers look
  nowhere else.
- **Leaves the note's content alone:** the spec exempts `archived` from updating `modified`.
- **No archive lost:** the move on open upgrades existing libraries, and the move on import catches
  archives that a version 2 device makes before it updates.

## Rejected alternatives

- **Keep actions' `archived` in their fields, and add a note-level key for the other kinds.** It
  needs no version change, since older readers would ignore the new key, but the format keeps two
  places for one fact, and the app needs a different path to archive an action than a memo. It fails
  the one field criterion.
- **Use the trash.** It needs no change to the format, but emptying the trash deletes archived memos
  for good. It fails the every kind criterion, since the archive's purpose is to keep notes.

## Consequences

- Each device has to update before it can write to a library that another device has opened, because
  opening it raises the version to 3.
- Every open and import scans the notes for the old key. The scan is cheap next to loading the
  library.
