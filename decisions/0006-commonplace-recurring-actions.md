# 0006 Commonplace recurring actions

## Context

[Decision 0004](0004-commonplace-actions.md) gave actions due and start dates, and left recurring
actions for later. Many actions come back on a schedule: a weekly review, a bill each month, a plant
to water a week after the last watering. Today each one needs a new action, or an old one reopened
with new dates by hand.

Org mode's repeaters show the shape most people expect: closing a repeating task logs the completion
and moves its dates forward. Org can't say "the last Friday of the month" or "Monday, Wednesday, and
Friday," though, and moving dates forward is a write that two devices can make at the same time.

[Decision 0002](0002-commonplace-library-storage.md) stores each library as one Loro document that
merges edits from many devices, and [decision 0003](0003-commonplace-note-identity-and-links.md)
gives every note a permanent identifier that links use. A recurring action has to fit both.

The [iCalendar standard][rfc5545] defines recurrence rules, written in its `RRULE` property, that
calendar apps share, including Google Calendar. Calendar apps also let their owner move or change a
single occurrence of a recurring event, which the standard records as an override of that
occurrence.

Actions aren't the only notes that recur. [Decision 0010](0010-commonplace-journals.md) gives
journals schedules that prompt at times of day, and a calendar of events may follow, which would
sync with calendar apps through iCalendar.

## Criteria

- **Merges well.** Two devices that close the same occurrence, or different ones, at the same time
  end up agreeing on which occurrences their owner handled, and none lost or doubled.
- **An action is a note.** Links point to the recurring action as a whole, such as "the weekly
  review," and it keeps its identifier, backlinks, and history across occurrences.
- **Expressive.** The rules cover daily, weekday, and workday schedules; weekly on one or more days;
  monthly by date, by the nth or last weekday, or by the last day; yearly; every nth period; and
  schedules that count from the last completion.
- **One occurrence can change.** Moving a single occurrence, such as paying this month's bill on
  Monday, leaves the rest of the schedule alone.
- **Exclusions stay live.** Holidays and vacations remove or move occurrences, and ending a vacation
  early brings its later occurrences back. No change to days off or to a rule makes a past
  occurrence overdue.
- **Fits Getting Things Done (GTD).** The current occurrence behaves like any other action in the
  lists and reviews, with a start date that brings it into view before it's due.
- **Low friction.** A plain repeat, such as every week, takes a keystroke or two, and the details
  can wait.
- **Interoperable.** Rules translate to and from iCalendar, so a library can later publish to and
  subscribe from calendar apps.
- **Room for spaced repetition.** The same model can later schedule reviews whose next date depends
  on how the last one went.
- **Shared with other notes.** Journals, and later a calendar, schedule with the same rules, so
  every implementation evaluates rules in one place.

## Decision

A recurring action is one action note with a `recur` map in its action fields. The map holds a rule
that produces occurrence dates, and a record of the occurrences its owner handled. The current
occurrence comes from the two, and nothing writes it. The
[library format specification](../commonplace/spec/library-format.md) defines the fields when the
format adds them.

- **Rules.** A rule is an `RRULE` string with no time parts, plus the date it starts from. It
  supports `FREQ` of `DAILY`, `WEEKLY`, `MONTHLY`, or `YEARLY`, and `INTERVAL`, `BYDAY` with
  ordinals such as `-1FR`, `BYMONTHDAY` including negative days, `BYMONTH`, `BYSETPOS`, `UNTIL`, and
  `COUNT`. Editing a rule keeps the occurrences already handled, and the unhandled occurrences of
  the old rule give way to those of the new one.
- **Times of day.** Rules produce dates only. A note that happens at times of day adds its times to
  each occurrence date, as journal schedules do with a list of times or a window, so the times never
  change which days a rule produces. Journals, and later a calendar, evaluate their rules with the
  same evaluator as recurring actions, exclusions included.
- **Occurrence dates.** Each occurrence has a date, and the rule gives the action's due and start
  dates as offsets in days from it. The due offset can be absent, and without a start offset an
  occurrence starts on its date, so an action never shows before it. An action can be due on the
  date, start on it with no deadline, or start three days before it's due. One rule drives both
  dates, so they can't drift apart. While an action recurs, its occurrence's dates replace its own
  `due` and `defer`.
- **Handled occurrences.** A map keyed by occurrence date records each occurrence its owner closed
  or skipped, when, and the day it happened. Closing the current occurrence adds an entry and leaves
  the action's `state` open. Two devices that close the same occurrence write the same key, so the
  merge keeps one entry. Another map keyed by occurrence date links an occurrence to a note written
  for it, such as the notes from one week's review, so an occurrence gets a note only when it needs
  one.
- **Overrides.** A map keyed by occurrence date moves single occurrences to other dates, and the
  offsets give a moved occurrence's due and start dates from its new date. The key stays the date
  the rule produced, so a moved occurrence keeps its handled entry and its steps, and two devices
  that move the same occurrence write the same key, and the last write wins. Exclusions don't apply
  to a moved occurrence, since its owner chose its date. An override whose key the rule no longer
  produces has no effect, so editing a rule drops the overrides of its old occurrences. Later kinds
  of notes can add fields to an override, such as a time.
- **Missed occurrences.** The rule names one of three behaviors. `all` makes every occurrence count,
  so missed ones pile up, as bills do. `latest` shows only the latest occurrence that has come into
  view, and counts earlier unhandled ones as missed, as a weekly review does. `after` starts the
  rule again from the day its owner most recently handled an occurrence, as watering plants does,
  even if that occurrence's date comes before another handled one's. Under every behavior, unhandled
  occurrences before the latest one handled on or after its start date count as missed, so editing a
  rule or its days off never brings old occurrences back as overdue, and handling an occurrence
  early leaves the ones before it waiting: for good under `all`, since bills stay owed, and until it
  starts under `latest`. Each handled occurrence records whether it came early when its owner
  handled it, so a later edit can't change that, and it misses only the occurrences that had started
  by the day its owner handled it. Each handled entry also records the boundary of those earlier
  dates, with exceptions for dates that moves postponed or brought into view early. Readers keep
  this boundary after edits to the rule, offsets, moves, or days off, so an edit can't undo which
  occurrences a completion missed.
- **Days off.** A library holds two kinds of days off: holidays, and vacations. Each holds named
  schedules, such as Christmas or a trip: a span of days that a rule can bring back, such as every
  year. A recurring action says, for each kind, whether a day off removes an occurrence, moves it to
  the next day that isn't a day off, or doesn't affect it, which is the default. Days off that
  remove occurrences filter the candidate days before `BYSETPOS` picks among them, so "the last
  weekday of the month" with holidays removed means the last workday. Removed days still count
  toward `COUNT`, as excluded dates do in iCalendar, so days off never add a period to the end of a
  rule, nor another date to its last period. Every reader computes occurrences from the days off as
  they stand, so archiving a schedule early brings its later occurrences back. Schedules are never
  deleted, only archived, and an archived schedule keeps applying through the day its owner archived
  it, so the past doesn't change. The apps hide a schedule once it's archived or a week after its
  last day.
- **Steps.** The steps of a recurring action record their completion in the same way, keyed by the
  occurrence of the recurring action they belong to, so each occurrence starts with every step open
  and nothing resets them. Readers ignore `recur` on an action inside the subtree of another
  recurring action.
- **Ending.** A rule ends at its `UNTIL` or `COUNT`, after which the action counts as done. Marking
  the action itself done stops it early. Archiving it pauses it, and on its return the missed
  occurrences behave as the rule's missed behavior says.

Against the criteria:

- **Merges well:** closing an occurrence adds a key that names the occurrence, so concurrent closes
  of the same occurrence collapse into one, and closes of different ones both survive. No field
  holds the next date for two devices to overwrite.
- **An action is a note:** the recurring action is one note for its whole life, and notes for single
  occurrences link to it without replacing it.
- **Expressive:** `RRULE` covers each calendar schedule in the criterion. Holidays add workdays, and
  the `after` behavior adds schedules that count from the last completion, which `RRULE` can't
  express.
- **One occurrence can change:** an override moves one occurrence and leaves the rule, and every
  other occurrence, as they were.
- **Exclusions stay live:** readers compute occurrences from the days off on every read, so a
  vacation ended early stops shaping the dates after it. Archived schedules keep shaping the dates
  before, and unhandled occurrences before the latest one handled once started count as missed, so
  the past doesn't change.
- **Fits GTD:** the current occurrence has a due date, a start date, and open steps like any other
  action, so the list order, the next tasks, and the review questions in decision 0004 apply
  unchanged.
- **Low friction:** a new recurring action needs only a frequency. The start date, interval, missed
  behavior, offsets, and days off all have defaults.
- **Interoperable:** a rule is `RRULE` text already, and an override is an iCalendar override keyed
  by the occurrence it replaces. Only the `after` behavior and days off need translating, into a
  fixed rule and `EXDATE` lists. Exclusions that move occurrences add `RDATE` lists too. Dates
  excluded in an imported event become skipped occurrences, so rules never hold `EXDATE` lists of
  their own.
- **Room for spaced repetition:** the next occurrence is a function of the rule and the handled
  occurrences. A spaced repetition schedule is another such function, with a rating in each entry.
- **Shared with other notes:** rules hold dates only, and each kind of note adds its times on top,
  so one evaluator serves actions, journals, and a calendar.

## Rejected alternatives

- **Moving the dates forward, as Org does.** Closing the action would write its next due and start
  dates and log the completion. Two devices that close it at the same time each write the same next
  date, so the merge loses one completion. With a log, it keeps both completions but advances once.
  An exclusion applied when the dates moved stays applied after its owner removes it. It fails the
  merges well and exclusions stay live criteria.
- **A template that spawns a new action for each occurrence.** Each occurrence would be its own
  action with its own steps and notes. Two devices could each spawn the same occurrence, leaving
  duplicates, unless identifiers came from the occurrence date, which decision 0003 rules out. Links
  would point to one occurrence or to the template, never to the recurring action as a whole, and
  the tree would fill with closed copies. It fails the merges well and an action is a note criteria.
- **Resetting the action and spinning off a closed copy.** Closing an occurrence would copy it as a
  done action and reopen the original with new dates. Concurrent closes would make two copies and
  race to reset the dates and steps. It fails the merges well criterion.
- **Resetting the steps on each occurrence.** Reopening every step in one commit would race with
  edits to those steps from other devices, and would lose the record of which steps each occurrence
  finished. It fails the merges well criterion.
- **Separate rules for the start and due dates.** Org repeats its scheduled and deadline dates
  separately, and they drift apart when one moves without the other. It fails the fits GTD
  criterion.
- **Times in the rule.** `RRULE` can give times with `BYHOUR` and `BYMINUTE`, but it combines every
  hour with every minute, so 9:00 and 17:30 also produce 9:30 and 17:00, and it has no way to give
  the random windows that journals need. Actions recur by day, and times in their rules would only
  add cases to handle. It fails the shared with other notes criterion.
- **Overrides keyed by the new date.** Keying a moved occurrence by the date it moved to would
  detach it from its handled entry and its steps, and a second move would leave two keys for one
  occurrence. It fails the merges well and one occurrence can change criteria.
- **Moving an occurrence by skipping it and adding a separate action.** The new action would lose
  the recurring action's steps and links, and nothing would tie it to the occurrence it replaces. It
  fails the an action is a note criterion.
- **A rule syntax of Commonplace's own.** A smaller syntax, like Org's repeaters, would be quicker
  to parse, but each schedule it lacks needs a new design, and every calendar app would need a
  translation. It fails the expressive and interoperable criteria.
- **Calendars of excluded days of the owner's own.** Any number of named calendars, each with its
  own choice on every recurring action and journal schedule, would cover more kinds of days off, but
  each new calendar would add a choice to every rule. Two fixed kinds cover holidays and vacations,
  and a later version can add more. It fails the low friction criterion.
- **A choice on each rule for each holiday.** Choosing per schedule of days off would mean marking
  every recurring action for Christmas, Thanksgiving, and every other holiday, and again for each
  new one. It fails the low friction criterion.
- **Deleting days off.** Deleting a past vacation would bring back the occurrences it removed, as
  missed or overdue. It fails the exclusions stay live criterion.
- **Exclusions as `EXDATE` lists in each rule.** The iCalendar standard's own exclusions list fixed
  dates that each rule keeps a copy of. A vacation would mean editing every rule it touches, and
  ending it early would mean finding them all again. It fails the exclusions stay live and low
  friction criteria.

## Consequences

- The library format becomes version 4. Older versions of Commonplace open a version 4 library
  read-only.
- Every implementation needs an `RRULE` evaluator for dates, extended with days off, overrides, and
  the `after` behavior.
- Journal schedules accept any rule this decision supports, and skip or move past days off as
  recurring actions do.
- The apps need ways to edit rules and days off, to see upcoming and past occurrences, and to skip
  or move one.
- A calendar of events needs a later decision on time zones, durations, keys for occurrences at
  times of day, and translating its events to and from iCalendar.
- Subscribing to iCalendar feeds, as a source of exclusions such as holidays and time off, needs a
  later decision.
- Publishing to calendar apps needs a later decision. Google Calendar fetches a subscribed calendar
  from a public address, which a library on its owner's devices doesn't have.
- Spaced repetition can build on handled occurrences and add a rating to each entry, in a decision
  of its own.

[rfc5545]: https://www.rfc-editor.org/rfc/rfc5545
