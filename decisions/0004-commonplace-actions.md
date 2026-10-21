# 0004 Commonplace actions

## Context

Commonplace's second kind of note is the action: something actionable, organized the way Getting
Things Done (GTD) organizes work. An action is either a project or a task. A project is an outcome
that takes more than one step, and its body holds its description, context, and planning. A task is
a single thing to do. Actions form a hierarchy: a project's steps are tasks and smaller projects.

Each action has a title, more text if it needs it, and a checkbox, and sits on a GTD list: inbox,
active, waiting for, someday, or maybe. Someday means an action to do without a decided time, and
maybe means one that might not happen.

Actions also need dates. A due date marks a deadline, and a start date keeps an action out of sight
until it can start, like a tickler file. An action waiting on someone else records who or what it
waits for.

Many actions grow out of a memo, and quick capture can also make a task directly.

[Decision 0002](0002-commonplace-library-storage.md) stores each library as one Loro document, and
chose Loro partly for its movable tree, which merges concurrent moves without cycles.
[Decision 0003](0003-commonplace-note-identity-and-links.md) gives every note a permanent identifier
that links use. This decision defines how actions fit into both.

## Criteria

- **Structure merges well.** Two devices that move, nest, or reorder actions at the same time end up
  with one valid hierarchy, with no cycles, no lost actions, and no action in two places.
- **An action is a note.** An action keeps its identifier, links, backlinks, and history through
  every change, including a task becoming a project.
- **Supports GTD.** The data answers the questions of a GTD review: what can I do next, which
  projects have no next task, what am I waiting for, what's due, and what's deferred. A project can
  move between active and put off, and stay a project throughout.
- **Low friction.** Capturing takes a keystroke or two, and nothing asks for details that can wait.
- **Reusable.** The pieces that aren't specific to actions, such as hierarchies, work for the curios
  and zettels to come.

## Decision

An action is a note whose kind is `action`. The
[library format specification](../commonplace/spec/library-format.md), version 2, defines the
fields.

- **Projects and tasks.** An action is a project when its `project` flag says so, and a task
  otherwise. A project can exist before it has steps. Giving a task its first step makes it a
  project.
- **Titles.** An action's title is its label: the first line of its body, by the rules that already
  name memos. The rest of the body is the action's description.
- **Action fields.** An action's fields live in an `action` map inside its note: its state, its
  list, whether it's a project, the time it closed, and its optional due date, start date,
  waiting-for note, and time it joined the planning queue.
- **Hierarchy.** A Loro movable tree, `actions`, holds every action once, with the note's identifier
  in each tree node's metadata. The tree orders siblings with fractional indexes.
- **States.** An action is open or done, and finishing it records when. Separately, an action can be
  archived: set aside on purpose, out of every list. Archiving changes nothing else about the
  action, so taking it out of the archive restores it unchanged.
- **Lists.** Every action sits on one list: inbox, active, waiting, someday, or maybe. The active
  list is the next actions list of GTD, named so that it fits a project in progress as well as a
  task to do now. A project's list applies to its whole subtree, so a project put off to someday
  takes its steps with it. When the lists of an action and its ancestors differ, the one that holds
  the action back furthest applies, in the order active, waiting, someday, maybe, inbox.
- **Planning queue.** An action can join a queue of actions to plan, with the time it joined in its
  fields, so a review can set an action aside for planning without changing its list. A queued task
  leaves the next tasks until its owner takes it out of the queue.
- **Dates.** Due and start dates are calendar dates, such as `2026-10-24`, with no time zone. An
  action due on Friday stays due on Friday wherever its owner travels.
- **Kinds don't change.** A note keeps its kind for life. An action that grows out of a memo is a
  new note, and its owner can link the two and delete the memo if it's no longer needed.
- **Capture.** The capture window and the command line can make a task directly, with the captured
  text as its body, and so can the link picker, for a task to link to. A captured task starts on the
  inbox list, out of the next tasks, until its owner sorts it onto another list. Capture is the only
  way onto the inbox list. A step added to an action starts on the active list, because adding it to
  an action sorts it.
- **Trash.** An action in the trash takes its subtree with it, and deleting it deletes its subtree.

Against the criteria:

- **Structure merges well:** Loro's movable tree resolves concurrent moves without cycles, and
  fractional indexes keep sibling order without renumbering. The spec settles the remaining
  conflicts, such as an action moved into a subtree that another device deleted.
- **An action is a note:** actions live in `notes` with every other note, so identifiers, links, and
  history work unchanged. The tree refers to notes by identifier.
- **Supports GTD:** state, list, project flag, dates, and hierarchy together answer each review
  question. Next tasks are open tasks on the active list, outside any closed, waiting, someday, or
  maybe ancestor and outside the planning queue, that start within a week, with one such step from
  each project. Every list orders its actions the same way: overdue, due within a week, started or
  undated, then starting within a week, with outline order breaking ties. Lists show an action that
  hasn't started yet dimmed, one due within a week in yellow, and one past due in red. A stalled
  project is an open project that has started, isn't put off to someday or maybe, and has no open
  task that isn't put off either. A project that's put off, by its list or a start date to come,
  stands for the actions inside it. A project put off to someday stays a project, and making it
  active again means only changing its list.
- **Low friction:** a new task needs only one line of text, and waits in the inbox until its owner
  sorts it. Capture can make one directly. Every other field can wait.
- **Reusable:** every kind of note gets its title from its label in the same way, and zettel
  sequences can use a tree of their own like the action tree.

## Rejected alternatives

- **A project list instead of a project flag.** In paper GTD, projects are a list like any other,
  and a project put off moves from the projects list to the someday list. As a list, though, a
  project put off to someday, maybe, or waiting would stop being a project: its rows would show it
  as one more someday action, and nothing would set one with no steps yet apart from a task. Making
  it active again would depend on its owner remembering it as a project. It fails the supports GTD
  criterion.

- **A title field, separate from the body.** A field would let an action's title differ from its
  first line, but a note would then have two rules for its name. The label rules already give every
  note a title, so the field adds a second place to edit the same thing, and fails the low friction
  criterion.

- **A parent field in each action.** Storing each action's parent in its note is simple, but two
  devices that move actions into each other at the same time create a cycle, which the last write to
  each field doesn't resolve. It fails the structure merges well criterion.

- **Actions as checklist items inside notes.** One outline note per project, with Markdown
  checklists, is the lightest option, and checklists still serve for quick lists inside a memo. But
  checklist items have no identifiers of their own, so nothing can link to them, review them, or
  give them dates. It fails the an action is a note and supports GTD criteria.

- **Any action with steps is a project.** Deriving projects from the hierarchy needs no flag, but a
  project with no steps yet can't exist, and people often capture a project before planning it. It
  fails the supports GTD criterion.

- **One tree per list.** Making each list the root of a separate tree would show each list directly,
  but the steps of one project often sit on different lists, and a project can't live in more than
  one tree. It fails the supports GTD criterion.

- **Timestamps for due and start dates.** A timestamp pins a deadline to an instant, so a date set
  in one time zone shifts by a day in another. Deadlines in personal planning are calendar days, so
  timestamps fail the supports GTD criterion.

- **Converting a memo into an action.** Changing a note's kind in place would keep its identifier,
  links, and history. But the kind decides how every app treats the note, so each app would need
  rules for the fields a note keeps after its kind changes, and two devices could convert the same
  note in different ways at once. A new action that links to its memo keeps the trail with neither
  cost.

- **Contexts and tags.** GTD often filters next tasks by context, such as `@home`. They're left for
  later, because they apply to every kind of note, not just actions.

## Consequences

- The library format becomes version 2. Older versions of Commonplace open a version 2 library
  read-only.
- Every implementation maintains the tree alongside the notes: creating, moving, and removing nodes
  in the same commits as the notes they refer to.
- The apps need ways to sort the inbox, execute, plan, and review: the inbox; next tasks; the
  planning queue and stalled projects; and the open projects and the waiting, scheduled, someday,
  maybe, and recently closed actions.
- Contexts or tags need a later decision that covers every kind of note.
- Recurring actions and reviews on a schedule wait for spaced repetition.
