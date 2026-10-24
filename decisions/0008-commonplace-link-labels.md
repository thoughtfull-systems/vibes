# 0008 Commonplace link labels

## Context

[Decision 0003](0003-commonplace-note-identity-and-links.md) makes every link name its target by
identifier, so links survive renames. It also had the editor write the target's title into each link
as a label, `[[<id>|<title>]]`, so the raw text reads well.

A note's title comes from the first line of its body, so it changes whenever the owner edits that
line. A label copied into a link then goes stale: the link keeps showing the old title. Nothing can
tell a copied title from a label the owner typed, so nothing can safely update it.

Commonplace notes live in one Conflict-free Replicated Data Type (CRDT) document per library, per
[decision 0002](0002-commonplace-library-storage.md). Any fix that rewrites the text of the notes
that link to a renamed note edits notes the owner didn't touch, on every device that runs it. When
two devices make the same rewrite before they sync, the CRDT keeps both insertions and doubles the
title.

Nobody reads a library's raw text outside the app. Someone who wants plain Markdown files exports
them, and the export can write titles into the links.

## Criteria

- **Current titles.** A link shows its target's current title, not the title from when the owner
  wrote the link.
- **Custom labels.** A label the owner writes stays as written.
- **Merge safety.** A rename or a deletion edits no other note's text, so devices can't make
  conflicting copies of the same change.
- **Deleted targets.** A link to a note deleted for good still says what it pointed to.
- **Low cost.** The design adds little to the format and to each implementation.

## Decision

A link's stored text carries a label only when the owner chose one. When the owner picks a note in
the link search, or creates one from it, the editor stores `[[<id>]]`. If the owner selected text
before linking, the selection becomes the label.

An implementation shows a link without a label by its target's current title. An editor that edits
the raw Markdown, such as the Linux app's, translates as it loads and saves:

- On load, it fills each link without a label with its target's title, and remembers each title it
  shows for each target.
- On save, it removes each label that equals its target's current title or a title it showed for
  that target. Undoing a title update brings back an old title, and the save still removes it.
- While the note is open, it updates the labels of links whose target's title changed, and fills in
  links whose target gained a title.

A filled-in title has to come back from the text exactly, or the save can't recognize it, so version
2 of the [links specification](../commonplace/spec/links.md#wikilinks) lets a backslash escape a
backslash, a `]`, or a `|` inside a wikilink. Writers escape all three in labels.

When the owner deletes a note for good, its library keeps a tombstone with its identifier and last
title, and a link to it shows that title, styled as broken. The
[library format specification](../commonplace/spec/library-format.md#deleted) defines tombstones.

- **Current titles:** stored links have no title to go stale, and every implementation looks up the
  title when it shows the link.
- **Custom labels:** a label differs from the title it replaced, so a save keeps it.
- **Merge safety:** a rename changes only the renamed note, and a deletion adds a tombstone, which a
  map merges by key without touching any text.
- **Deleted targets:** the tombstone's title stands in for the note. A link added on another device
  before the sync still finds it.
- **Low cost:** a tombstone is a few bytes per deleted note, and the translation is a pair of text
  passes in each editor.

## Rejected alternatives

- **Rewrite the links when a title changes.** Each rename edits every note that links to the renamed
  one, and titles change with every keystroke on a first line. It fails the merge safety criterion,
  because two devices that rewrite the same link double the title, and links written on another
  device before the sync stay stale.
- **Keep labels, and treat a label that matches one of the target's past titles as automatic.** The
  stored text stays readable, but every note has to keep a history of its titles, which grows with
  each keystroke on its first line. It fails the low cost criterion, and the raw text it keeps
  readable has no reader.
- **Rewrite the links of a note when it's deleted.** Deletion is rare, but the rewrite still fails
  the merge safety criterion: two devices that empty the trash before they sync double the label,
  and a link added on another device stays bare.
- **Show the title without changing the editor's text.** The Linux editor is a GTK text view, which
  can't show text that isn't in its buffer. Hiding the identifier and drawing the title over it
  breaks when a line wraps. It fails the current titles criterion in the editor itself.
- **Mark automatic labels with their own syntax,** such as `[[<id>|=<title>]]`. The marker separates
  automatic labels from custom ones, but the stored text still holds a title that goes stale, so
  something still has to rewrite it. It fails the merge safety criterion like the first alternative.
- **Replace the characters a label can't hold with spaces,** as version 1 of the links specification
  did for `]]` and `|`, and keep a label that ends in `]` apart from the closing brackets with a
  space. It needs no syntax change, but titles with `|` or `]]` come back altered, so each save of a
  note that links to them stores them as custom labels. It fails the current titles criterion for
  those titles.

## Consequences

- Raw note text shows identifiers where it used to show titles. Exports to Markdown fill in titles.
- Search over raw text doesn't find a note by the titles of the notes it links to.
- Links written before this decision keep their labels. A label that equals its target's title when
  the owner edits the note looks the same as a filled-in title, so that save stores the link without
  it, and from then on it follows the title.
- A title that holds a link shows that link's target title, one level deep, so titles that link to
  each other can't loop.
- Moving a note to another library deletes it from the first without leaving a tombstone, since the
  note still exists.
- A label written before version 2 that holds a backslash before a `]` or a `|` now reads
  differently: the backslash escapes the character, and a `\\]` before the closing brackets keeps
  the link open.
