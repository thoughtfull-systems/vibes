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
calendar apps share, including Google Calendar.

## Criteria

- **Merges well.** Two devices that close the same occurrence, or different ones, at the same time
  end up agreeing on which occurrences their owner handled, and none lost or doubled.
- **An action is a note.** Links point to the recurring action as a whole, such as "the weekly
  review," and it keeps its identifier, backlinks, and history across occurrences.
- **Expressive.** The rules cover daily, weekday, and workday schedules; weekly on one or more days;
  monthly by date, by the nth or last weekday, or by the last day; yearly; every nth period; and
  schedules that count from the last completion.
- **Exclusions stay live.** Holidays and vacations remove or move occurrences, and taking an
  exclusion away brings its occurrences back.
- **Fits Getting Things Done (GTD).** The current occurrence behaves like any other action in the
  lists and reviews, with a start date that brings it into view before it's due.
- **Low friction.** A plain repeat, such as every week, takes a keystroke or two, and the details
  can wait.
- **Interoperable.** Rules translate to and from iCalendar, so a library can later publish to and
  subscribe from calendar apps.
- **Room for spaced repetition.** The same model can later schedule reviews whose next date depends
  on how the last one went.

## Decision

A recurring action is one action note with a `recur` map in its action fields. The map holds a rule
that produces occurrence dates, and a record of the occurrences its owner handled. The current
occurrence comes from the two, and nothing writes it. The
[library format specification](../commonplace/spec/library-format.md) defines the fields.

- **Rules.** A rule is an `RRULE` string with no time parts, plus the date it starts from. It
  supports `FREQ` of `DAILY`, `WEEKLY`, `MONTHLY`, or `YEARLY`, and `INTERVAL`, `BYDAY` with
  ordinals such as `-1FR`, `BYMONTHDAY` including negative days, `BYMONTH`, `BYSETPOS`, `UNTIL`, and
  `COUNT`. Editing a rule keeps the occurrences already handled, and the unhandled occurrences of
  the old rule give way to those of the new one.
- **Occurrence dates.** Each occurrence has a date, and the rule gives the action's due and start
  dates as offsets in days from it. Either offset can be absent, so an action can be due on the
  date, start on it with no deadline, or start three days before it's due. One rule drives both
  dates, so they can't drift apart. While an action recurs, its occurrence's dates replace its own
  `due` and `defer`.
- **Handled occurrences.** A map keyed by occurrence date records each occurrence its owner closed
  or skipped, when, and the day it happened. Closing the current occurrence adds an entry and leaves
  the action's `state` open. Two devices that close the same occurrence write the same key, so the
  merge keeps one entry. An entry can link to a note written for that occurrence, such as the notes
  from one week's review, so an occurrence gets a note only when it needs one.
- **Missed occurrences.** The rule names one of three behaviors. `all` makes every occurrence count,
  so missed ones pile up, as bills do. `latest` shows only the latest occurrence that has come into
  view, and counts earlier unhandled ones as missed, as a weekly review does. `after` starts the
  rule again from the day its owner handled the last occurrence, as watering plants does.
- **Exclusions.** A library holds named calendars of excluded days: single days, date ranges, and
  rules for days that come back, such as a holiday each year. A recurring action refers to the
  calendars that apply to it, and each reference says whether an excluded day removes an occurrence
  or moves it to the next day that isn't excluded. Exclusions filter the candidate days before
  `BYSETPOS` picks among them, so "the last weekday of the month" with a holiday calendar means the
  last workday. Every reader computes occurrences from the calendars as they stand, so removing an
  exclusion brings its occurrences back.
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
- **Expressive:** `RRULE` covers each calendar schedule in the criterion. Exclusion calendars add
  workdays, and the `after` behavior adds schedules that count from the last completion, which
  `RRULE` can't express.
- **Exclusions stay live:** readers compute occurrences from the calendars on every read, so an
  exclusion's effect lasts only as long as the exclusion.
- **Fits GTD:** the current occurrence has a due date, a start date, and open steps like any other
  action, so the list order, the next tasks, and the review questions in decision 0004 apply
  unchanged.
- **Low friction:** a new recurring action needs only a frequency. The start date, interval, missed
  behavior, offsets, and calendars all have defaults.
- **Interoperable:** a rule is `RRULE` text already. Only the `after` behavior and exclusion
  calendars need translating, into a fixed rule and `EXDATE` lists.
- **Room for spaced repetition:** the next occurrence is a function of the rule and the handled
  occurrences. A spaced repetition schedule is another such function, with a rating in each entry.

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
- **A rule syntax of Commonplace's own.** A smaller syntax, like Org's repeaters, would be quicker
  to parse, but each schedule it lacks needs a new design, and every calendar app would need a
  translation. It fails the expressive and interoperable criteria.
- **Exclusions as `EXDATE` lists in each rule.** The iCalendar standard's own exclusions list fixed
  dates that each rule keeps a copy of. A vacation would mean editing every rule it touches, and
  ending it early would mean finding them all again. It fails the exclusions stay live and low
  friction criteria.

## Consequences

- The library format becomes version 3. Older versions of Commonplace open a version 3 library
  read-only.
- Every implementation needs an `RRULE` evaluator for dates, extended with exclusion calendars and
  the `after` behavior.
- The apps need ways to edit rules and exclusion calendars, to see upcoming and past occurrences,
  and to skip one.
- Subscribing to iCalendar feeds, as a source of exclusions such as holidays and time off, needs a
  later decision.
- Publishing to calendar apps needs a later decision. Google Calendar fetches a subscribed calendar
  from a public address, which a library on its owner's devices doesn't have.
- Spaced repetition can build on handled occurrences and add a rating to each entry, in a decision
  of its own.

[rfc5545]: https://www.rfc-editor.org/rfc/rfc5545
