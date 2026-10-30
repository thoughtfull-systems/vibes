# Lucide icons

These icons come from [Lucide 0.460.0][lucide], the line icons that Obsidian uses. They use the
[Internet Systems Consortium (ISC) license][license], and the `LICENSE` file contains it. Both apps
draw their interface icons from this folder.

Each file keeps Lucide's geometry, with line wrapping for this repository, under the symbolic icon
name the Linux app looks up. Each shape carries GTK's `transparent-fill foreground-stroke` classes,
without which GTK fills the lines instead of recoloring them. The starred-filled icon is Lucide's
star with a fill added, which shows a note's star while it's on.

The Linux app finds the folder from the source tree in a debug build, and its package installs the
icons with its own. The Android app's `build-icons.sh` renders them at Android's display densities.

[license]: https://lucide.dev/license
[lucide]: https://github.com/lucide-icons/lucide/tree/0.460.0
