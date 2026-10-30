# Library format

Version 3. This spec defines how a Commonplace library stores its notes, so that every
implementation can read and write the same data.
[Decision 0002](../../decisions/0002-commonplace-library-storage.md) explains the choice of format,
and [decision 0004](../../decisions/0004-commonplace-actions.md) explains actions.

Version 1 covered memos. Version 2 adds actions. Version 3 lets every note go to the archive, and
adds journals. Later versions add curios, zettels, attachments, and spaced repetition.

## Document

A library is one [Loro] document. Implementations exchange it, whole or in part, in Loro's binary
encoding: a snapshot for the whole document, or updates for the changes since a version.

A device can hold more than one library and show them together as spaces, as
[decision 0007](../../decisions/0007-commonplace-spaces.md) describes. Each library is a document of
its own, and syncs on its own.

Each running instance of an implementation uses a fresh, random Loro peer identifier, as Loro does
by default. Two instances that write at the same time must never share one.

The document has five root containers.

| Name       | Type | Contents                                                   |
| ---------- | ---- | ---------------------------------------------------------- |
| `meta`     | Map  | Facts about the library itself.                            |
| `notes`    | Map  | Every note, keyed by its identifier.                       |
| `actions`  | Tree | The hierarchy of actions, as the [tree](#actions) defines. |
| `settings` | Map  | Preferences that follow the owner across devices.          |
| `deleted`  | Map  | A [tombstone](#deleted) for each note deleted for good.    |

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
| `kind`     | String  | The kind of note: `memo`, `action`, `journal`, or `entry`.             |
| `modified` | Integer | When the note last changed, as a timestamp.                            |
| `action`   | Map     | The [action fields](#action-fields). Present for every action.         |
| `archived` | Integer | When the note went to the [archive](#archive). Absent outside it.      |
| `journal`  | Map     | The [journal fields](#journal-fields). Present for every journal.      |
| `entry`    | Map     | The [entry fields](#entry-fields). Present for every entry.            |
| `starred`  | Integer | When the note gained its [star](#stars). Absent without one.           |
| `trashed`  | Integer | When the note moved to the trash. Absent for notes not in it.          |
| `settings` | Map     | Preferences about this note alone, as [settings](#settings) describes. |

Later versions use the names `curio` and `zettel` for the kinds they add, so version 3 reserves
them. An implementation shows a note of a kind it doesn't know as a memo, and leaves its `kind`
alone.

A note's `kind` never changes. Readers ignore an `action`, `journal`, or `entry` map in a note of
another kind.

### Action fields

An action is a project if its `project` is `true`, and a task otherwise.

| Key           | Type    | Value                                                         |
| ------------- | ------- | ------------------------------------------------------------- |
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
while the action is open, and `defer`, `due`, `planning`, and `waiting_for` are optional. The
planning queue holds the actions their owner wants to plan next, and an action leaves it only when
its owner removes `planning`, because planning can span more than one sitting.

A reader treats a missing or unknown `state` as `open`, a missing or unknown `list` as `active`, and
a missing `project` as `false`.

A project's list and dates apply to its subtree too. An action counts as deferred if it or any
ancestor has a `defer` date after today. The list that applies to an action is the latest, among its
own list and its ancestors' lists, in the order `active`, `waiting`, `someday`, `maybe`, `inbox`.

### Journal fields

A note of kind `journal` is a journal, as
[decision 0010](../../decisions/0010-commonplace-journals.md) describes. Its body's label names it.
Its `journal` map has these keys.

| Key         | Type         | Value                                                         |
| ----------- | ------------ | ------------------------------------------------------------- |
| `color`     | String       | The journal's color, from the list below.                     |
| `questions` | Movable list | The [questions](#questions), in order, each a map value.      |
| `schedules` | Map          | The [schedules](#schedules), each a map value, by identifier. |
| `handled`   | Map          | What happened to each [prompt](#prompts), by its key.         |

The colors are `blue`, `green`, `yellow`, `orange`, `red`, `purple`, `brown`, and `gray`, and
readers treat a missing or unknown `color` as `blue`. Identifiers of questions, options, and
schedules take the form of note identifiers, and writers make a new one for each.

#### Questions

Each question is a map value, which writers replace whole to change it.

| Key        | Type    | Value                                                                    |
| ---------- | ------- | ------------------------------------------------------------------------ |
| `id`       | String  | The question's identifier, which never changes.                          |
| `prompt`   | String  | The question as it shows.                                                |
| `type`     | String  | The kind of answer it takes, from the list below.                        |
| `retired`  | Integer | When the owner removed the question. Absent while it's current.          |
| `unit`     | String  | For `number`, an optional unit.                                          |
| `min`      | Number  | For `number`, an optional least value, and for `scale`, the least value. |
| `max`      | Number  | For `number`, an optional greatest value, and for `scale`, the greatest. |
| `low`      | String  | For `scale`, an optional label for `min`.                                |
| `high`     | String  | For `scale`, an optional label for `max`.                                |
| `multiple` | Boolean | For `choice`, whether an answer can pick more than one option.           |
| `options`  | List    | For `choice`, the options in order.                                      |

The types are `text`, `short_text`, `number`, `scale`, `choice`, `yes_no`, `duration`, and `time`.
Each option is a map of `id`, `label`, and an optional `retired`. A scale with no `min` runs from 1,
and one with no `max` runs to 5. Readers skip a question with no `id` or an unknown `type`. A
retired question or option asks nothing, but keeps its place so old answers keep their labels. A
question's `type` never changes, since its answers depend on it.

#### Schedules

Each schedule is a map value. Its `rule` is an `RRULE` of dates: `FREQ=DAILY`, or `FREQ=WEEKLY` with
a `BYDAY` that names days without ordinals. Its `kind` is `fixed` or `random`. Times are local to
the device that reads them, written `HH:MM`.

- **Fixed:** `times` is a list of times.
- **Random:** `start` and `end` bound a window, `mean` is a whole number of minutes, at least 1, and
  `min_gap` is a whole number of minutes.

Readers skip a schedule whose rule or fields they can't read.

#### Prompts

A schedule makes its prompts for each day its rule includes. A fixed schedule prompts at each of its
times. A random schedule seeds a generator with the 64-bit Fowler-Noll-Vo (FNV) hash, in its FNV-1a
variant, of the UTF-8 text of the journal's identifier, a space, the schedule's identifier, a space,
and the date, written `YYYY-MM-DD`. The generator is SplitMix64, whose state starts at the seed and
which, for each output, adds `0x9E3779B97F4A7C15` to its state and mixes the result with the
standard SplitMix64 mixing function. The schedule then walks each minute from `start` up to but not
including `end`. A minute less than `min_gap` minutes after the last prompt draws nothing. Every
other minute draws one output, and prompts if the output is a multiple of `mean`. A `min_gap` of 0
counts as 1.

A prompt's key is the schedule's identifier, the date, and the time, joined by `/`, as in
`<schedule>/2026-10-26/14:00`. Each value in `handled` is a map value with a `state` of `answered`,
with the entry's identifier in `entry`; `skipped`; or `snoozed`, with a timestamp in `until`. It
also holds, in `at`, the timestamp of the write that made it. A prompt has come due once its time
has passed and nothing has handled it, or once its snooze has run out. An implementation shows only
the latest prompt due today in each journal, and never a prompt from an earlier day.

### Entry fields

A note of kind `entry` is an entry in one journal. Its `entry` map has these keys.

| Key        | Type    | Value                                                                    |
| ---------- | ------- | ------------------------------------------------------------------------ |
| `journal`  | String  | The identifier of its journal, which is in the same library.             |
| `at`       | Integer | When the entry happened, as a timestamp.                                 |
| `prompt`   | String  | The key of the prompt it answers. Absent for an entry started by hand.   |
| `prompted` | Integer | When that prompt came due. Absent for an entry started by hand.          |
| `answers`  | Map     | The answers to the questions that aren't `text`, by question identifier. |

An answer to a `number`, `scale`, or `duration` question holds a number, with durations in minutes.
An answer to a `short_text` or `time` question is a string, with times written `HH:MM`. An answer to
a `yes_no` question is a Boolean, and one to a `choice` question is a list of option identifiers.
Readers treat an entry with a missing `at` as happening at its `created` time, and show an entry
with no `journal` as a memo.

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

The Linux app keeps no settings here. Its preferences describe one device, so it keeps them on the
device, under the keys below. Versions before spaces kept them in the library, and the Linux app
copies them from there once, when it moves that library into a folder of its own.

The Linux app keeps the widths of its sidebars, in pixels, under `linux.sidebar.actions`,
`linux.sidebar.details`, and `linux.sidebar.outline`. It keeps the filters of its Memos, All notes,
and Trash tabs under `linux.filter.memos`, `linux.filter.all`, and `linux.filter.trash`, each
followed by one of these names:

| Name           | Value                                                                |
| -------------- | -------------------------------------------------------------------- |
| `order`        | 0 created, 1 modified, 2 due, 3 scheduled, 4 trashed, or 5 archived. |
| `newest_first` | 1 to list the latest first, or 0 to list the earliest first.         |
| `from`         | The day to start from, written as year, month, and day digits, or 0. |

For example, `linux.filter.all.from` holds `20261024` to start All notes from October 24, 2026.

The string `linux.screen` holds the screen the owner last viewed, so the app opens there: `memos`,
`all`, `trash`, or `actions.` followed by `execute`, `plan`, `review`, or `inbox`. With a note open,
a slash and the note's identifier follow, such as `memos/` and then the identifier. If the note no
longer exists, the app opens the Memos tab.

A note's own `settings` map follows the same rules, for preferences that belong to that note alone
and go with it when it's deleted. Changing one doesn't change the note's `modified` time. The Linux
app keeps the height, in pixels, that the owner dragged the panel below the note's text to under
`linux.bottom.height`, and the Android app keeps the same height, in density-independent pixels,
under `android.bottom.height`. Each app removes its key when the owner fits the panel to what it
shows again. Without it, the panel fits what it shows.

A key without an implementation's prefix belongs to every implementation. Every app keeps whether an
action's list of steps shows its done steps under `steps.show_done`: 1 to show them, or 0 to hide
them. Without it, apps read `linux.steps.show_done`, where the Linux app kept the choice before, and
without either, the list hides them. Writing `steps.show_done` removes `linux.steps.show_done`.

### `deleted`

Each key in `deleted` is the identifier of a note deleted for good, and each value is a map with
these keys. Links to the note show the label the tombstone keeps, as the
[links spec](links.md#labels) describes.

| Key       | Type    | Value                                                         |
| --------- | ------- | ------------------------------------------------------------- |
| `deleted` | Integer | When the owner deleted the note, as a timestamp.              |
| `label`   | String  | The note's [label](#labels) then. Absent if it had none then. |

Readers ignore a tombstone for a note that `notes` still holds.

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
- **Leave a tombstone.** In the commit that deletes a note for good, write a tombstone for it, for
  each action in its subtree, and for each entry in it if it's a journal. Moving a note to another
  library leaves none, because the note still exists.
- **Promote parents.** When an action gets its first child, set its `project` to `true`.
- **Answer a prompt with its entry.** Create an entry that answers a prompt in the same commit as
  the `handled` record that names it.
- **Forget old prompts.** A writer may remove `handled` records whose prompts' dates come before
  yesterday, since those prompts never show again.
- **Turn a memo into an entry in one commit.** Create the entry with the memo's body and `at` set to
  the memo's `created`, replace the memo's body with a wikilink to the entry, and set the memo's
  `archived`, all in one commit.
- **Move a journal with its entries.** Moving a journal to another library moves every entry in it,
  and an entry never moves on its own.
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

An entry is in the trash if its journal is, so a journal in the trash takes its entries with it.
Restoring an entry also restores its journal. Deleting a journal for good deletes its entries, with
a tombstone for each.

## Archive

Archiving sets a note aside, out of every list, without changing it, so taking it out of the archive
restores it unchanged. A note is in the archive if it has `archived`. Setting or removing `archived`
doesn't change the note's `modified` time. An entry counts as archived while its journal is, and an
archived journal doesn't prompt.

Version 2 kept `archived` in the action fields. When an implementation opens or imports a library,
it moves any `archived` it finds there to the note, in one commit, keeping the note's own if it
already has one. A device still on version 2 can archive an action offline and sync it after another
device upgrades the library, so the move runs every time, not only once.
[Decision 0009](../../decisions/0009-commonplace-archived-notes.md) explains the choice.

## Stars

A star marks a note its owner wants to find quickly. A note has a star if it has `starred`. Setting
or removing `starred` doesn't change the note's `modified` time. Readers ignore `starred` on
journals and entries. A note keeps its star when it goes to the archive or the trash, and an action
keeps it when it closes, until its owner removes `starred`.
[Decision 0014](../../decisions/0014-commonplace-starred-notes.md) explains the choice.

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

An entry's label comes from the first line that contains a character other than white space and
doesn't start with a heading marker, since its journal's template writes the headings. An entry with
nothing but headings takes its label from the body as any note does.

An implementation may shorten a long label for display.

[commonmark]: https://spec.commonmark.org/
[gfm]: https://github.github.com/gfm/
[loro]: https://loro.dev/
