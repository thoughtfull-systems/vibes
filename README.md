# Vibes

A monorepo of personal, vibe-coded applications. Each one is a local-first app for Linux, Android,
and the web, the platforms I use every day.

## Principles

- **Local-first.** Each app keeps its data on the device and works fully offline. Syncing between
  devices is optional, and no app depends on a server to function.
- **Linux, Android, and the web.** Every app ships on all three platforms.
- **Room for more platforms.** Choose languages, libraries, and data formats that don't rule out
  native macOS, Windows, or iOS versions later.
- **Compatible implementations.** An app's implementations can differ in language or technology from
  one platform to the next, but they must work together. Any implementation can read the data
  another writes and sync with it. The shared data formats and protocols are the contract between
  implementations, so define them independently of any one implementation.
- **Diversity within bounds.** General guidelines set the allowed languages and technologies. Within
  those bounds, each app, or each implementation of an app, can choose what suits it best.

## Languages and technologies

The guidelines for languages and technologies are still to come.

## Development

The development environment comes from [devenv](https://devenv.sh). Enter it with `devenv shell`,
which also installs the git hooks that check every commit.

[AGENTS.md](AGENTS.md) describes the tooling and the conventions for code, comments, and
documentation. It's written for coding agents, but it applies to anyone working in the repository.

## License

The code uses the [Mozilla Public License 2.0](LICENSE) (`MPL-2.0-no-copyleft-exception`). It
applies to every file in the repository except vendored third-party code, such as
`.config/vale/styles/Google/`, which keeps its own license:

<!-- vale off -->

> This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0. If a copy of
> the MPL was not distributed with this file, You can obtain one at <https://mozilla.org/MPL/2.0/>.
>
> This Source Code Form is "Incompatible With Secondary Licenses", as defined by the Mozilla Public
> License, v. 2.0.

<!-- vale on -->
