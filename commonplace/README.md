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

The Linux app covers memos, actions, and journals:

- Switch between the five tabs, Memos, Actions, Journals, All notes, and Trash. The search bar
  beside them filters each tab's notes until you clear it. A note opens below the tabs and the
  search bar. Back retraces visited notes and tabs; Escape returns to the listing.
- Write and edit memos in an editor that renders Markdown as you write, shows the syntax of whatever
  the cursor touches, and saves as you type. A memo shows its details beside it and its backlinks
  below it.
- Capture a memo, a task, or a journal entry from a small window, with buttons that switch between
  the three, or capture a memo or a task from the command line, or create one from the link picker.
  Only capture puts actions in the inbox, so an action's list picker offers Inbox only while the
  action started there.
- Organize actions the Getting Things Done way, with projects, steps, lists, due dates, and start
  dates, and work with them in four modes, Execute, Plan, Review, and Inbox.
  [Where actions show](#where-actions-show) describes what each mode lists.
- Mark an action done.
- Make an action repeat from the **Repeats** menu in its details sidebar: daily, every workday,
  weekly, monthly, yearly, or a custom rule, such as the last workday of the month. Ticking a
  recurring action closes this occurrence and shows the next, with its steps open again. **This
  time** closes or skips the current occurrence, picking a date moves it, and **History** lists past
  and upcoming occurrences. A rule can keep each missed occurrence, only the latest, or count from
  when you last did it.
- Keep holidays and vacations under **Days off** in the main menu. Each recurring action and journal
  schedule ignores them, skips them, or moves to the next day. Past and archived days off hide.
- Star a project, a task, or a memo in its details sidebar. Starred actions lead Execute, starred
  memos lead the Memos tab, and All notes leads with every starred note. Click a star there, or in a
  Starred section, to take it away. An archived note, or a done action, leaves its Starred section
  until it comes back.
- Archive any note with the button at the bottom of its details sidebar, to set it aside without
  changing it. Archived memos leave the Memos tab, and archived actions leave the four modes. All
  notes lists archived notes, and ordering it by archive date lists only them.
- Keep journals, each with its own questions: text that the entry's body holds under a heading for
  each, and short text, numbers, scales, choices, yes or no, durations, and times of day that the
  entry keeps as answers. A duration takes minutes, such as `90`, or hours and minutes, such as
  `1:30` or `1h30m`. A journal's sidebar edits its questions, its color, its schedules, and whether
  it prompts on this device.
- Get prompted by a notification that names the journal, on the days a schedule repeats, at fixed
  times or at random times within a window. Answer it, snooze it for 15 minutes or an hour, or skip
  it. Answering opens the capture window for that prompt. Your notification server's do-not-disturb
  mode holds prompts, such as while you share your screen.
- Read entries in the Journals tab, grouped by day, newest first. A button for each journal shows or
  hides it, and a menu picks how far back to go. With one journal showing, the tab charts its
  answers: a line for numbers, scales, durations, and times, and daily counts for choices and yes or
  no. An entry's sidebar changes when it happened and its answers.
- Turn a memo into a journal entry from its sidebar. The memo keeps a link to the entry and goes to
  the archive.
- Reorder a project's steps by dragging them, or with Alt+Shift+Up and Alt+Shift+Down. Drop a step
  on the middle of another to nest it there.
- Link notes with wikilinks, follow links, and see each note's backlinks. Clicking a link puts the
  cursor in it for editing; the icon immediately before the link follows it. Control-click and the
  keyboard shortcut also follow links.
- Open web links in the browser.
- Keep checklists in memos, and tick their boxes.
- Edit lists like an outliner, where Enter starts the next item and Tab nests it.
- Select text and type `*`, `_`, `` ` ``, `~`, `=`, or `[` to wrap it in that delimiter. Type it
  again to double it, as `**` makes text bold and `==` highlights it.
- Search notes, move them to the trash from a note's menu, and restore or delete them from the Trash
  tab.
- Browse memos in the Memos tab, every note in All notes, and the trash in Trash. Order them by
  creation, change, or trash date, and in All notes by the due or start dates of actions, or by
  archive date, which lists only the archive. Pick a day to start from, and the list runs forward or
  back from it in the order's direction. Reset returns a tab to its defaults.
- Keep the filters, the sidebar widths, and the screen you last viewed on the device, so the app
  opens where you left off.
- Keep separate libraries as spaces, such as Work and Personal, and show them together. A button for
  each library in the top bar hides or shows its notes, and so does Alt with the library's number.
  Capture saves into the library you pick, starting with the last one, and a note's sidebar moves it
  to another library, with its steps. The Libraries dialog adds, names, merges, and deletes
  libraries, and the names belong to the device. With one library, none of this shows.
- Keep running in the system tray. Clicking the icon hides the window, or brings it to the current
  workspace when it's hidden or on another one. Closing the window or pressing Ctrl+Q hides it, and
  the icon's menu captures an action, a journal entry, or a memo, or quits. Without a tray, Ctrl+Q
  quits.
- Sync libraries with your other devices, Linux or Android, over encrypted connections. In the Sync
  dialog, pair a device on the same network with a click and a matching code, show an invite as a QR
  code for a phone to scan, with the libraries it offers, or open an invite link from another
  device. Then choose which libraries each device shares. Sync runs by hand, or on its own after
  each change. A new relay server takes effect at once.
- Open notes from `commonplace:` links.
- Print an action with the bodies of the projects it belongs to, outermost first, with
  `commonplace print URI`, to hand it to a coding agent as a prompt.

The [Android app](android) uses Jetpack Compose and the same Rust core. It covers memos, actions,
journals, spaces, links, and the trash, with capture from Android's share menu and journal prompts
through Android notifications. Its README describes the build and device setup.

Curios, zettels, spaced repetition, and the web come next.

## Design

The [specs](spec) define the data, independently of any implementation:

- [Library format](spec/library-format.md): how a library stores its notes.
- [Sync](spec/sync.md): how devices pair and sync libraries.
- [Links](spec/links.md): note identifiers, wikilinks, and URIs.

These decisions explain the choices behind them:

- [0001 Programming languages and technologies][0001]
- [0002 Commonplace library storage][0002]
- [0003 Commonplace note identity and links][0003]
- [0004 Commonplace actions][0004]
- [0007 Commonplace spaces][0007]

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
restart. The samples come from `core/src/samples.rs`, which covers every state an action can be in
and every kind of formatting the editor renders. Their dates count from the day you run it.

To create samples on an older date, pass `--date`:

```sh
devenv shell -- commonplace-test --date 2026-01-15
```

The generator uses noon Coordinated Universal Time (UTC) on that date for creation, modification,
archive, and trash timestamps. It also dates action states and journal entries from that time, and
counts due and start dates from the chosen day. The app uses the real clock when it opens, so later
edits use the current time. Choose **All time** in Journals to see entries outside its recent date
range. Run `commonplace-test --help` to see the options.

Exit and reenter `devenv shell` after changing the launcher, or use the command here from outside
the shell. An existing shell can keep an earlier copy of the script in its command path.

To run the working tree on a library of your own instead, set `COMMONPLACE_LIBRARY` to an existing
directory. The script prints the path, opens that library, and leaves it in place when the app
quits. Only one copy can open a library at a time, so close any everyday copy that uses it first.
`--date` only creates fresh samples; it refuses to run when you set `COMMONPLACE_LIBRARY`.

To quick capture into the test copy while it runs, use `commonplace-test-capture` with `memo`,
`action`, or `journal`, like the `capture` command. It refuses to run without a test copy open, so
captures go to the test copy's library and never start an everyday copy. In Sway, a binding in the
capture mode can sit beside the everyday ones:

```text
bindsym t exec sh -c 'cd ~/vibes && devenv shell -- commonplace-test-capture memo'; mode "default"
```

### Where actions show

An open action is one that isn't done, archived, or in the trash, and has no such project over it.
An action's list is the one furthest back among its own and its projects' lists, in the order inbox,
maybe, someday, waiting, active. A project counts as put off when its list is the inbox, someday, or
maybe, or when it starts after today. It then stands for the actions inside it, and none of the
modes below show those.

- **Execute** lists the starred actions, on any list, then the open tasks on the active list that
  start within a week and aren't queued for planning, with only the most pressing task from each
  top-level project.
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
- **Archived actions.** All notes lists them, and ordering it by archive date lists only them.
- **Done actions** after their first week. All notes lists them.
- **Inbox actions you skip,** until the next time you sort the inbox.

An action stays in the planning queue, out of Execute and Review, until you take it out, even after
you change it, because planning can take more than one sitting.

### Keys

People who use Emacs key bindings can remap them system-wide into standard keys, for example with
[xremap]. Where an Emacs command has an equivalent in the app, its shortcut is the standard key that
a typical remapping produces for it, shown in the Emacs column.

| Keys                 | Emacs   | Action                                                     |
| -------------------- | ------- | ---------------------------------------------------------- |
| Ctrl+F               | C-s     | Search the current tab                                     |
| Ctrl+Tab             |         | Switch to the next tab                                     |
| Ctrl+Shift+Tab       |         | Switch to the previous tab                                 |
| Ctrl+S               | C-x C-s | Save now                                                   |
| Ctrl+Return          |         | Save and close a capture, or add or tick a list's checkbox |
| Alt+\[, or type `[[` |         | Link to a note                                             |
| Ctrl+O               |         | Follow the link at the cursor                              |
| Ctrl+click           |         | Follow a link, or add or tick a checkbox                   |
| Escape               | C-g     | Leave the note for the list                                |
| Alt+Left or Back     |         | Go back to the previous note or listing                    |
| Alt+Right or Forward |         | Go forward again                                           |
| Escape               | C-g     | Discard the capture                                        |
| Ctrl+F4              | C-x k   | Close the window                                           |
| Ctrl+Q               | C-x C-c | Hide in the tray, or discard the capture                   |

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
`commonplace capture memo`, `commonplace capture action`, and `commonplace capture journal` in your
desktop's settings. Each opens the quick capture window on its kind of note, and buttons at the top
of the window switch kinds. A journal entry starts in the journal you name after `journal`, by its
name or its `commonplace:` URI, or in the journal you last wrote in, and buttons in the window
switch journals. A binding that gives the URI keeps working when you rename the journal. Its Discard
button throws the text away. The commands start Commonplace if it isn't running, and work the same
from inside the app.

In Sway, for example, these lines make the window float and bind a key sequence to each command:
press `$mod+c`, then `m` for a memo, `a` for an action, or `j` for a journal entry. Escape backs
out. Sway has no syntax for a sequence of keys, so the first key enters a binding mode, and each
binding in the mode returns to the default mode. Sway matches a title as a regular expression
anywhere in the window's title. The main window's title always starts with `Commonplace`, so with
the anchors no note's label can make it float.

```text
set $capture "capture: (m)emo (a)ction (j)ournal"
mode $capture {
    bindsym m exec commonplace capture memo; mode "default"
    bindsym a exec commonplace capture action; mode "default"
    bindsym j exec commonplace capture journal; mode "default"
    bindsym Escape mode "default"
}
bindsym $mod+c mode $capture
for_window [app_id="systems.thoughtfull.Commonplace" title="^Quick capture$"] floating enable
```

[0001]: ../decisions/0001-languages-and-technologies.md
[0002]: ../decisions/0002-commonplace-library-storage.md
[0003]: ../decisions/0003-commonplace-note-identity-and-links.md
[0004]: ../decisions/0004-commonplace-actions.md
[0007]: ../decisions/0007-commonplace-spaces.md
[xremap]: https://github.com/xremap/xremap
