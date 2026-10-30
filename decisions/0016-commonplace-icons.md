# 0016 Commonplace icons

## Context

The Linux app took its icons from the system's Adwaita theme, and the Android app vendored copies of
the same Adwaita icons. Adwaita draws its symbolic icons as filled shapes. On a phone they read as
heavy and dark, and some of them, such as the ones for highlight, headings, and indentation, don't
say what they do. The owner also uses Obsidian, whose line icons come from Lucide.

The [platform parity rule](../AGENTS.md#commonplace-platform-parity) asks the two apps to look as
alike as each platform allows, so a change of icons on one app applies to the other.

## Criteria

- **Readable at a glance.** Each icon says what its control does, in the style people know from
  other note apps.
- **Light.** The icons sit lightly beside text on a phone and on a desktop.
- **Same on both apps.** A control shows the same icon on Linux and Android.
- **Follows the theme.** On Linux, icons take the theme's foreground color in light and dark styles.
- **One copy.** A change to an icon reaches both apps from one file.

## Decision

Both apps draw their interface icons from [Lucide]. One vendored copy lives in
`commonplace/vendor/lucide`, under the symbolic icon names that the Linux app looks up, such as
`systems.thoughtfull.Commonplace-trash-symbolic`. The Linux app adds the folder to its icon search
path in a debug build, and its package installs the icons beside its own. The Android build renders
them to images at each display density.

The app's own icons, for memos, journals, and the app's symbolic logo, follow Lucide's grid: a
24-unit square, 2-unit lines, and round caps and joins. A star is an outline while it's off, and
Lucide's star with a fill while it's on, so the two states differ in shape as well as color.

- **Readable at a glance:** Lucide covers each control with a familiar icon, and Obsidian uses the
  same set.
- **Light:** the icons are 2-unit lines, drawn 20 pixels wide on Android, in density-independent
  pixels.
- **Same on both apps:** both apps use the same files under the same names.
- **Follows the theme:** GTK 4.20 and later recolor the lines of symbolic icons, not only their
  fills, when each shape carries GTK's `transparent-fill foreground-stroke` classes.
- **One copy:** the Android build reads the folder that the Linux app installs.

## Rejected alternatives

- **Keep Adwaita.** It fails readable at a glance and light on a phone.
- **Lucide on Android only.** It fails same on both apps.
- **Lucide under Adwaita's names.** An app's icons can't override the system theme's icons of the
  same name, so the Linux app would still show Adwaita. It fails same on both apps.

## Consequences

- Icons that GTK and libadwaita draw themselves, such as window controls, dropdown arrows, check
  buttons, and the search entry's magnifier, stay Adwaita on Linux.
- A new icon comes from the same Lucide version, or follows its grid if Lucide has no fitting icon.

[lucide]: https://lucide.dev
