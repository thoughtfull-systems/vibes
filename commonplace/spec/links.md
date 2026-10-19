# Links

Version 1. This spec defines how Commonplace identifies notes and how links refer to them, inside
note text and from outside the app.
[Decision 0003](../../decisions/0003-commonplace-note-identity-and-links.md) explains the choices.

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

- The **target** is the text between `[[` and the first `|` or `]]`.
- The **label**, if present, is the text between the first `|` and `]]`.
- Implementations remove white space from both ends of the target and the label.
- Neither part may contain a line break or `]]`. A wikilink ends at the first `]]` after its `[[`.
- A wikilink isn't recognized inside a code span, a fenced or indented code block, or an HTML block,
  or when a backslash escapes its first `[`.
- An empty target makes the text not a wikilink.

In version 1, a target is a note identifier. A wikilink whose target isn't the identifier of a note
in the library, including one in the trash, is a broken link. Implementations keep its text intact
and show it as broken. Later versions may define other targets, such as titles, aliases, or zettel
addresses, and how they resolve.

An implementation shows a wikilink by its label, or by the target note's [label] if the wikilink has
none.

When an editor inserts a wikilink for the user, it writes the target note's identifier and the
note's current label: `[[<id>|<label>]]`. It leaves out the label if the note has none. It replaces
any `]]`, `|`, or line break in the label with a space.

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
