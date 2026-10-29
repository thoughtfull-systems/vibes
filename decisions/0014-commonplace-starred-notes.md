# 0014 Commonplace starred notes

## Context

[Decision 0004](0004-commonplace-actions.md) defines the next tasks that the Execute mode lists:
open tasks on the active list that start within a week and aren't queued for planning, with one such
step from each project. Execute works breadth-first. It shows the most pressing step of every
project in progress, so the owner can move between projects and keep each one going.

Some owners work depth-first instead. They pick a project or a task that's on their mind and go deep
on it. A project's page already lists its steps, so the owner doesn't need Execute to show them. The
owner needs a short way back to the few actions they've chosen. Today they have to find each one
again through Review or search.

Memos have the same need. A memo the owner keeps coming back to sinks down the Memos tab as newer
memos arrive, and the curios and zettels that later versions add will sink the same way. Journals
and their entries don't: a journal has its own tab, and an entry belongs to its journal.

## Criteria

- **Quick to reach.** One click in a note's tab opens any note the owner has chosen.
- **Every kind that needs it.** The owner can choose projects, tasks, and memos, and later the
  curios and zettels, with no change to the format.
- **Nothing pressing hides.** The next tasks stay in Execute beside the chosen actions.
- **Low friction.** Choosing a note and letting it go each take one click.
- **Same on every device.** The owner's phone and computer agree on the chosen notes.
- **Merges well.** Two devices that change the chosen notes at the same time end up in a valid
  state.

## Decision

The owner can star an action or a memo. Each tab that lists them shows its starred notes at the top,
under a **Starred** heading.

- **Storage.** A starred note has a `starred` timestamp beside `archived` and `trashed`: the time
  its owner starred it. Removing the key removes the star. Writers set and remove it without
  changing `modified`, as they do `archived`. It's a new optional key, so the
  [library format](../commonplace/spec/library-format.md) stays at version 3, and a device that
  doesn't know the key shows its lists as before. Readers ignore `starred` on journals and entries.
- **Which notes.** A note counts as starred while it has a star and sits outside the archive and the
  trash. An action also has to be open, like each project it belongs to. Finishing, archiving, or
  trashing a note leaves `starred` in place, so taking it back brings its star back too.
- **Execute.** Execute lists the starred actions first, whatever their list, start date, or place in
  the planning queue, because a star is the owner's explicit choice. It orders and colors them the
  way decision 0004 orders and colors every list, so an action that hasn't started yet shows dimmed.
  The next tasks follow, as decision 0004 defines them, without the ones that already show under
  **Starred**, so no action shows twice.
- **Memos and All notes.** The Memos tab lists its starred memos first, and All notes lists every
  starred note of any kind first. Each splits its list in two and keeps its order, search, and day
  in both parts, so the starred notes leave the list below them. The rest of the list follows under
  an **Other memos** or **Other notes** heading. Trash has no **Starred** section, since a note in
  the trash doesn't count as starred.
- **Opening.** Selecting a starred row opens the note, as every list does. A starred project opens
  to its page, with its steps.
- **Controls.** The details sidebar of an action or a memo has a star button, gray without a star
  and yellow with one, that toggles the star. Each row under **Starred** shows its yellow star too,
  as a button that takes the star away. No other list shows stars.

Against the criteria:

- **Quick to reach:** every starred note sits at the top of its tab, and All notes gathers them all,
  one click away.
- **Every kind that needs it:** the key sits on the note, so a new kind gets stars by offering the
  button, with no change to the format.
- **Nothing pressing hides:** the next tasks keep their place below the starred actions, with the
  usual colors for tasks due soon or overdue.
- **Low friction:** one click in a note's sidebar stars it, and clicking its star under **Starred**
  takes the star away.
- **Same on every device:** the key lives in the library and syncs with it.
- **Merges well:** `starred` is one key in the note's map, so concurrent changes resolve by Loro's
  last write, and every outcome is a valid state.

## Rejected alternatives

- **A key in the action fields.** Keeping `starred` among the action fields, as an earlier draft of
  this decision did, would leave memos without stars, and each later kind would need a key of its
  own. It fails the every kind that needs it criterion, as `archived` did before
  [decision 0009](0009-commonplace-archived-notes.md) moved it to the note.
- **Every starred note in Execute.** One **Starred** section for all kinds would gather what's on
  the owner's mind in one place. Execute is about what to do next, though, and a memo there doesn't
  fit. All notes gathers every starred note instead.
- **Focused projects.** Focusing a project would list every available step inside it in Execute.
  That repeats the project's page inside Execute, and it can't hold a task on its own. It fails the
  every kind that needs it criterion, and adds a view the owner already has.
- **A Starred mode.** A fifth mode beside Execute, Plan, Review, and Inbox would keep Execute
  unchanged. The owner would then switch modes to see the starred actions and the next tasks, which
  fails the quick to reach criterion.
- **Starred actions only.** Showing nothing but the starred actions in Execute while any action has
  a star would give the shortest list. A task due tomorrow in another project would vanish until the
  owner removes every star. It fails the nothing pressing hides criterion.
- **Sequential and parallel projects.** A flag on each project, as in OmniFocus, would let a
  parallel project put all its available steps into Execute. It describes how a project's steps
  depend on each other, not what the owner has on their mind, and it can't mark a task or a memo. It
  fails the every kind that needs it and low friction criteria.
- **Stars as a device setting.** Keeping the starred notes in each device's settings would keep them
  out of the library. The owner would then star the same note once on each device, which fails the
  same on every device criterion.
- **A Boolean flag instead of a timestamp.** A flag says as much as the app uses today. A timestamp
  costs nothing more and lets a later version order starred notes by when they gained a star, as the
  planning queue's timestamp does.
- **Remove the star when a note closes.** Clearing `starred` in the same commit as finishing,
  archiving, or trashing would keep the library tidy, but a note brought back would lose its star,
  and the key has no effect meanwhile anyway.
- **A star on every row.** A star button on each row of every list would let the owner star a note
  without opening it. It adds a control to every row for a choice the owner makes for only a few
  notes, and the sidebar already offers it one click after opening the note.
- **Tags.** A tag such as `starred` could mark any kind of note. Decision 0004 leaves tags for a
  later decision that covers every kind of note, and a star needs no name, so it doesn't wait for
  one.

## Consequences

- The library format specification gains the `starred` key on notes. Decision 0004's description of
  next tasks still holds, minus the starred tasks that Execute already shows.
- The Linux and Android apps ship the star buttons and the **Starred** sections in the same pull
  request, following the platform parity rule in `AGENTS.md`.
- The curios and zettels that later versions add get a star button in their sidebars, and their tabs
  a **Starred** section.
- The sample library gains a starred project, task, and memo once the feature is ready to ship.
- The Commonplace README describes the **Starred** sections.
