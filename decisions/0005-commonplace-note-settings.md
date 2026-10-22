# 0005 Commonplace note settings

## Context

The library's `settings` map holds preferences that follow the owner across devices, such as the
widths of the Linux app's sidebars. Some preferences belong to one note instead of the whole
library. The first is the height the owner drags the panel below a note's text to: a memo with many
backlinks or a project with many steps wants a taller panel than a note with one.

[Decision 0002](0002-commonplace-library-storage.md) stores each library as one Loro document, and
the [library format spec](../commonplace/spec/library-format.md) lets a version add optional keys
that older readers ignore.

## Criteria

- **Goes with the note.** Deleting a note removes its preferences, with no cleanup for every writer
  to remember.
- **Leaves the note's content alone.** Changing a preference doesn't change the note's `modified`
  time, so lists sorted by modification don't reorder.
- **Fits the format.** Older readers keep working, and the rules match the library's `settings`.

## Decision

Give each note an optional `settings` map, beside `body` and `action`, with the same rules as the
library's `settings`: each key starts with the implementation that uses it and a period, and the
last write to a key wins. Writers create the map with an ordinary map container, as they create
`action`, and don't change `modified` when they change a setting.

- **Goes with the note:** deleting a note's entry from `notes` deletes its `settings` with it.
- **Leaves the note's content alone:** the spec exempts these keys from updating `modified`.
- **Fits the format:** it's an optional key that older readers ignore, so the version stays the
  same.

## Rejected alternatives

- **Library settings keyed by note identifier.** A key such as `linux.bottom.height.` followed by
  the identifier keeps preferences in one place, but deleting a note leaves its keys behind unless
  every writer that deletes notes also searches `settings`. It fails the goes with the note
  criterion.
- **A mergeable Loro map.** Loro can merge two devices that create the same child map at once, but
  it marks the slot in a way that older Loro versions read as a binary value instead of a map. It
  fails the fits the format criterion.

## Consequences

- A note's preferences sync with the note, and stay with it in the trash until it's deleted.
- Two devices that both set a note's first preference at once keep only one device's map, which
  costs little for preferences.
- Each implementation documents its note settings in the spec, as it does its library settings.
