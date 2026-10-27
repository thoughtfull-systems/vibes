# 0012 Commonplace link activation

## Context

Commonplace uses live Markdown editing on Linux and Android. Following a link on the first tap
prevents the editor from treating links like other Markdown: placing the cursor in an item reveals
its syntax so the owner can edit it. Android also needs an action that doesn't depend on a physical
keyboard.

## Criteria

- **Consistent editing.** Placing the cursor in a link works like placing it in other Markdown.
- **Deliberate navigation.** Editing a label or target doesn't unexpectedly leave the note.
- **Visible action.** Following a link needs no knowledge of keyboard shortcuts.

## Decision

On both platforms, an ordinary click or tap places the cursor in the link and reveals its syntax.
Show an open-link icon immediately before every link, including links whose syntax is visible.
Clicking or tapping that icon or the gap between it and the link follows the link. Keep the icon
smaller than the text and preserve the text line height. The owner edits the label and target
directly in the Markdown; links don't need a separate edit mode or edit button. Keep Linux's
Control-click and keyboard shortcuts as optional ways to follow a link.

On Android, keep formatting commands in one row that the owner can scroll. Reserve space for that
toolbar outside the editable area and scroll the caret into view when the keyboard, toolbar, or
selection changes. Hide steps and backlinks while the note editor uses the keyboard and toolbar,
then show them again when those controls close. The separate step-entry field stays visible while
its keyboard opens.

- **Consistent editing:** links reveal their syntax through the same cursor interaction as other
  Markdown.
- **Deliberate navigation:** following a link requires its own action.
- **Visible action:** both platforms put the open action beside each link.

## Rejected alternatives

- **Put Open link in the footer or toolbar.** This separates the action from its link and fails
  visible action when the owner edits far from those controls.
- **Follow links on the first tap.** This fails consistent editing and deliberate navigation by
  making links behave differently from other Markdown.
- **Offer separate Edit and Open buttons.** This adds an editing step that other Markdown doesn't
  need and fails consistent editing.
- **Use only a long press or a keyboard shortcut.** This fails visible action because the editor
  doesn't show how to follow the link.

## Consequences

- Both apps' back buttons return through visited notes and listings, skipping deleted notes. On
  Linux, Escape returns directly to the current listing. Android also restores each note's cursor
  and scroll position.
- Link activation changes no stored Markdown or library format. Linux uses child anchors for inline
  controls and keeps undo history in a separate text buffer, since GTK counts anchors in offsets and
  records their deletion without recording their insertion. Android uses a display transformation
  that maps icon positions to source offsets.
