# Commonplace

A personal knowledge management app, named for the commonplace book, a single notebook for
everything worth keeping: reminders, plans, quotes, and ideas.

## Kinds of notes

Commonplace keeps four kinds of notes together, and any note can link to any other.

- **Memos** are freeform text with no title and no required fields: reminders for an upcoming event,
  agreements, scraps. Capturing one takes a single keystroke and some typing.
- **Actions** are the things to do, organized the way Getting Things Done (GTD) organizes them. An
  action is either a project, an outcome that takes more than one step, or a task, a single thing to
  do. Actions form a hierarchy, and each has a title, an optional body, and a checkbox. A project's
  body holds its description and planning.
- **Curios** record something someone else made, such as a book, an article, or a video. Each type
  of curio has its own fields. A curio can start with a title alone, and saved queries find the
  curios with fields still to fill in.
- **Zettels** hold ideas, in the sense of a Zettelkasten, a slip box. Each has an address that
  places it next to related ideas, and new ideas can go between two others or start a new branch.

Spaced repetition runs through all four. A note can come back for review on a schedule, whether to
drill a fact, to develop an idea a little more each time, or to check on a project.

## Status

The Linux app covers memos and actions:

- Switch between the four tabs, Memos, Actions, All Notes, and Trash. The search bar beside them
  filters each tab's notes until you clear it. A note opens below the tabs and the search bar, and
  Back returns to the tab.
- Write and edit memos in an editor that renders Markdown as you write, shows the syntax of whatever
  the cursor touches, and saves as you type. A memo shows its outline beside it and its backlinks
  below it.
- Capture a memo or a task from a small window or from the command line, or create one from the link
  picker. Only capture puts actions in the inbox, so an action's list picker offers Inbox only while
  the action started there.
- Organize actions the Getting Things Done way, with projects, steps, lists, due dates, and start
  dates, and work with them in four modes, Execute, Plan, Review, and Inbox.
  [Where actions show](#where-actions-show) describes what each mode lists.
- Mark an action done, or archive it to set it aside without changing it.
- Reorder a project's steps by dragging them, or with Alt+Shift+Up and Alt+Shift+Down. Drop a step
  on the middle of another to nest it there.
- Link notes with wikilinks, follow links, and see each note's backlinks.
- Open web links in the browser.
- Keep checklists in memos, and tick their boxes.
- Edit lists like an outliner, where Enter starts the next item and Tab nests it.
- Select text and type `*`, `_`, `` ` ``, `~`, `=`, or `[` to wrap it in that delimiter. Type it
  again to double it, as `**` makes text bold and `==` highlights it.
- Search notes, move them to the trash from a note's menu, and restore or delete them from the Trash
  tab.
- Browse memos in the Memos tab, every note in All Notes, and the trash in Trash. Order them by
  creation, change, or trash date, and in All Notes by the due or start dates of actions, or by
  archive date, which lists only the archive. Pick a day to start from, and the list runs forward or
  back from it in the order's direction. The filters follow you to other devices, and Reset returns
  a tab to its defaults.
- Keep the sidebar widths and the screen you last viewed in the library, so they follow you to other
  devices, and the app opens where you left off.
- Open notes from `commonplace:` links.

Curios, zettels, spaced repetition, sync, Android, and the web come next.

## Design

The [specs](spec) define the data, independently of any implementation:

- [Library format](spec/library-format.md): how a library stores its notes.
- [Links](spec/links.md): note identifiers, wikilinks, and URIs.

These decisions explain the choices behind them:

- [0001 Programming languages and technologies][0001]
- [0002 Commonplace library storage][0002]
- [0003 Commonplace note identity and links][0003]
- [0004 Commonplace actions][0004]

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

### Test library

To try the app as the working tree has it, on a library of sample memos and actions, run:

```sh
devenv shell -- commonplace-test
```

The script builds a fresh library in a temporary directory, runs the app on it beside any copy
that's already running, and deletes the library when the app quits, so nothing you change survives a
restart. The samples come from `core/examples/test_library.rs`, which covers every state an action
can be in and every kind of formatting the editor renders. Their dates count from the day you run
it.

To run the working tree on a library of your own instead, set `COMMONPLACE_LIBRARY` to an existing
directory. The script prints the path, opens that library, and leaves it in place when the app
quits. Only one copy can open a library at a time, so close any everyday copy that uses it first.

To quick capture into the test copy while it runs, use `commonplace-test-capture` with `memo` or
`action`, like the `capture` command. It refuses to run without a test copy open, so captures go to
the test copy's library and never start an everyday copy. In Sway, a binding in the capture mode can
sit beside the everyday ones:

```text
bindsym t exec sh -c 'cd ~/vibes && devenv shell -- commonplace-test-capture memo'; mode "default"
```

### Where actions show

An open action is one that isn't done, archived, or in the trash, and has no such project over it.
An action's list is the one furthest back among its own and its projects' lists, in the order inbox,
maybe, someday, waiting, active. A project counts as put off when its list is the inbox, someday, or
maybe, or when it starts after today. It then stands for the actions inside it, and none of the
modes below show those.

- **Execute** lists open tasks on the active list that start within a week and aren't queued for
  planning, with only the most pressing task from each top-level project.
- **Plan** lists the planning queue, then the stalled projects: open projects that have started,
  aren't on someday or maybe, and have no open task that isn't put off either.
- **Review** lists the open projects, then the open actions whose own list is waiting, someday, or
  maybe, those whose own start date is still to come, and the actions finished in the last week. It
  leaves out actions queued for planning.
- **Inbox** shows the open actions on the inbox that have started, one at a time.

Some actions stay out of all four modes on purpose:

- **The steps of a project** beyond the one that Execute shows. The project's page lists them all.
- **Actions inside a project put off** by its list or by its start date. The project stands for
  them.
- **Actions inside a done or archived project,** which no longer matter.
- **Projects and tasks that start more than a week from now,** which their start date hides until
  then. Review shows those with a start date of their own.
- **Archived actions.** All Notes lists them, and ordering it by archive date lists only them.
- **Done actions** after their first week. All Notes lists them.
- **Inbox actions you skip,** until the next time you sort the inbox.

An action stays in the planning queue, out of Execute and Review, until you take it out, even after
you change it, because planning can take more than one sitting.

### Keys

The shortcuts avoid Emacs key bindings, for people who remap those bindings system-wide, for example
with [xremap]. Where an Emacs command has an equivalent in the app, its shortcut is the standard key
that a typical remapping produces for it, shown in the Emacs column.

| Keys                      | Emacs        | Action                                   |
| ------------------------- | ------------ | ---------------------------------------- |
| Ctrl+F or Ctrl+O          | C-s, C-x C-f | Search memos and actions                 |
| Ctrl+S                    | C-x C-s      | Save now, or save and close a capture    |
| Alt+\[, or type `[[`      |              | Link to a note                           |
| Ctrl+Return or Ctrl+click |              | Follow a link, or add or tick a checkbox |
| Escape                    | C-g          | Leave the note for the list              |
| Alt+Left or Back          |              | Go back to the previous note or listing  |
| Alt+Right or Forward      |              | Go forward again                         |
| Escape                    | C-g          | Discard the capture                      |
| Ctrl+F4                   | C-x k        | Close the window                         |
| Ctrl+Q                    | C-x C-c      | Quit, or discard the capture             |

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

Linux has no standard way for an app to claim a global keyboard shortcut, so bind keys to
`commonplace capture memo` and `commonplace capture action` in your desktop's settings. Each opens
the quick capture window for its kind of note. Its Discard button throws the text away. The commands
start Commonplace if it isn't running, and work the same from inside the app.

In Sway, for example, these lines make the window float and bind a key sequence to each command:
press `$mod+c`, then `m` for a memo or `a` for an action. Escape backs out. Sway has no syntax for a
sequence of keys, so the first key enters a binding mode, and each binding in the mode returns to
the default mode.

```text
set $capture "capture: (m)emo (a)ction"
mode $capture {
    bindsym m exec commonplace capture memo; mode "default"
    bindsym a exec commonplace capture action; mode "default"
    bindsym Escape mode "default"
}
bindsym $mod+c mode $capture
for_window [app_id="systems.thoughtfull.Commonplace" title="Capture"] floating enable
```

[0001]: ../decisions/0001-languages-and-technologies.md
[0002]: ../decisions/0002-commonplace-library-storage.md
[0003]: ../decisions/0003-commonplace-note-identity-and-links.md
[0004]: ../decisions/0004-commonplace-actions.md
[xremap]: https://github.com/xremap/xremap
