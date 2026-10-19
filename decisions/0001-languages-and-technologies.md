# 0001 Programming languages and technologies

## Context

The README sets out the principles for every app: local-first, available on Linux, Android, and the
web, with room for more platforms, and with implementations that interoperate through shared data
formats rather than shared code. It leaves the guidelines for languages and technologies to a later
decision. Commonplace, the first app, needs them now.

The main fork in the road is between one cross-platform implementation and a separate native app for
each platform. A cross-platform toolkit draws its own widgets or wraps a web view, so it never fully
matches the conventions of any platform: navigation, text input, accessibility, sharing,
notifications, and system integration all feel slightly off. The repository's owner prefers a native
app on each platform over one implementation that's mediocre everywhere.

Native apps multiply the code, though. Logic that has nothing to do with the user interface, such as
parsing, data access, merging, and scheduling, would then exist once per platform, and the copies
would drift apart.

## Criteria

- **Native feel.** Each app uses its platform's own toolkit and follows its conventions.
- **Interoperability.** Implementations on different platforms read each other's data and sync with
  each other.
- **Room for more platforms.** The choices don't rule out macOS, Windows, or iOS later.
- **One source of truth for logic.** Logic that doesn't depend on the platform doesn't have to be
  written, tested, and fixed once per platform.
- **Small toolbox.** A single maintainer, working with coding agents, can keep every language and
  toolchain in working order.

## Decision

Build a native app for each platform, with that platform's own language and toolkit:

- **Linux:** a Rust app built with GTK 4 and libadwaita.
- **Android:** a Kotlin app built with Jetpack Compose.
- **Web:** a TypeScript app. Each web implementation chooses its own framework, if any.

An app may share logic that doesn't touch the user interface through a Rust library, its core. Each
platform reaches the core in its own way. Rust apps link it directly, Kotlin apps call it through
bindings that [UniFFI] generates, and web apps load it as WebAssembly. The core never contains user
interface code. Each app decides whether it has a core; an app whose implementations stay
independent has to specify its formats precisely enough for them to interoperate.

Adding a language or a platform toolkit to this list takes a revision of this decision.

- **Native feel:** every user interface uses the platform's own toolkit.
- **Interoperability:** the data formats stay the contract between implementations, as the README
  requires, and a shared core makes it hard for two implementations to disagree about them.
- **Room for more platforms:** the same UniFFI tool generates Swift bindings, and Rust builds for
  macOS, Windows, and iOS, so a later Apple or Windows app can reuse the same core.
- **One source of truth for logic:** the core holds the logic once, with one set of tests.
- **Small toolbox:** the list holds three languages, one per platform family, and Rust does double
  duty as the core language and the Linux language.

## Rejected alternatives

- **A cross-platform user interface toolkit,** such as Flutter, Compose Multiplatform, React Native,
  or Tauri. Any of these would cover all three platforms with one code base, but each fails the
  native feel criterion on at least one platform, which is the outcome this decision exists to
  avoid.
- **Fully independent implementations, with no shared code.** This tests the principle that formats
  are the contract most strictly, but it fails the one source of truth for logic criterion: every
  rule in the format, every merge rule, and every algorithm gets written and debugged three times.
  An app may still choose this path.
- **A shared core in C or C++.** Every platform can call C, but the core would need manual memory
  management and handwritten bindings, and Rust's crates for the problems at hand, such as
  conflict-free replicated data types, have no equal there. It fails the small toolbox criterion.
- **Kotlin Multiplatform for the shared core.** It shares Kotlin code with Android naturally, but
  its WebAssembly and native targets are less mature than Rust's, and the Linux app would then need
  a second language. It fails the small toolbox criterion.

## Consequences

- The development environment provides the Rust toolchain, GTK 4, and libadwaita. It needs Android
  and web toolchains once those implementations begin.
- The `license-header` hook covers Rust files, and the `rustfmt` and `clippy` hooks check them.
- Each app with a core needs a bindings build for Kotlin and a WebAssembly build for the web.
- An app's core exposes an interface that every binding can express, which rules out some Rust
  idioms, such as generic types and borrowed references, at that boundary.

[uniffi]: https://mozilla.github.io/uniffi-rs/
