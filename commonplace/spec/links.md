# Links

Version 2. This spec defines how Commonplace identifies notes and how links refer to them, inside
note text and from outside the app.
[Decision 0003](../../decisions/0003-commonplace-note-identity-and-links.md) explains the choices.

Version 1 had no escapes inside wikilinks. Version 2 adds them, so a label can hold any character
but a line break.

## Note identifiers

A note identifier is a version 7 Universally Unique Identifier (UUID), as the
[UUID specification][rfc9562] defines it. An implementation generates the identifier when it creates
the note. The identifier never changes and never gets reused.

The text form of an identifier is its 128 bits, read as one unsigned big-endian integer, written in
base 32 with 26 digits, most significant first, padded with leading zeros. The digits are
[Crockford's Base32][crockford] alphabet, in lowercase:

```text
0123456789abcdefghjkmnpqrstvwxyz
```

The first digit holds only the top 3 bits, so it's always between `0` and `7`.

Writers always produce lowercase. Readers accept uppercase too, and treat any other character, any
other length, or a first digit greater than `7` as not an identifier. Readers don't accept
Crockford's aliases for the letters `i`, `l`, `o`, and `u`.

For example, the UUID `01920000-0000-7000-8000-000000000000` has the text form
`01j8000000e008000000000000`.

## Wikilinks

A wikilink is a link written in note text with double square brackets:

```markdown
[[target]]
[[target|label]]
```

- Inside the brackets, a backslash escapes a backslash, a `]`, or a `|`: `\\`, `\]`, and `\|` stand
  for the character alone, and don't count as syntax. A backslash before any other character is a
  backslash.
- The **target** is the text between `[[` and the first `|` or `]]` that a backslash doesn't escape.
- The **label**, if present, is the text between that `|` and the `]]`.
- Implementations remove white space from both ends of the target and the label, and then remove the
  escaping backslashes.
- Neither part may contain a line break. A wikilink ends at the first `]]` after its `[[` whose
  first `]` isn't escaped.
- A wikilink isn't recognized inside a code span, a fenced or indented code block, or an HTML block,
  or when a backslash escapes its first `[`.
- An empty target makes the text not a wikilink.

In version 1, a target is a note identifier. A wikilink whose target isn't the identifier of a note
in the library, including one in the trash, is a broken link. Implementations keep its text intact
and show it as broken. Later versions may define other targets, such as titles, aliases, or zettel
addresses, and how they resolve.

### Labels

An implementation shows a wikilink by its label. If the wikilink has none, it shows the target
note's [label], or for a note deleted for good, the label its [tombstone] keeps. If the target's
label holds wikilinks, it shows each of them by its label, or else by its own target's label with
any wikilinks in that label shown by their labels or targets, so labels that link to each other
can't loop. If no label applies, it shows the target.

A label that a wikilink stores is one the user chose. When an editor inserts a wikilink for the
user, it stores the target note's identifier alone: `[[<id>]]`. If the user selected text first, it
stores that text as the label: `[[<id>|<text>]]`. It replaces any line break in a label with a
space, and escapes each backslash, `]`, and `|`.

An editor that shows a note's raw text may fill in labels so the text shows titles, as long as it
stores the text without them:

- When it loads a note, it gives each wikilink without a label its target's label, as described
  earlier, and remembers each label it gives each target.
- When it saves, it removes each label that equals its target's current label or a label it gave
  that target while the note was open.
- When a target's label changes while the note is open, it replaces each label it gave that target
  with the new one, gives a label to each wikilink without one whose target gained a label, and
  remembers the labels it gives.

## Uniform resource identifiers

A note's uniform resource identifier (URI) is `commonplace:note/<id>`, where `<id>` is the
identifier in lowercase. Implementations register themselves as the handler for the `commonplace`
scheme where their platform allows it, and open the note when asked to open its URI.

`web+commonplace:note/<id>` is equivalent. It exists for web implementations, because browsers only
let web pages handle custom schemes whose names start with `web+`.

A web implementation that runs as a server also serves each note at `https://<host>/note/<id>`.

Readers ignore any query or fragment after `<id>`, which later versions may define.

Note text may also link with a standard Markdown link whose destination is a note URI, such as
`[the plan](commonplace:note/<id>)`. Implementations treat such a link like a wikilink with that
target and label.

## Backlinks

Note A links to note B if A's body contains a wikilink or a Markdown link whose target is B's
identifier. B's backlinks are all the notes that link to it, except notes in the trash.

[crockford]: https://www.crockford.com/base32.html
[label]: library-format.md#labels
[rfc9562]: https://www.rfc-editor.org/rfc/rfc9562
[tombstone]: library-format.md#deleted
