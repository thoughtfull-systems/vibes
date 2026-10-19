# Library format

Version 1. This spec defines how a Commonplace library stores its notes, so that every
implementation can read and write the same data.
[Decision 0002](../../decisions/0002-commonplace-library-storage.md) explains the choice of format.

Version 1 covers memos. Later versions add tasks, curios, zettels, attachments, and spaced
repetition.

## Document

A library is one [Loro] document. Implementations exchange it, whole or in part, in Loro's binary
encoding: a snapshot for the whole document, or updates for the changes since a version.

Each running instance of an implementation uses a fresh, random Loro peer identifier, as Loro does
by default. Two instances that write at the same time must never share one.

The document has two root containers.

| Name    | Type | Contents                             |
| ------- | ---- | ------------------------------------ |
| `meta`  | Map  | Facts about the library itself.      |
| `notes` | Map  | Every note, keyed by its identifier. |

### `meta`

| Key       | Type    | Value                                              |
| --------- | ------- | -------------------------------------------------- |
| `format`  | String  | Always `commonplace-library`.                      |
| `version` | Integer | The highest format version that wrote the library. |

An implementation sets both keys when it creates a library. When it writes to a library with a lower
`version` than its own, it raises `version` to its own.

An implementation that finds a `version` higher than any it knows opens the library read-only,
because it can't tell which of its writes would break newer data. Additions that older readers can
safely ignore, such as a new optional key, don't raise the version.

### `notes`

Each key in `notes` is a note identifier, as the [links spec](links.md#note-identifiers) defines.
Each value is a map with these keys.

| Key        | Type    | Value                                                         |
| ---------- | ------- | ------------------------------------------------------------- |
| `body`     | Text    | The note's content, in [Commonplace Markdown](#body).         |
| `created`  | Integer | The note's creation time, as [a timestamp](#timestamps).      |
| `kind`     | String  | The kind of note. Version 1 defines `memo`.                   |
| `modified` | Integer | When the note last changed, as a timestamp.                   |
| `trashed`  | Integer | When the note moved to the trash. Absent for notes not in it. |

Later versions use the names `curio`, `task`, and `zettel` for the kinds they add, so version 1
reserves them. An implementation shows a note of a kind it doesn't know as a memo, and leaves its
`kind` alone.

## Rules for writers

- **Create a note in one commit.** Insert the note's map with `body`, `created`, `kind`, and
  `modified` in the same Loro commit, so a partial note never reaches another device. Create `body`
  as a text container, even when it's empty.
- **Edit text in place.** Change `body` with insertions and deletions, or with Loro's diff-based
  update, never by replacing the container. A replaced container discards concurrent edits from
  other devices.
- **Update `modified`.** Set `modified` to the current time in the same commit as any change to the
  note.
- **Trash before deleting.** To delete a note, first set `trashed`. To restore it, remove `trashed`.
  Only delete a note's entry from `notes` once it's in the trash. Deleting an entry discards any
  concurrent edits to it from other devices, so the trash gives those edits time to arrive.
- **Preserve what you don't understand.** Never remove or overwrite a key or a root container that
  this spec doesn't define. A later version may define it.

## Body

A note's body is [CommonMark] Markdown with the [GitHub Flavored Markdown][gfm] extensions for
tables, task list items, strikethrough, and autolinks, plus the wikilinks in the
[links spec](links.md#wikilinks).

## Timestamps

A timestamp is an integer count of milliseconds since 1970-01-01 00:00:00 Coordinated Universal
Time, ignoring leap seconds.

Device clocks disagree, so timestamps only order events approximately. Don't use them to resolve
conflicts. Loro does that.

## Labels

A memo has no title, so implementations show a label derived from its body wherever a list or a link
needs a name:

1. Take the first line of the body that contains a character other than white space.
1. Remove the line's leading white space.
1. Remove one marker from the start of the line, if it starts with one of those listed below.
1. Remove any leading and trailing white space that remains.
1. If nothing remains, the note has no label, and implementations show a placeholder instead.

The markers are:

- **Heading marker:** one or more `#` characters followed by a space.
- **List marker:** `-`, `*`, or `+` followed by a space, plus an optional task box, `[ ]`, `[x]`, or
  `[X]`, after any white space.
- **Block quote marker:** `>`.

An implementation may shorten a long label for display.

[commonmark]: https://spec.commonmark.org/
[gfm]: https://github.github.com/gfm/
[loro]: https://loro.dev/
