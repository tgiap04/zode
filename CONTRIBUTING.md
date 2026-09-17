# Contributing to Zode

Thanks for being here.

Zode is a fork of [Zed](https://github.com/zed-industries/zed) maintained by one person.
There is no company behind it, no hosted forum, and no Contributor License Agreement to
sign — contributions are made under this repository's own licence, see
[`LICENSE-GPL`](./LICENSE-GPL) and [`LICENSE-APACHE`](./LICENSE-APACHE).

It also means review can be slow. That is worth knowing before you spend a weekend on
something: open an issue first for anything larger than a fix, and you will find out
early whether it fits.

Bugs in the editor core, the LSP layer, the terminal or the debugger usually belong
[upstream](https://github.com/zed-industries/zed) — this fork inherits fixes from there.
What belongs here is anything in the parts this fork owns: the project rail, agents,
the database client, containers, git tooling, the account, and the way any of it is
documented.

## What this fork is, and what that means for your change

Zode exists to be an editor that works entirely on your machine: no telemetry, no
mandatory account, nothing phoning home that you did not ask for. Everything it added —
agents, databases, containers, git tooling — was added on that basis.

So the first question asked of a change is not "is the code good" but "does it hold that
line". A change can be well-written, well-tested and still be declined for reintroducing
something this fork removed on purpose. Concretely:

- **A new outbound network call needs a reason, and it needs disclosing.** If your change
  makes Zode talk to anything, it must also update the list in
  [`legal/privacy-policy.md`](./legal/privacy-policy.md) and the table in the
  [README](./README.md) in the *same* pull request. Those documents claim to be complete.
  They are only true if every change that touches the network keeps them true.
- **Nothing gets collected.** No usage analytics, no event tracking, no crash upload.
  `telemetry::send_event` is an empty function and is meant to stay one.
- **The account stays optional and stays out of the way.** It exists for one thing:
  end-to-end encrypted syncing of your own settings, keymap and extension list. It is not
  a login wall and not an identity for anything else. `script/check-account-no-telemetry`
  asserts this against the dependency graph rather than by grepping for calls — a graph
  cannot be satisfied by a comment, and does not rot when somebody refactors.
- **Agents stay agents.** They are terminal sessions running a CLI you installed. Zode
  holds no model credentials and makes no model API call, and a change that gives it
  either is a different product.

If you think one of these lines is in the wrong place, that is a conversation worth
having — open an issue and make the argument. What will not work is a pull request that
crosses it quietly.

## Before you send

Build and lint with the repo's own script, not `cargo clippy` directly:

```sh
./script/clippy                 # whole workspace
./script/clippy -p editor       # one crate, much faster
```

It runs clippy in release across all targets and features with `--deny warnings`, then
`cargo machete` and `typos` if you have them installed. CI runs the same thing, so a clean
run locally means a clean run there.

Tests:

```sh
cargo test -p <crate>
```

Run the suites for what you touched, plus anything downstream of it. `crates/ui` and
`crates/workspace` are depended on by nearly everything, so a change there is worth a
wider sweep.

A few other checks exist and are worth knowing about if you are near them:
`script/check-glibc-floor` (Linux builds must keep working on glibc 2.35),
`script/check-licenses`, `script/check-links`, `script/check-keymaps`.

Coding standards live in [`.rules`](./.rules) — Rust conventions, GPUI patterns, how
comments are expected to be written, and the pull-request title format. Read it before
your first change rather than after review. It is not restated here on purpose; two copies
of a rule drift.

## Sending changes

Working code beats a long discussion thread. The best way to get a change looked at is to
open a pull request.

To give it the best chance:

- **Make sure it is wanted.** Bug fixes are always welcome. For a feature, open an issue
  first — this is a small project and a rejected feature is a wasted weekend for you.
- **Say what you are solving**, and why it matters, in the description.
- **Include tests.** A non-trivial change without them is unlikely to land.
- **Attach screenshots or a recording** if it changes the UI, in both light and dark.
- **One thing per pull request.** A bug fix plus two features plus a refactor is four
  reviews wearing a trench coat.
- **Understand what you are submitting.** AI assistance is fine and is used here too; a
  change whose author cannot explain it is not.

If you are stuck partway through something already agreed, open the pull request early and
say so. Reviewing code in hand is easier than reviewing a description of it.

## UI/UX checklist

When your change affects the interface, walk this list:

**Accessibility and ergonomics**

- Do all keyboard shortcuts work as intended?
- Are shortcuts discoverable (tooltips, menus, docs)?
- Do all mouse actions work (drag, context menus, resizing, scrolling)?
- Does it look right in both light and dark mode?
- Are hover, focus and active states clear and consistent?
- Is it usable without a mouse?

**Responsiveness**

- Does it hold up in a narrow pane, a short pane, and on a high-DPI display?
- Does resizing a pane or window keep it usable?
- Do dialogs and modals stay within the viewport?

**Platform consistency**

- Is it fully usable on macOS, Linux and Windows?
- Does it respect system settings (fonts, scaling, input methods)?

**Performance**

- Every interaction gives instant feedback. Anything slow shows that it is working.
- Does it survive large files, big projects and heavy workloads?
- Frames must take no more than 8ms.

**Consistency**

- Does it match the spacing, typography and icon language already in use?
- Are terminology and tone consistent with the rest of the app?
- Do interactions behave like their neighbours — how tabs close, how modals dismiss, how
  errors surface?

**Text**

- Are strings concise, clear and unambiguous?
- Do they avoid jargon only a maintainer would recognise?

**Paths and edge cases**

- What does the happy path look like? The unhappy one — errors, rejections, invalid state?
- How does it behave offline, and signed out?
- What happens when data is missing, corrupted or slow to arrive?
- Are error messages actionable?

**Discoverability**

- Can a first-time user work it out without the docs?
- Is there an obvious undo?
- Are power features there without being in the way?

## Things that probably will not be merged

Few hard rules, but typically not:

- Anything an extension could provide — a language, a theme.
- New file icons. The icon theme is designed to hang together; off-the-shelf SVGs do not.
- Features whose complexity outweighs the number of people helped.
- Giant refactors.
- Non-trivial changes with no tests.
- Style-only changes that alter no behaviour. Removing allocations, removing `unwrap()`s
  and fixing typos are welcome; "more readable" on its own is a matter of taste.
- Anything that crosses one of the lines in "What this fork is" without discussing it
  first.

## A map of the code

The [glossary](./docs/src/development/glossary.md) explains the recurring types and terms.
Zode is many crates; these are the ones you are most likely to open.

Inherited from Zed, and largely unchanged:

- [`gpui`](./crates/gpui) — the GPU-accelerated UI framework everything is built on. Start
  with its root-level documentation.
- [`editor`](./crates/editor) — the `Editor` type behind both the code editor and every
  input field, plus the display layer for inlay hints and completions.
- [`project`](./crates/project) — files, the file tree, and this side of LSP.
- [`workspace`](./crates/workspace) — panes, docks, and local state serialization.
- [`language`](./crates/language) — the editor's understanding of a language, from symbols
  to the syntax map.
- [`lsp`](./crates/lsp) — talking to language servers.
- [`vim`](./crates/vim) — Vim workflow layered over `editor`.
- [`theme`](./crates/theme) — the theme system and the shipped themes.
- [`ui`](./crates/ui) — shared components and the patterns they follow.
- [`rpc`](./crates/rpc) — the wire messages. In this fork they serve remote development
  only; the collaboration server they were written for was removed.
- [`cli`](./crates/cli), [`zed`](./crates/zed) — the CLI, and the binary where it all
  comes together.

Specific to this fork:

- [`sidebar`](./crates/sidebar) — the project rail: every open project, its avatar and
  colour, its panels and its agents.
- [`agent_ui`](./crates/agent_ui), [`agent_sessions`](./crates/agent_sessions),
  [`agent_usage`](./crates/agent_usage) — agents as terminal tabs, their session history,
  and the status-bar quota read.
- [`database`](./crates/database), [`database_ui`](./crates/database_ui) — the database
  client. Zode ships no driver; each engine is a sidecar process speaking JSON-RPC over
  stdio, so a driver that crashes costs a process rather than the editor.
- [`container_ui`](./crates/container_ui) — Docker and Kubernetes in one list, with the
  backend answering which kinds and actions it offers rather than the UI branching on it.
- [`git_ui`](./crates/git_ui) — the git and worktree panels, branch pickers, blame and
  commit view.
- [`project_footprint`](./crates/project_footprint) — the per-project CPU and RAM badge.
- [`zode_account`](./crates/zode_account), [`zode_sync`](./crates/zode_sync) — the
  optional account and end-to-end encrypted settings sync. Read
  `script/check-account-no-telemetry` before changing either.

## Packaging

Upstream's [notes for packaging Zed](https://zed.dev/docs/development/linux#notes-for-packaging-zed)
still apply in the main, because the build system itself was not touched. Two constraints
are this fork's own and are not in that document:

- **Linux builds target glibc 2.35 or newer** (Ubuntu 22.04, Debian 12). Releases are
  built inside an `ubuntu:22.04` container so the floor holds whatever runner image GitHub
  offers, and `script/check-glibc-floor` fails the build if anything in the bundle asks
  for more.
- **Release installers do not ship `remote_server`.** Building it doubles the time and
  disk of every release, which does not fit free GitHub-hosted runners. It still builds
  from source with `cargo build --release --package remote_server`.
