# Commonplace for Android

The native Android app uses Kotlin, Jetpack Compose, and the same Rust core as Linux. It works
without a server or a network connection. Both apps use the same icons, note titles, browser
filters, and action sections. On a phone, **Details** holds the panels that Linux shows beside the
editor. Steps and backlinks appear in a panel you can scroll below the note body, without opening
Details. The keyboard and toolbar hide that panel while you edit, giving the note body more room.

Details uses library toggles to move notes and keeps **Archive** at the bottom. Memo details offers
**Make Journal Entry**. Action fields follow the same order as Linux. The parent path and **Move…**
appear before the steps.

The Journals menu creates journals or opens them to edit, and a date range menu filters the
timeline. Entry rows use their journal's colored dot in Journals, All Notes, and Trash. A journal's
color menu shows each color beside its name, and schedules use the same descriptions as Linux. List
timestamps show the time today, a short date this year, or a full date for earlier years.

The editor uses live preview: it renders Markdown and reveals its syntax at the cursor. A single row
at the keyboard's top edge scrolls horizontally through formatting, list, and link commands.
**Outdent** and **Indent** come first. Formatting keeps the keyboard open, and the editor scrolls
the caret into view when the available space changes.

Tap a link to place the cursor and reveal its Markdown, and tap the icon immediately before the link
to follow it. Edit its label or target directly in the text, just like other Markdown. Tap a
checkbox to toggle it. Back retraces the notes and tabs you've visited and restores each note's
cursor and scroll position.

The **Inbox** processes one captured action at a time. Edit its text, due date, project flag, and
parent, then sort it onto Active, Waiting, Schedule, Someday, or Maybe. You can also finish,
archive, trash, or skip it. **Start over** brings skipped items back. Processing saves the latest
text before advancing. Done, Archive, Trash, and Skip stay between the item and the navigation tabs
while the item scrolls. Due and Project precede the parent name and its Move button.

## Features

- Browse memos, actions, journals, all notes, and the trash. Search and filter by date.
- Capture a memo, an inbox action, or a journal entry. Receive text from Android's share menu.
- Create a journal from **New journal**: choose its name and library, then edit its note and
  questions.
- Edit Markdown with formatting, checklists, list continuation and indentation, links, backlinks,
  and automatic saving.
- Handle actions through Execute, Inbox, Plan, and Review, with projects, steps, lists, dates,
  waiting reasons, and the planning queue. Add steps inline without leaving the parent, and move
  actions between parents. Drag a step's handle to reorder it; drop on another step's title to nest
  it there. The parent and Move row stay fixed while the steps scroll.
- Keep multiple libraries, name and hide them on this device, move notes, and merge libraries.
- Keep journals with all eight question types, fixed or random schedules, entry timestamps,
  structured answers, a timeline, numeric charts, and daily answer counts.
- Receive journal notifications, answer them, snooze them for 15 minutes or an hour, or skip them.
- Open `commonplace:note/` links and copy note links.
- Archive notes, move them to the trash, restore them, and delete them permanently.
- Sync libraries with Linux devices and other phones.

## Build

Build from the repository root:

```sh
devenv --profile android shell -- sh commonplace/android/build.sh
```

The Android profile provides Java 17, Gradle, the native build helper, Rust's Android targets, and
the Android software development kit (SDK). It includes platform 36, build tools `36.0.0`, platform
tools, and native development kit version `28.2.13676358`, and sets `ANDROID_HOME` for you. Entering
the profile downloads these packages on first use, so you don't need a separate SDK installation.

The build generates Kotlin bindings through UniFFI and builds native libraries for `arm64-v8a` and
`x86_64`. It then assembles a debug Android package, runs Android lint, and runs the unit tests. The
package appears at `commonplace/android/app/build/outputs/apk/debug/app-debug.apk`.

Android 8.0 or later can run the app. Open `commonplace/android` in Android Studio from the Android
profile's shell to use its editor and emulator. If you use your own SDK, set `sdk.dir` in
`commonplace/android/local.properties`; Git ignores that file. Gradle passes its resolved SDK and
native kit paths to the Rust build too.

To install on a device that you've connected for development:

```sh
devenv --profile android shell -- sh commonplace/android/build.sh installDebug
```

## Checks

Run the shared core's tests, including its mobile boundary tests:

```sh
devenv shell -- cargo test --manifest-path commonplace/Cargo.toml \
  -p commonplace-core --features mobile
```

Run the native bridge tests on an emulator with a separate app identifier:

```sh
devenv --profile android shell -- sh commonplace/android/build.sh \
  -Pcommonplace.applicationId=systems.thoughtfull.CommonplaceAgent.androidtest \
  connectedDebugAndroidTest
```

These tests check native library loading, persistence, action hierarchy, inherited trash, links, and
Unicode editor offsets. Use an emulator for agent testing, and don't launch the owner's app.

## Test library

Choose **Libraries → Load test library** to add the same Personal and Work libraries as Linux's
`test_library` example. This action appears only in **Libraries**. The samples include formatted
titles, links, lists, archived and trashed notes, actions in each state, and journals with all eight
question types, schedules, and answers. Dates count from the day you load them. Loading the samples
keeps existing notes and libraries, and the app offers the action only once to avoid duplicates.

You can also capture your own notes, or sync libraries from your Linux devices.

## Sync

Choose **Sync** to pair the phone with other devices, as the Linux app's Sync dialog does.
[Decision 0013](../../decisions/0013-commonplace-sync.md) explains the design.

- Copy or share the address under **This device**, and add it on the other device.
- Choose **Add device**, then enter the other device's address and a name for it.
- Open the device and turn on **Share** for each library to sync with it. The other device then
  lists an offer, which it accepts into a new library or merges into an existing one. Offers from
  other devices appear here the same way.

With **Sync automatically** on, the phone syncs shortly after each change, when you open the app,
and every few minutes while it's open. In the background, a job syncs about every 15 minutes when a
network is available. Android can delay that job to save battery. **Sync now** syncs with every
device at once, and each device shows when it last synced or why it couldn't.

Sync uses n0's public relays unless you enter your own under **Relay server**. The phone keeps its
sync key and devices in a directory that Android never backs up.

## Storage and notifications

Libraries use the core's Loro snapshots and append-only log in the app's private files directory.
Library names, view filters, and prompt preferences stay in Android's device preferences. The app
uses the network only for sync, doesn't request shared-storage permissions, and turns automatic
Android backup off.

Allow notifications and turn on **Prompt here?** in a journal's settings to receive its prompts.
Android can delay inexact alarms while saving battery. Prompts respect notification permissions and
do-not-disturb settings, and don't show entry contents.

[Decision 0011](../../decisions/0011-commonplace-android.md) explains the platform boundary and
storage choices. Curios, zettels, and spaced repetition remain future work on both platforms.

**Libraries → Add quick capture to Quick Settings** adds a tile to the panel you pull down from the
status bar. The tile opens capture over your current app and returns there when you save or cancel.
You can also add **Quick capture** through the Quick Settings panel's **Edit** control.

**Libraries** offers **Add quick capture to home screen**. Choose it to add the capture icon. You
can also hold the app icon and drag its **Quick capture** shortcut onto the home screen. The
shortcut opens a capture dialog over your current app. Saving or canceling returns to that app.
Enable **Quick capture notification** in **Libraries** for the same capture dialog from an ongoing
notification. Quick capture remembers the note type and library from your last successful save,
including a journal entry's journal and library. Canceling leaves those defaults unchanged. Capture
preserves the current screen and search; the app has no floating add button.

New journal entries start with the date as their title. Drag across a numeric chart to inspect the
date and value of the closest recorded point. Choice charts show a row per option, with a count on
the right: dots mark selected days, dashed rings mark missing answers, and crosses mark no.
