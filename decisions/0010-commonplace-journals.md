# 0010 Commonplace journals

## Context

Commonplace's owner wants to keep more than one journal. Some prompt them during the day, such as a
work log that asks what they've been doing, which feeds their standup reports. Others take entries
only when the owner decides to write one. Some prompts ask for data instead of prose, such as hours
of sleep, a weight, or a mood picked from a list, and that data should chart over time.

Other apps split along these lines. Logseq, Roam, and Obsidian's daily notes give each day one page,
which holds one journal and nothing more. Day One keeps many journals, files each entry in exactly
one of them, and shows them mixed or one at a time. Memos has no journals, but groups its stream of
memos by day, with a calendar of how many each day holds. Daylio and Nomie track structured answers
and chart them. TagTime pings its owner at random times, so the samples don't depend on when they
remember to write, and its devices share a seed so they agree on when to ping.

[Decision 0002](0002-commonplace-library-storage.md) stores each library as one Loro document that
merges edits from many devices, and [decision 0007](0007-commonplace-spaces.md) gives each device
its own settings outside every library. Both apply here: a page per day merges badly, because two
offline devices both create the same day's page, and a prompt answered on one device shouldn't
appear again on another.

## Criteria

- **Many journals.** The owner keeps any number of journals and turns each one's prompts on and off.
  A view shows one journal alone, or any mix of them by day.
- **Prompts merge well.** A prompt that the owner answers, skips, or snoozes on one device doesn't
  come back on another, and two devices that handle the same prompt at once leave one record.
- **Data charts.** Structured answers keep their meaning when the owner renames a question or one of
  its choices, so a chart can follow one question across months.
- **Writing stays writing.** Prose answers live in a body that the editor, search, links, and
  backlinks already handle.
- **Low friction.** Answering a prompt takes a keystroke and some typing, and the owner can write an
  entry without a prompt, or turn an existing memo into one.
- **Private while sharing.** A prompt never shows a journal's content while the owner shares their
  screen.
- **Fits the format.** Older readers keep working, and journals follow the rules for notes,
  libraries, and the trash.

## Decision

Add two kinds of note, `journal` and `entry`. The
[library format specification](../commonplace/spec/library-format.md) defines their fields.

### Journals

A journal is a note of kind `journal`. Its body's first line names it, as a label does for any note,
and the rest describes it. Its `journal` map holds its color, its questions, its schedules, and the
record of its handled prompts.

- **Questions.** An ordered list, each with an identifier made when the owner adds it, a prompt, and
  a type. Removing a question retires it instead of deleting it, so old answers keep their labels.
  Questions come in eight types.
  - **Text:** prose, written in the entry's body under a heading with the question's prompt.
  - **Short text:** one line, stored as a string in the entry's answers, for labels to count or
    list.
  - **Number:** with an optional unit, minimum, and maximum.
  - **Scale:** a whole number between two bounds, with optional labels for the ends.
  - **Choice:** one option or more, each with its own identifier, so renaming an option keeps its
    history.
  - **Yes or no.**
  - **Duration:** in minutes.
  - **Time of day:** as `HH:MM`, such as a bedtime.
- **Schedules.** Zero or more, each with an identifier. A journal with none takes entries only when
  the owner writes them. Both kinds of schedule pick their days with an `RRULE` of dates, as
  [decision 0006](0006-commonplace-recurring-actions.md) defines, and give times in the device's
  local time.
  - **Fixed:** a list of times on each of those days, such as 10:00, 14:00, and 17:00 on workdays.
  - **Random:** a window, such as 9:00 to 17:00, a mean gap, and a minimum gap. A random number
    generator that the spec fixes, seeded with the journal's identifier, the schedule's identifier,
    and the date, picks the times, so every device computes the same ones.
- **Prompt keys.** Each prompt has a key made of its schedule's identifier, its date, and its time.
  Devices compute prompts from the schedules, and nothing writes the prompts themselves.
- **Handled prompts.** A map from prompt key to what happened to that prompt: answered, with the
  entry's identifier; skipped; or snoozed until a time. Two devices that handle the same prompt
  write the same key, so the merge keeps one record, and the last write wins.
- **Missed prompts.** A device that wakes to more than one unhandled prompt from one journal shows
  only the latest, and doesn't record the others. A prompt from an earlier day never shows.

### Entries

An entry is a note of kind `entry`, and belongs to exactly one journal, in the journal's library.
Its `entry` map holds:

- **`journal`:** the journal's identifier.
- **`at`:** when the entry happened, which the owner can edit to backfill or correct it. A new entry
  takes the time the owner opens it, so a snoozed prompt answered later takes the later time.
- **`prompt`:** the key of the prompt it answers, absent for an entry the owner started.
- **`prompted`:** when that prompt first came due, absent for an entry the owner started.
- **`answers`:** a map from question identifier to answer, for every type but text: a number, a
  string, a Boolean, or a list of option identifiers.

A new entry's body holds a heading for each text question in the journal, in order, with room to
write under each. The headings are a template, not a schema: nothing reads them back, and renaming
the question later leaves existing entries alone. To address a text answer on its own, such as for a
blockers list, the owner splits it into a journal of its own.

A journal in the trash takes its entries with it, and restoring it restores them, as a project does
its steps. Deleting a journal for good deletes its entries. Moving a journal to another library
moves its entries with it.

### Turning a memo into an entry

A memo becomes an entry in one commit: a new entry in the chosen journal takes the memo's body, with
`at` set to the memo's creation time, the memo's body becomes a link to the entry, and the memo goes
to the archive, as [decision 0009](0009-commonplace-archived-notes.md) defines. Links to the memo
still lead to the entry through that link.

### Prompting

Each device decides which journals prompt on it, in its own settings, as decision 0007 keeps device
preferences. A journal prompts on the device that creates it, and on no other device until the owner
turns it on there.

A prompt arrives as a notification from the platform's own service: on Linux, the Desktop
Notifications Specification, which libnotify implements and a server such as mako shows, and on
Android, the system's notifications. The notification names the journal, so the owner can choose
whether to answer, snooze, or skip without opening it. Its actions answer, snooze for 15 minutes or
an hour, or skip, and activating the notification itself answers. Answering opens the journal's
capture window for that prompt, on Linux, or the app at that prompt, on Android. The notification
service decides how and when to show a notification, so its do-not-disturb modes hold prompts, and a
notification never takes keyboard focus.

### Capture

`commonplace capture journal` opens the capture window for an entry, with a button for each journal
in place of the space buttons, since the journal decides the library. It starts on the journal the
device last captured into, and `Alt+1` to `Alt+9` pick a journal, as they pick a space when
capturing a memo. Journals in hidden libraries get no button. Picking a journal shows its questions
as a form, every answer optional, with the body below it. A body still unedited takes the new
journal's headings.

`commonplace capture journal <journal>` starts on the named journal, given by its identifier, its
`commonplace:note/` URI, or its name, ignoring case, if exactly one journal has that name. A
keyboard binding that names a journal by identifier survives a rename.

Answering a prompt opens the same window on the prompt's journal, and saving records the prompt as
answered. Closing the window without saving leaves the prompt unhandled. A memo's details sidebar
offers to turn it into an entry.

### Views

One timeline shows entries grouped by day, newest first, with a button per journal to show or hide
it, and a range of dates to cover. Showing one journal alone gives that journal's view, with a chart
for each structured question: a line for numbers, scales, durations, and times, and counts per day
for choices and yes or no.

### Against the criteria

- **Many journals:** each journal is a note, so the owner can make as many as they like, and the
  timeline's buttons show any mix of them. Per-device settings turn prompts on and off.
- **Prompts merge well:** every device computes the same prompts and keys, random ones included, and
  a handled prompt is one key in a map, so devices agree on what's left.
- **Data charts:** answers point to questions and options by identifier, never by text, and retired
  questions keep their history.
- **Writing stays writing:** text answers live in the body as Markdown, where every existing feature
  already works.
- **Low friction:** a prompt opens the form directly, the capture window starts an entry anywhere,
  and a memo becomes an entry in one step.
- **Private while sharing:** the owner turns on the notification service's do-not-disturb mode while
  sharing, and it holds every prompt until they turn it off.
- **Fits the format:** older readers show a note of a kind they don't know as a memo and leave its
  fields alone, so the version stays the same. Journals and entries follow the existing rules for
  creating, trashing, and moving notes.

## Rejected alternatives

- **A page per day.** Logseq and Roam work this way. It's the simplest view of a day, but it holds
  one journal, and two offline devices that both open today's page create two pages for the same
  day. It fails the many journals and fits the format criteria.
- **Entries as memos with a journal map.** Filing a memo into a journal and back would take one
  click, with no copy, but entries and memos are different things to their owner, with their own
  views, and the Memos tab would need a filter to keep them apart. Turning a memo into an entry by
  copying it, and leaving a link behind, costs little, since entries rarely get linked.
- **Each text answer in its own text container.** Text answers would stay addressable, but the
  editor, search, and the link and backlink scans all assume one body per note. It fails the writing
  stays writing criterion, and a separate journal addresses an answer more simply.
- **Random times picked by each device.** Each device would pick its own times, so the owner would
  get pinged by both, and neither device could match the other's handled prompts. It fails the
  prompts merge well criterion.
- **Snoozes kept on the device.** A snooze would stay local, but the other device would prompt again
  at once. It fails the prompts merge well criterion.
- **Journals defined in the library's settings.** Definitions would stay out of the note lists, but
  settings keys are last-write-wins values with no links, no trash, and no history. A journal as a
  note gets all three.
- **Times in the `RRULE`.** An `RRULE` with `BYHOUR` and `BYMINUTE` can express fixed times, but
  decision 0006 keeps rules to dates, and random schedules need a window that `RRULE` can't express.
  A rule for the days and a list or window for the times covers both.
- **Detecting screen sharing directly.** Linux offers no portal or protocol for an app to learn that
  its owner shares the screen. The notification service's do-not-disturb mode is a signal the owner
  already controls.
- **Notifications that name no journal.** A generic prompt would reveal less if one appeared while
  the owner shares their screen, but the owner decides whether to answer, snooze, or skip by which
  journal is asking, and a generic prompt hides that. The do-not-disturb mode already keeps prompts
  off a shared screen.
- **A prompt window of the app's own.** A window that opens at each prompt would show the form at
  once, but it would take keyboard focus while the owner types elsewhere, and the app would have to
  learn when the owner wants quiet. The notification service already knows.

## Consequences

- Every implementation needs the seeded generator for random schedules, exactly as the spec fixes
  it, or its devices disagree on the times.
- Devices in different time zones compute different prompt keys, so a journal that prompts on both
  can prompt twice. One owner rarely runs devices in two zones.
- An entry's charts depend on its journal's questions, so an entry moved alone to another library
  loses them. Moving the journal moves its entries together.
- A standup view, which gathers a range of work log entries with the actions closed in that range,
  can build on the timeline in a later change.
- The app needs editors for questions and schedules, the prompt notifications, the timeline, and the
  charts.
