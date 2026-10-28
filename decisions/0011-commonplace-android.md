# 0011 Commonplace on Android

## Context

[Decision 0001](0001-languages-and-technologies.md) chooses Kotlin and Jetpack Compose for Android,
with bindings that UniFFI generates for the Rust core. Commonplace's Linux implementation now has
spaces, actions, journals, links, and a Markdown editor. Android needs the same rules and library
format, with navigation, capture, notifications, and storage that fit a phone.

## Criteria

- **Compatible data.** Both platforms read and write the same libraries, without translating notes
  through another storage format.
- **One implementation of rules.** Android uses the core's action views, scheduling, journal
  answers, link handling, and editor operations.
- **Offline operation.** Opening the app, editing notes, and answering prompts need no network. Only
  sync with other devices does.
- **Platform integration.** Android sharing, note links, notifications, and document pickers use the
  platform's services.
- **Responsive editing.** Disk writes and library operations stay off the main thread, and leaving
  an editor doesn't cancel its pending writes.

## Decision

Build the Android app in `commonplace/android`. An optional `mobile` feature on `commonplace-core`
exports a typed UniFFI interface. The interface owns the core's `Spaces` behind a lock and copies
note data into records that Kotlin can read. Android owns one instance per app process and runs
operations on a serial background dispatcher. The default Linux build doesn't enable the bindings
feature.

Store libraries in the app's private files directory, using the core's existing snapshots, update
log, and writer locks. Keep library names, visibility, capture choices, and view preferences in
Android's device preferences. Don't include libraries in Android's automatic backup service.

Generate Kotlin bindings from the host library, then build Android libraries for devices with
`arm64-v8a` processors and emulators with x86-64 processors. Gradle runs this step before compiling
Kotlin, so bindings and native code come from the same sources. The Android development profile adds
the software development kit, native development kit, cross-compilation targets, and build tools
without changing Linux's Rust toolchain. Gradle passes its resolved kit paths to the Rust build, so
both builds use the same installation, including when Android Studio sets a local kit path.

Use Compose for browsing, editors, forms, and charts. The core supplies Markdown spans, list edits,
links, and checkbox edits. The boundary converts Rust byte offsets to Kotlin UTF-16 code units,
including characters that take two code units. The editor saves through the model's scope, which
survives navigation and configuration changes.

Use Android notifications for journal prompts, with actions to answer, skip, and snooze. Android
converts local calendar times to timestamps, while the core computes fixed and random schedules and
records handled prompts. Schedule an inexact alarm for the next event. Android can delay it while
saving battery; the app doesn't request exact-alarm access. Reschedule after a reboot, a package
update, or a change to the clock or time zone. Notifications respect the system's notification
permissions and do-not-disturb settings.

Follow Linux's interface as well as its rules: use the same icon artwork, title rendering, browser
filters, and action sections. Keep the note body as the main editing surface. On a narrow screen,
move the desktop's details panel into a Details sheet and keep editing commands at the keyboard's
top edge. Steps and backlinks stay below the editor, as on Linux. Add steps with an inline field
that keeps the parent open and reveals the new step. Submit with the keyboard action, without a
second accept button. Keep the parent and Move controls outside the steps' scrolling area. Step drag
handles reorder siblings; dropping on a title nests the dragged action, including its children. The
core supplies both operations. Neither platform shows an outline sidebar; Linux offers action modes
in a dropdown. Live preview is the only editor mode: it removes syntax from the display and maps
cursor offsets back to the stored Markdown, so hidden link targets take no space. The toolbar
exposes commands in one horizontally scrolling row of icons, without menus that take focus.
[Decision 0012](0012-commonplace-link-activation.md) defines link editing and activation on both
platforms. The Inbox follows Linux's one-at-a-time processing workflow and saves edits before
sorting or skipping an item. Its processing buttons stay between the scrolling content and
navigation.

Entry details label the journal link and place a date selector beside a time field. Both platforms
show the journal's colored dot at that link. Scale answers use numbered buttons between their
endpoint labels, and yes or no answers show Yes first. Android uses neutral gray for answer and
library choices, conversion menus, and secondary Inbox controls; Inbox Trash uses red.

Both details panels end with creation and modification timestamps, then Archive. Both journal
timelines render Markdown in entry previews and show summaries of the structured answers. Before
inspection, chart headers show a value range or symbol legend. A desktop pointer or phone drag
reveals the nearest recorded value, or every option's state for that day, with a vertical marker.
The desktop restores the legend when the pointer leaves the plot. Android uses the selected date
range for numeric and choice graphs, including days without answers.

Keep one sample-library generator in the shared core. The Linux command-line example and Android's
**Load test library** action both use it, with dates relative to when they run. Loading samples
preserves existing notes. Android doesn't expose a separate snapshot exchange interface; both apps
use the shared sync transport that [decision 0013](0013-commonplace-sync.md) describes.

The Linux test launcher also accepts `--date` to generate historical samples. The core scopes a
fixed timestamp to sample generation on the calling thread, including creation, modification, action
states, and entry times. It restores the clock after generation, including errors and panics, so
existing notes and later edits keep their real timestamps. The generator uses noon Coordinated
Universal Time (UTC) on the selected day; both apps keep their real clocks when browsing the data.

Against the criteria:

- **Compatible data:** both platforms use `commonplace-core` and its existing storage format.
- **One implementation of rules:** the bindings expose core operations instead of implementing them
  again in Kotlin.
- **Offline operation:** all data stays on the device, and every operation except sync works without
  a network.
- **Platform integration:** intents, notifications, alarms, and the document picker connect the app
  to Android.
- **Responsive editing:** background operations serialize writes, and the model keeps editor writes
  alive when a screen closes.

## Rejected alternatives

- **Implement the rules again in Kotlin.** This would duplicate behavior that already exists in the
  core and fail the one implementation of rules criterion.
- **Run the Linux interface in a web view.** The Linux interface needs GTK, and a second web
  interface would fail the platform integration criterion that decision 0001 establishes.
- **Store Android notes in a separate database.** Translating between that database and Loro would
  introduce a second source of truth and fail the compatible data criterion.

## Consequences

- Android builds need the Android development kit and native development kit, Gradle, Java, and the
  Rust Android targets. The Android README documents their versions and build commands.
- An Android library stays in private app storage, and reaches other devices only through the sync
  that decision 0013 describes. The bindings sync through the same lock as every other operation,
  and report each write that commits to a library, so automatic sync sends it. The app runs sync
  while it's in the foreground and for a minute after, and a WorkManager job syncs every 15 minutes,
  with a network, while automatic sync is on. Android can defer that job while saving battery.
- The device's sync key and configuration live in Android's no-backup directory, so neither
  automatic backup nor device transfer copies the key to another device.
- Changes to the exported interface require regenerating the bindings. Generated Kotlin and native
  libraries stay in build directories and don't enter source control.
- Android has no desktop tray or command-line capture window. Sharing and in-app capture preserve
  the current view. A home-screen shortcut and an optional ongoing notification open capture in a
  separate translucent activity with its own task. A Quick Settings tile opens the same activity.
  Saving or canceling removes that task and returns to the app underneath. Capture defaults to the
  last successfully saved type and space, and remembers the journal for entries. The main app has no
  floating add button. Creating a journal remains a separate name-and-library dialog.
