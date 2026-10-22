# Library format

Version 2. This spec defines how a Commonplace library stores its notes, so that every
implementation can read and write the same data.
[Decision 0002](../../decisions/0002-commonplace-library-storage.md) explains the choice of format,
and [decision 0004](../../decisions/0004-commonplace-actions.md) explains actions.

Version 1 covered memos. Version 2 adds actions. Later versions add curios, zettels, attachments,
and spaced repetition.

## Document

A library is one [Loro] document. Implementations exchange it, whole or in part, in Loro's binary
encoding: a snapshot for the whole document, or updates for the changes since a version.

Each running instance of an implementation uses a fresh, random Loro peer identifier, as Loro does
by default. Two instances that write at the same time must never share one.

The document has four root containers.

| Name       | Type | Contents                                                   |
| ---------- | ---- | ---------------------------------------------------------- |
| `meta`     | Map  | Facts about the library itself.                            |
| `notes`    | Map  | Every note, keyed by its identifier.                       |
| `actions`  | Tree | The hierarchy of actions, as the [tree](#actions) defines. |
| `settings` | Map  | Preferences that follow the owner across devices.          |

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

| Key        | Type    | Value                                                                  |
| ---------- | ------- | ---------------------------------------------------------------------- |
| `body`     | Text    | The note's content, in [Commonplace Markdown](#body).                  |
| `created`  | Integer | The note's creation time, as [a timestamp](#timestamps).               |
| `kind`     | String  | The kind of note: `memo` or `action`.                                  |
| `modified` | Integer | When the note last changed, as a timestamp.                            |
| `action`   | Map     | The [action fields](#action-fields). Present for every action.         |
| `trashed`  | Integer | When the note moved to the trash. Absent for notes not in it.          |
| `settings` | Map     | Preferences about this note alone, as [settings](#settings) describes. |

Later versions use the names `curio` and `zettel` for the kinds they add, so version 2 reserves
them. An implementation shows a note of a kind it doesn't know as a memo, and leaves its `kind`
alone.

A note's `kind` never changes. Readers ignore an `action` map in a note whose kind isn't `action`.

### Action fields

An action is a project if its `project` is `true`, and a task otherwise.

| Key           | Type    | Value                                                         |
| ------------- | ------- | ------------------------------------------------------------- |
| `archived`    | Integer | When its owner set the action aside, out of every list.       |
| `closed`      | Integer | When the action became done, as a timestamp.                  |
| `defer`       | String  | The [date](#dates) until which the action stays out of sight. |
| `due`         | String  | The date the action is due.                                   |
| `list`        | String  | One of `inbox`, `active`, `waiting`, `someday`, or `maybe`.   |
| `planning`    | Integer | When the action joined the planning queue, as a timestamp.    |
| `project`     | Boolean | Whether the action is a project.                              |
| `state`       | String  | `open` or `done`.                                             |
| `waiting_for` | String  | Who or what the action waits for, in plain text.              |

The lists come from Getting Things Done (GTD): inbox, next actions, and waiting for, with the
someday and maybe list of GTD split in two. The next actions list is `active` here, because it holds
projects in progress as well as tasks to do now. An action on the someday list is one to do without
a decided time, and an action on the maybe list is one that might not get done at all. A new action
starts on the inbox list, and leaves it when its owner sorts it onto another. `closed` is absent
while the action is open, `archived` is absent while it's outside the archive, and `defer`, `due`,
`planning`, and `waiting_for` are optional. Archiving an action changes only `archived`, so taking
it out of the archive restores it unchanged. The planning queue holds the actions their owner wants
to plan next, and an action leaves it only when its owner removes `planning`, because planning can
span more than one sitting.

A reader treats a missing or unknown `state` as `open`, a missing or unknown `list` as `active`, and
a missing `project` as `false`.

A project's list and dates apply to its subtree too. An action counts as deferred if it or any
ancestor has a `defer` date after today. The list that applies to an action is the latest, among its
own list and its ancestors' lists, in the order `active`, `waiting`, `someday`, `maybe`, `inbox`.

### `actions`

The `actions` tree holds one node for each action. Each node's metadata map has one key, `note`, a
string holding the action's note identifier. A node's parent is the action it belongs to, and a node
with no parent is a top-level action.

The tree orders siblings with Loro's fractional indexes. Every writer enables them before it creates
or moves a node.

Concurrent changes can leave the tree and the notes out of step, so readers resolve these cases:

- **More than one node for a note.** The node whose identifier is smallest, comparing the peer first
  and the counter second, belongs to the action. Readers ignore the others, and treat their children
  as children of the node that belongs to the action.
- **A node for a missing note, or for a note that isn't an action.** Readers ignore the node, and
  treat its children as children of its parent.
- **An action with no node.** Readers treat it as a top-level action, after the actions that have
  nodes.

### `settings`

Each key in `settings` names a preference, prefixed with the implementation that uses it and a
period, such as `linux.sidebar.outline`. Each value is a whole number or a string. The last write to
a key wins. Readers ignore keys they don't know, and a missing key means the implementation's
default.

The Linux app keeps the widths of its sidebars, in pixels, under `linux.sidebar.actions`,
`linux.sidebar.details`, and `linux.sidebar.outline`. It keeps the filters of its Memos, All Notes,
and Trash tabs under `linux.filter.memos`, `linux.filter.all`, and `linux.filter.trash`, each
followed by one of these names:

| Name           | Value                                                                |
| -------------- | -------------------------------------------------------------------- |
| `order`        | 0 created, 1 modified, 2 due, 3 scheduled, 4 trashed, or 5 archived. |
| `newest_first` | 1 to list the latest first, or 0 to list the earliest first.         |
| `from`         | The day to start from, written as year, month, and day digits, or 0. |

For example, `linux.filter.all.from` holds `20261024` to start All Notes from October 24, 2026.

The string `linux.screen` holds the screen the owner last viewed, so the app opens there on any
device: `memos`, `all`, `trash`, or `actions.` followed by `execute`, `plan`, `review`, or `inbox`.
With a note open, a slash and the note's identifier follow, such as `memos/` and then the
identifier. If the note no longer exists, the app opens the Memos tab.

A note's own `settings` map follows the same rules, for preferences that belong to that note alone
and go with it when it's deleted. Changing one doesn't change the note's `modified` time. The Linux
app keeps the height, in pixels, that the owner dragged the panel below the note's text to under
`linux.bottom.height`. Without it, the panel fits what it shows. It keeps whether an action's list
of steps shows its done steps under `linux.steps.show_done`: 1 to show them, or 0 to hide them.
Without it, the list hides them.

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
- **Keep the tree with the actions.** Make each change to the `actions` tree in the same commit as
  the change to the note it belongs to.
- **Make an action with its node.** To create an action, insert its note with its `action` map and
  create its node, all in one commit.
- **Delete an action with its subtree.** To delete an action, delete its node and its subtree's
  nodes and notes.
- **Promote parents.** When an action gets its first child, set its `project` to `true`.
- **Close and reopen.** Set `closed` when `state` becomes `done`, and remove it when `state` becomes
  `open`.
- **Preserve what you don't understand.** Never remove or overwrite a key or a root container that
  this spec doesn't define. A later version may define it.

## Body

A note's body is [CommonMark] Markdown with the [GitHub Flavored Markdown][gfm] extensions for
tables, task list items, strikethrough, and autolinks, plus the wikilinks in the
[links spec](links.md#wikilinks).

A pair of `==` marks a highlight, as Obsidian, Pandoc with its `mark` extension, and others read it:
`==this==` highlights "this." The markers pair the way `~~` does for strikethrough, within one
block. A marker opens if text follows it without a space, and closes if text precedes it without a
space. Only a run of exactly two `=` counts, and markers inside code don't count.

## Trash

A note is in the trash if it has `trashed`, or if it's an action with an ancestor in the trash. A
project in the trash takes its whole subtree with it, and restoring the project restores the
subtree. Restoring an action also restores any ancestors in the trash, so that it leaves the trash
rather than staying there through one of them.

## Dates

A date is a calendar day, written `YYYY-MM-DD` as in `2026-10-24`. It has no time of day or time
zone. An implementation compares dates with the current date where it runs.

## Timestamps

A timestamp is an integer count of milliseconds since 1970-01-01 00:00:00 Coordinated Universal
Time, ignoring leap seconds.

Device clocks disagree, so timestamps only order events approximately. Don't use them to resolve
conflicts. Loro does that.

## Labels

A note's label is its title, which implementations show wherever a list or a link needs a note's
name. They derive it from the body:

1. Take the first line of the body that contains a character other than white space.
1. Remove the line's leading white space.
1. Remove one marker from the start of the line, if it starts with one of those listed below.
1. Remove any leading and trailing white space that remains.
1. If nothing remains, the note has no label, and implementations show a placeholder instead.

The markers are:

- **Heading marker:** one or more `#` characters followed by a space.
- **List marker:** `-`, `*`, or `+` followed by a space, plus an optional checkbox, `[ ]`, `[x]`, or
  `[X]`, after any white space.
- **Block quote marker:** `>`.

An implementation may shorten a long label for display.

[commonmark]: https://spec.commonmark.org/
[gfm]: https://github.github.com/gfm/
[loro]: https://loro.dev/
