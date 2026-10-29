# 0014 Commonplace starred actions

## Context

[Decision 0004](0004-commonplace-actions.md) defines the next tasks that the Execute mode lists:
open tasks on the active list that start within a week and aren't queued for planning, with one such
step from each project. Execute works breadth-first. It shows the most pressing step of every
project in progress, so the owner can move between projects and keep each one going.

Some owners work depth-first instead. They pick a project or a task that's on their mind and go deep
on it. A project's page already lists its steps, so the owner doesn't need Execute to show them. The
owner needs a short way back to the few actions they've chosen. Today they have to find each one
again through Review or search.

## Criteria

- **Quick to reach.** One click in Execute opens any action the owner has chosen.
- **Any action.** The owner can choose projects and tasks alike.
- **Nothing pressing hides.** The next tasks stay in Execute beside the chosen actions.
- **Low friction.** Choosing an action and letting it go each take one click.
- **Same on every device.** The owner's phone and computer agree on the chosen actions.
- **Merges well.** Two devices that change the chosen actions at the same time end up in a valid
  state.

## Decision

The owner can star any action: a project as well as a task. Execute lists the starred actions at the
top, under a **Starred** heading, and the next tasks after them.

- **Storage.** A starred action has a `starred` timestamp in its action fields: the time its owner
  starred it. Removing the key removes the star. It's a new optional key, so the
  [library format](../commonplace/spec/library-format.md) stays at version 3, and a device that
  doesn't know the key shows Execute as before.
- **Which actions.** Starred lists every starred action that, like each project it belongs to, is
  open and outside the archive and the trash, whatever its list, start date, or place in the
  planning queue, because a star is the owner's explicit choice. It orders and colors them the way
  decision 0004 orders and colors every list, so an action that hasn't started yet shows dimmed.
- **Next tasks.** Below **Starred**, Execute lists the next tasks as decision 0004 defines them,
  without the ones that already show under **Starred**, so no action shows twice.
- **Opening.** Selecting a starred row opens the action, as every list does. A starred project opens
  to its page, with its steps.
- **Controls.** The details sidebar of every action has a star button. Each row in Execute, Plan,
  Review, the actions search results, and a project's steps has a star button too, filled for a
  starred action, that toggles the star.
- **Closing.** Finishing, archiving, or trashing an action leaves `starred` in place. The action
  leaves **Starred** because it's no longer open, and reopening it brings it back.

Against the criteria:

- **Quick to reach:** every starred action sits at the top of Execute, the mode that opens first,
  one click away.
- **Any action:** the key lives in the action fields that projects and tasks share.
- **Nothing pressing hides:** the next tasks keep their place below the starred actions, with the
  usual colors for tasks due soon or overdue.
- **Low friction:** a single click of a star in any action list or sidebar adds or removes the
  action.
- **Same on every device:** the key lives in the library and syncs with it.
- **Merges well:** `starred` is one key in the action's map, so concurrent changes resolve by Loro's
  last write, and every outcome is a valid state.

## Rejected alternatives

- **Focused projects.** Focusing a project would list every available step inside it in Execute.
  That repeats the project's page inside Execute, and it can't hold a task on its own. It fails the
  any action criterion, and adds a view the owner already has.
- **A Starred mode.** A fifth mode beside Execute, Plan, Review, and Inbox would keep Execute
  unchanged. The owner would then switch modes to see the starred actions and the next tasks, which
  fails the quick to reach criterion.
- **Starred actions only.** Showing nothing but the starred actions in Execute while any action has
  a star would give the shortest list. A task due tomorrow in another project would vanish until the
  owner removes every star. It fails the nothing pressing hides criterion.
- **Sequential and parallel projects.** A flag on each project, as in OmniFocus, would let a
  parallel project put all its available steps into Execute. It describes how a project's steps
  depend on each other, not what the owner has on their mind, and it can't mark a task. It fails the
  any action and low friction criteria.
- **Stars as a device setting.** Keeping the starred actions in each device's settings would keep
  them out of the library. The owner would then star the same action once on each device, which
  fails the same on every device criterion.
- **A Boolean flag instead of a timestamp.** A flag says as much as the app uses today. A timestamp
  costs nothing more and lets a later version order starred actions by when they gained a star, as
  the planning queue's timestamp does.
- **Remove the star when an action closes.** Clearing `starred` in the same commit as closing would
  keep the library tidy, but an action reopened by mistake would lose its star, and the key has no
  effect while the action stays closed anyway.
- **Tags.** A tag such as `starred` could mark any kind of note. Decision 0004 leaves tags for a
  later decision that covers every kind of note, and a star needs no name, so it doesn't wait for
  one.

## Consequences

- The library format specification gains the `starred` key in the action fields. Decision 0004's
  description of next tasks still holds, minus the starred tasks that Execute already shows.
- The Linux and Android apps ship the star buttons and the two sections of Execute in the same pull
  request, following the platform parity rule in `AGENTS.md`.
- The sample library gains a starred project and a starred task once the feature is ready to ship.
- The description of Execute in the Commonplace README gains starred actions.
