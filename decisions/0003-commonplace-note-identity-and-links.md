# 0003 Commonplace note identity and links

[Decision 0008](0008-commonplace-link-labels.md) revises how links carry labels.

## Context

Commonplace notes link to each other, and other software links to them: a browser bookmark, a
calendar entry, a message to oneself, or a launcher on a desktop or a phone. A link has to keep
working when the note changes.

Memos have no title, and asking for one adds friction to capture. Titles that do exist change, and
reorganizing a slip box changes zettel addresses. A link can't rely on any of these. Each device
also creates notes offline, so their identifiers can't come from a shared counter.

Links appear inside note text, which people read and type as Markdown. A link's form affects how the
raw text looks to anyone who opens it outside the app.

## Criteria

- **Stable.** A link keeps pointing to the same note through edits and renames.
- **Offline creation.** Any device can create a note and its identifier without coordination.
- **Readable text.** Links don't clutter a note's raw Markdown more than they must.
- **Quick to write.** Linking while writing takes a few keystrokes.
- **Reachable from outside.** The operating system, the browser, and other apps can open a note from
  a link.

## Decision

Give every note an identifier when it's created: a version 7 Universally Unique Identifier (UUID),
written as 26 lowercase characters of Crockford's Base32. The identifier never changes and never
gets reused. The [links specification](../commonplace/spec/links.md) defines the exact syntax.

Inside note text, link with wikilinks: `[[target]]` or `[[target|label]]`. A target is a note
identifier. Later specs may add other targets, such as titles or zettel addresses, with their
resolution rules. Typing `[[` in an editor opens a search, and picking a note inserts a link to its
identifier. Standard Markdown links with a `commonplace:` URI work too.

Outside the app, refer to a note with the uniform resource identifier (URI) `commonplace:note/<id>`.
Each implementation registers itself as the handler for the `commonplace` scheme. An instance of the
web app also serves each note at `https://<host>/note/<id>`, and it registers the `web+commonplace`
scheme, because browsers only let web pages handle schemes that start with `web+`.

- **Stable:** the identifier is the only part of a link that the app resolves, and nothing about a
  note's content changes it.
- **Offline creation:** version 7 UUIDs combine a timestamp with 74 random bits, so two devices
  won't generate the same one.
- **Readable text:** Base32 makes the identifier 26 characters instead of 36, without hyphens, and
  the app shows each link by its label or its target's title.
- **Quick to write:** the editor's search turns `[[` and a few letters into a finished link.
- **Reachable from outside:** a custom scheme works on Linux through the desktop entry, on Android
  through an intent filter, and in a browser through the `web+` scheme. A web instance also gives
  every note an HTTPS address.

## Rejected alternatives

- **Links by title, as in Obsidian.** They're the quickest to type and the most readable, but they
  break when a title changes and can't reach a memo, which has no title. They fail the stable
  criterion. Later specs may accept titles as a convenience, but the editor still writes
  identifiers.
- **Standard UUID text.** The usual 36-character form with hyphens works everywhere, but it adds 10
  characters of noise to every link and fails the readable text criterion.
- **Short random identifiers,** such as 8 random characters. They read well, but each device would
  need to check for collisions, which it can't do offline. They fail the offline creation criterion.
- **Sequential numbers.** They're the most readable, but two offline devices would hand out the same
  number. They fail the offline creation criterion.
- **Markdown links only.** A single link syntax keeps the text standard, but
  `[label](commonplace:note/<id>)` is longer to type and to read than `[[<id>|label]]`. It fails the
  quick to write and readable text criteria as the main syntax, so it stays as a second form.

## Consequences

- Every implementation needs a UUID version 7 generator, a Crockford Base32 codec, and a parser for
  wikilinks.
- Implementations register the `commonplace` scheme with their platform, and the web app registers
  `web+commonplace`.
- Backlinks come from parsing every note's text, so each implementation keeps an index of links.
- Raw text shows identifiers rather than titles, so an export to Markdown files needs to rewrite
  links to file names.
