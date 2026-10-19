# Commonplace

A personal knowledge management app, named for the commonplace book, a single notebook for
everything worth keeping: reminders, plans, quotes, and ideas.

## Kinds of notes

Commonplace keeps four kinds of notes together, and any note can link to any other.

- **Memos** are freeform text with no title and no required fields: reminders for an upcoming event,
  agreements, scraps. Every other kind of note can start life as a memo, so capturing one takes a
  single keystroke and some typing.
- **Tasks** are actionable items, organized the way Getting Things Done (GTD) organizes them: next
  actions, projects, and someday or maybe. Tasks form a hierarchy, and each has a title, an optional
  body, and a checkbox. A project's body holds its description and planning.
- **Curios** record something someone else made, such as a book, an article, or a video. Each type
  of curio has its own fields. A curio can start with a title alone, and saved queries find the
  curios with fields still to fill in.
- **Zettels** hold ideas, in the sense of a Zettelkasten, a slip box. Each has an address that
  places it next to related ideas, and new ideas can go between two others or start a new branch.

Spaced repetition runs through all four. A note can come back for review on a schedule, whether to
drill a fact, to develop an idea a little more each time, or to check on a project.

## Status

The first milestone covers memos on Linux:

- Write and edit memos in an editor that renders Markdown as you write, shows the syntax of whatever
  the cursor touches, and saves as you type.
- Capture a memo from a small window or from the command line.
- Link notes with wikilinks, follow links, and see each note's backlinks.
- Open web links in the browser.
- Keep checklists in memos, and tick their boxes.
- Edit lists like an outliner, where Enter starts the next item and Tab nests it.
- Search memos, move them to the trash, and restore or delete them.
- Open notes from `commonplace:` links.

Tasks, curios, zettels, spaced repetition, sync, Android, and the web come next.

## Design

The [specs](spec) define the data, independently of any implementation:

- [Library format](spec/library-format.md): how a library stores its notes.
- [Links](spec/links.md): note identifiers, wikilinks, and URIs.

These decisions explain the choices behind them:

- [0001 Programming languages and technologies][0001]
- [0002 Commonplace library storage][0002]
- [0003 Commonplace note identity and links][0003]

## Layout

- `core`: the Rust library that every implementation shares, with the library format, links, and
  search.
- `linux`: the GTK 4 and libadwaita app for Linux.
- `spec`: the specs.

## Linux app

Build and run the app from this directory inside the development environment:

```sh
devenv shell -- cargo run --bin commonplace
```

The app keeps its library in `$XDG_DATA_HOME/commonplace`, and `XDG_DATA_HOME` defaults to
`~/.local/share`. To try it without touching that library, point `COMMONPLACE_LIBRARY` at another
directory.

Run `commonplace --help` to see the command line. A second invocation hands its work to the copy
that's already running, so commands work while the window is open.

### Keys

The shortcuts avoid Emacs key bindings, for people who remap those bindings system-wide, for example
with [xremap]. Where an Emacs command has an equivalent in the app, its shortcut is the standard key
that a typical remapping produces for it, shown in the Emacs column.

| Keys                      | Emacs        | Action                                   |
| ------------------------- | ------------ | ---------------------------------------- |
| Alt+N                     |              | New memo                                 |
| Ctrl+F or Ctrl+O          | C-s, C-x C-f | Search for a note                        |
| Ctrl+S                    | C-x C-s      | Save now, or save and close a capture    |
| Alt+\[, or type `[[`      |              | Link to a note                           |
| Ctrl+Return or Ctrl+click |              | Follow a link, or add or tick a checkbox |
| Alt+Left or Back          |              | Go back to the previous note             |
| Alt+Right or Forward      |              | Go forward again                         |
| Ctrl+Return or Escape     | C-g          | Save and close the capture window        |
| Ctrl+F4                   | C-x k        | Close the window                         |
| Ctrl+Q                    | C-x C-c      | Quit                                     |

In a list item, some keys edit the list the way an outliner such as Workflowy does:

| Keys        | Action                                                                 |
| ----------- | ---------------------------------------------------------------------- |
| Enter       | Start the next item, or move a blank item out a level, or end the list |
| Shift+Enter | Continue the item on a new line                                        |
| Tab         | Nest the item, with its children, under the preceding item             |
| Shift+Tab   | Move the item, with its children, out a level                          |

### Installing

The repository's flake provides the app as the package `commonplace`, with its desktop entry. To
install it for your user from a checkout:

```sh
nix profile install .#commonplace
```

On NixOS, add the repository as an input to your system flake, then add the package, or use the
overlay that provides `pkgs.commonplace`:

```nix
environment.systemPackages = [ vibes.packages.${pkgs.stdenv.hostPlatform.system}.commonplace ];
```

To make the app open `commonplace:` links, set it as their handler:

```sh
xdg-mime default systems.thoughtfull.Commonplace.desktop x-scheme-handler/commonplace
```

Linux has no standard way for an app to claim a global keyboard shortcut, so bind one to
`commonplace capture` in your desktop's settings. In Sway, for example, these lines bind a key to it
and make its window float:

```text
bindsym $mod+n exec commonplace capture
for_window [app_id="systems.thoughtfull.Commonplace" title="Capture"] floating enable
```

[0001]: ../decisions/0001-languages-and-technologies.md
[0002]: ../decisions/0002-commonplace-library-storage.md
[0003]: ../decisions/0003-commonplace-note-identity-and-links.md
[xremap]: https://github.com/xremap/xremap
