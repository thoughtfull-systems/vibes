# Agent guide

Instructions for coding agents working in this repository.

## Environment

The development environment comes from [devenv](https://devenv.sh). Run commands inside it with
`devenv shell -- <command>`, or enter it with `devenv shell`. Entering the shell installs the git
hooks.

- `devenv.nix` defines the packages, environment variables, and git hooks.
- `devenv.yaml` pins the inputs, and `devenv.lock` records their versions.
- `.pre-commit-config.yaml` is a generated file. Don't edit or commit it.

## Tool configuration

Keep configuration for tooling in `.config/`, not in the repository root. Most tools don't look
there on their own, so point each one at its file from `devenv.nix`. Two locations can't move:
editors read `.editorconfig` only from the root, and GitHub reads workflows only from
`.github/workflows/`.

- `.config/vale/vale.ini`: prose linting for markdown.
- `.config/vale/styles/Google/`: the vendored Google style. Don't edit these files by hand. Run
  `vale sync` to update them. Sync replaces the whole folder, so the style's MIT license lives
  beside it in `.config/vale/styles/LICENSE-Google`.
- `.config/vale/styles/Project/`: project rules, including the general entries from the Google word
  lists and the multi-word entries from write-good's `TooWordy` rule.
- `.config/vale/styles/config/vocabularies/Project/`: accepted and rejected terms.
- `.config/vale/styles/write-good/`: the vendored write-good style, which checks for weasel words,
  clichés, and weak sentence openings. Treat it like the Google style. Its MIT license lives in
  `.config/vale/styles/LICENSE-write-good`.
- `.config/markdownlint.yaml`: markdown structure rules.
- `.config/rustfmt.toml`: formatting for Rust. Neither `cargo fmt` nor editors find it on their own,
  so format with `prek run rustfmt --all-files`, or pass `--config-path` to `rustfmt`.
- `.config/gitlint`: commit message rules.
- `.config/lychee.toml`: link checking.
- `.config/taplo.toml`: formatting for `.toml` files.
- `.config/yamlfmt.yaml`: formatting for `.yaml` files.
- `.config/yamllint.yaml`: linting for `.yaml` files.

Name YAML files with the `.yaml` extension, except under `.config/vale/`, where they must end in
`.yml` because Vale ignores rule files named any other way. The `yaml-extension` hook checks both.

## Checks

Every commit runs the git hooks in `devenv.nix`. Any failure blocks the commit. To run them all by
hand:

```sh
devenv shell -- prek run --all-files
```

Some hooks fix files in place and then fail: trailing whitespace, final newlines, formatting, and
sorted files. Review the change, stage it, and commit again. Don't bypass the hooks with
`--no-verify`.

The `Hooks` GitHub Actions workflow runs the same hooks, and checks each commit message with
gitlint, when a pull request enters the merge queue. To save time, it skips the hooks on the pull
request itself, so run them by hand before you queue it. If they fail, GitHub removes the pull
request from the queue. Read the failed run, fix the problem, push, and queue it again.

Links can break without a commit, so a GitHub Actions workflow checks them on every push and pull
request, and once a week. To check them by hand:

```sh
devenv shell -- lychee --config .config/lychee.toml .
```

## Licensing

The code uses the Mozilla Public License 2.0 and is "Incompatible With Secondary Licenses." Start
each new source code file with this Software Package Data Exchange (SPDX) license identifier, in the
comment syntax of its language:

```text
SPDX-License-Identifier: MPL-2.0-no-copyleft-exception
```

In a script, put it on the line after the shebang. The `license-header` hook checks the first five
lines of each file whose type it lists. When you add a language, add its [identify] tag to the
hook's `types_or` in `devenv.nix`.

Vendored third-party code keeps its own license. When you vendor code, add its license file too. If
the tool that updates the vendored copy replaces its folder, put the license file beside the folder.

## Decisions

Record design and architectural decisions in `decisions/`. Before making a choice that affects more
than one app, defines a data format or protocol, or would be hard to reverse, read the existing
decisions and follow them. [Decision 0000](decisions/0000-record-decisions.md) describes when to
write a decision, the format, and the process.

## Source comments

Write a comment only when it gives the reader information that the code and its names can't. Valid
reasons:

- A fact from outside the code that forces its shape, such as a tool's quirk or an upstream bug.
- A reason for code that's unusual or not idiomatic.
- A hazard: editing this code breaks something that isn't visible from here.

Before writing a comment, check it against these rules:

- Don't describe what the code, a tool, an option, or a flag does. If the name already says it, the
  comment adds nothing.
- Don't repeat documentation from elsewhere, including this file. Link to it if needed.
- Put a warning where the risky edit happens. If that file can't hold a comment, document the
  warning here instead.
- Read the code without the comment. If a competent reader loses nothing, delete the comment.

## Line length

Wrap lines at 100 characters in every file: prose, source, comments, and configuration. Fill each
line close to the limit instead of breaking early.

Enforce the limit with a tool that fails the commit whenever possible. When you add a language or
file type, configure its formatter or linter for 100 characters. The `line-length` hook in
`devenv.nix` catches any text file that no other tool checks. If you have to exclude a file from
that hook, keep its lines within the limit by hand.

The hook allows no exceptions, so restructure content that's too long. In markdown, use
reference-style links for long URLs. In code, split long strings.

## Ordering

Sort lists alphabetically instead of grouping or categorizing them. This applies to imports,
dependency lists, package lists, configuration keys, attribute sets, and similar collections. Keep
another order only when the order changes behavior, and add a comment that explains why.

In Nix files, wrap each new sorted attribute set or list in `keep-sorted` markers so the hook
enforces the order. Use `block=yes` so the hook keeps each multi-line entry together:

```nix
{
  # keep-sorted start block=yes
  alpha = {
    enable = true;
  };
  beta = 1;
  # keep-sorted end
}
```

Markers can nest. If the entries have blank lines between them, add `newline_separated=yes` to the
start marker.

## Writing markdown

Vale checks prose against the Google developer documentation style and parts of write-good, and
every enabled rule is an error. Before you commit, run `vale <file>` in the shell and fix each
alert.

- Write in active voice and present tense.
- Use sentence case for headings, with no closing period.
- Spell out acronyms on first use.
- Use contractions such as "don't" and "it's."
- Avoid Latin abbreviations such as "e.g." and "i.e."

For a project term that fails the spell check, add it to `accept.txt` in the vocabulary folder. Vale
then enforces the capitalization given there. To ban a term, add it to `reject.txt`.

Turn off a rule for a section only when the text can't comply, such as quoted material:

```markdown
<!-- vale Google.Passive = NO -->

Quoted text.

<!-- vale Google.Passive = YES -->
```

The markdownlint hook enforces structure: heading levels, blank lines around lists and code blocks,
and a language on each fenced code block.

The `mdformat` hook fills each paragraph's lines up to 100 characters and overrides manual line
breaks.

## Commonplace test library

`commonplace/core/examples/test_library.rs` builds the sample library that `commonplace-test` runs
the app on. When a Commonplace feature is ready to ship, add samples that exercise it. Don't add
samples for a feature that's still changing.

## Commonplace test runs

The owner runs two copies of Commonplace: the installed app, with the app ID
`systems.thoughtfull.Commonplace` on their own library, and `commonplace-test`, with
`systems.thoughtfull.CommonplaceTest`, which can also run on that library. Commonplace runs one
instance per app ID, so a launch, a URI, or a capture command under either ID goes to the owner's
running copy. Never use either ID. Don't open windows on the owner's display either, because they
take keyboard focus while the owner types.

Instead, run the app headless on GTK's Broadway backend, which draws windows to a web page, on a
library in a temporary directory built with the test library example. Give each test session its own
app ID under `systems.thoughtfull.CommonplaceAgent`, Broadway display, and port, so sessions running
at the same time don't hand their work to each other. The commands below derive the ID from the
temporary directory and pick a random display. If the daemon reports that the display or port is in
use, pick another number. Run these commands inside `devenv shell`, so the daemon comes from the
same GTK build as the app. A daemon from another build refuses the connection.

```sh
dir=$(mktemp -d)
cargo run --manifest-path commonplace/Cargo.toml --example test_library -- "$dir/library"
id=systems.thoughtfull.CommonplaceAgent.$(basename "$dir" | tr -cd '[:alnum:]')
display=$((RANDOM % 900 + 100))
gtk4-broadwayd -a 127.0.0.1 -p $((8000 + display)) ":$display" &
GDK_BACKEND=broadway BROADWAY_DISPLAY=":$display" COMMONPLACE_APP_ID="$id" \
  COMMONPLACE_LIBRARY="$dir/library" commonplace/target/debug/commonplace
```

Open `127.0.0.1` at that port in a browser to see and drive the window. GTK warnings go to standard
error.

- Broadway clicks wherever the pointer last moved, so hover over a spot before clicking it.
- The page can lag behind the window. Move the pointer to make it redraw before trusting a
  screenshot.
- The app keeps running after its window closes. Stop it by its process ID when the test ends.

## Sorted files

A hook sorts the lines of `.gitignore` and the vocabulary files, ignoring case. Every line takes
part in the sort, so don't add comments or blank lines to these files.

[identify]: https://github.com/pre-commit/identify
