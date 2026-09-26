# Four requests arrived at the bench, and three were really saying something else

**Date**: 2026-09-25
**Severity**: High — a crash-free but unusable floating window, a refresh button with no feedback, and kubectl commands that would have talked to the wrong cluster
**Component**: `container`, `container_ui`, `database_ui`, `floating_pane`
**Status**: Resolved (commits `9c149f2`, `f35d91a`, `ca02b95`)

## What Happened

Four requests, one sentence each: let kubectl pick a config file to list pods; give reload an
animation; the "open in floating window" button has no background and cannot be dragged; add
database, docker and kubectl options to the floating window's empty screen.

Reading the code first changed three of the four.

## The Brutal Truth

**"Reload needs an animation" was really "reload has no feedback at all."** `ContainerPanel::reload`
never wrote any loading state. Pressing Refresh on a ready list changed nothing on screen — no
spinner, no text, not even the row count in the header — until the new list swapped in. There was
no animation to add because there was no state to animate.

**Two different things in this repo are called "floating window", and the two complaints belonged
to different ones.** `crates/floating_pane/` is an in-app overlay; it already painted
`elevated_surface_background` and it was the one that could not be dragged. The
`container::OpenInFloatingWindow` and `database::OpenInFloatingWindow` actions open real OS windows
through `cx.open_window`; those could be dragged by their native title bars and were the ones
showing black. One sentence, two defects, two crates.

**"Docker" and "kubectl" are the same panel.** `ContainerPanel` holds Docker, Podman and Kubernetes
behind one backend switcher. There is no kubectl UI and no `kubernetes_ui` crate. Three separate
rows were still right — `ContainerPanel::standalone` already takes an active-backend index, so
pre-selecting an engine cost nothing — but a plan that treated them as three panels would have been
building something that does not exist.

**The scope machinery was already there and already dead.** `KubernetesBackend::with_scope` had
exactly one caller in the workspace, and it was in `crates/container/src/tests.rs`. Production always
built `KubernetesBackend::new()` with `scope: None`, and `watch()` returns `None` without a scope —
so Kubernetes never had a watch stream at all, only the 30-second tick. Meanwhile the error screen
printed "Choose a context, then refresh" beside a lone Refresh button. The code told users to do
something the UI had never offered.

## Technical Details

### The grab anchor carried the floating layer's position in the window

`grab()` computed `event.position - bounds.origin`. `event.position` on a `MouseDownEvent` is
window-relative. `bounds` was `placement`, whose origin is relative to the floating layer — it is
applied as `.left(origin.x).top(origin.y)` on an `absolute()` child of that layer. The consumer then
worked in layer space. So the stored anchor was wrong by exactly the layer's own origin in the
window: the height of the app title bar plus the width of any left dock. A title-bar drag jumped up
and left by that amount, and `clamp_into` pinned it to the top edge, where it stuck.

The fix inverts the anchor — store `bounds.origin - event.position`, then move to
`anchor + pointer_in_window`. Both operands are read in window space, so the layer origin drops out
of the Move path entirely. `Grab::Resize` was never affected; it uses a zero offset.

The existing tests could not see this. The helper that paints a pane mounts it as the **root view of
its own test window**, where the layer origin is `(0,0)` and the two coordinate spaces coincide. The
regression test feeds a non-zero, per-axis-distinct layer origin and was confirmed red before the fix
and green after.

### `logs_command` and `exec_command` build their own argv

Adding `--kubeconfig` to `KubernetesBackend::command()` was not enough. Those two build their own
`Vec<String>` and never call `command()`, so after a user picked a specific kubeconfig they would
have kept talking to whatever cluster the environment named — silently, and only for those two
commands. A `global_flags()` helper now prepends `--kubeconfig`, `--context` and `--namespace`
**before** the subcommand words, because both commands end with a `--` separator and kubectl reads
everything after `--` as positional.

These are the same two buttons — "Follow logs" and "Open shell" — whose double-lease crash was fixed
the day before. They had just stopped killing the app; they would have gone on to read the wrong
cluster.

`kubeconfig::command()` needed the flag too, separately: it bypasses `KubernetesBackend::command()`
entirely, so without it the context list would have been enumerated from the wrong file.

### A cfg gate that only runs one way

`RemoteConnectionOptions::Mock` is gated on **`remote`**'s `test-support`. The match arm for it in
`remote_connection` is gated on **`remote_connection`**'s own. `test-support = ["remote/test-support"]`
implies one direction and not the other, so a graph with the first feature on and the second off
leaves the variant present and the arm absent — a non-exhaustive match, in committed code that
nothing had previously built that way. Adding `git_ui` as a plain dependency of `floating_pane` was
the first thing to produce that combination. Fixed the way `git_ui` and `project_panel` already
handle it: a dev-dependency requesting `remote_connection/test-support`.

### The standalone windows painted nothing

`WindowOptions` in both `standalone.rs` files omitted `window_background`, so it defaulted to
`Opaque` — an opaque black NSWindow background on macOS. The root views paint no surface of their
own, and a comment in `database_ui/src/render.rs` says why: `Dock::render` draws the card. In a dock
or a pane something else paints; in a window of its own, nothing does. The fix sets
`window_background` from the theme and wraps the panel in a private `StandaloneWindow` view that
paints `panel_background`, leaving the shared `Render` impl untouched so the dock and editor-tab
hosts do not gain a second card.

Note that under the default theme `window_background_appearance()` genuinely returns `Opaque` —
the wrapper is the half that removes the black. No test can assert it: GPUI cannot read back a drawn
colour.

## Lessons Learned

**A request names a symptom, not a cause.** "Add an animation" meant "there is no state to animate".
"It has no background" pointed at a different crate from "it cannot be dragged". Reading the code
before planning changed three of four requests, and one of those changes — Docker and Kubernetes
being one panel — would otherwise have produced a plan for a thing that does not exist.

**A test harness that simplifies the composition cannot see bugs in the composition.** The drag maths
was wrong for as long as the pane has rendered inside a layer, and every test passed, because the
helper put the pane where the two coordinate spaces were the same. When a test constructs a simpler world
than production, say in the test what it is not covering.

**Written prohibitions are not boundaries.** Despite a bolded "NEVER run `cargo fmt --all`" in its
prompt, an agent ran a broad format and rewrapped five files across two crates it did not own.
Harmless here — `git diff -w` confirmed formatting only — but it proves the instruction is advice,
not a gate. Run `git status` against the assigned file list after every wave of parallel agents.

**"Code is done, only the build is blocked" is not a status.** An agent reported DONE_WITH_CONCERNS
having never run a test, because the cfg bug above blocked compilation. Its new menu rows had broken
an existing test — `the_plus_menu_offers_a_split_submenu` sweeps a fixed pixel distance looking for
the Split submenu, and seven new rows pushed the submenu past it. That surfaced only when the
orchestrator unblocked the build and ran the suite. An agent that never went green has not finished;
it has stopped.

**A gate piped through `tail` reports `tail`'s exit code.** This is the second session in a row it
has bitten: a clippy run that died with `error: could not compile rustix` came back as exit 0.

**Past about a thousand changed lines, one reviewer stalls.** A single review commission over this
diff hit the watchdog at 600 seconds and returned nothing. Split across two reviewers by crate group,
both finished in about three minutes and found a Medium the stalled one never reached.

## What review caught

`config_error` was captured when a chosen kubeconfig could not be read, and never rendered — the same
silent-failure shape the session had just spent a phase curing on the refresh button. It now has its
own dismissible banner, sharing a helper with the existing one so the two cannot drift; a separate
field rather than reusing `last_error`, because switching engines clears one and not the other. Also
caught: two shipped comments saying "this phase", which resolves to nothing once `plans/` is
gitignored.

## Shipped with a known limit

Git, Project Files and Debugger open in the floating pane as **second instances**.
`workspace.panel::<T>(cx)` returns the dock-registered entity at 62 call sites, so `ToggleFocus`, the
editor's git buffer-header controls, the debug process launcher and the attach picker all keep
addressing the dock copy. Planning recommended dropping this tier; the cost was put to the user and
they chose to build it. The limits are written on the `PanelItem` wrapper, where a surprised reader
will find them. This was an informed choice, not an oversight.

An earlier verdict also claimed `GitPanel::render`'s fixed `.id("git_panel")` would collide between
two instances. It does not: `GlobalElementId` is a path built from the live element-id stack, so the
same local id resolves differently under a dock and under a pane.

## Gates

`cargo test -p container` 59, `-p container_ui` 56, `-p database_ui` 61, `-p floating_pane` 49 — all
exit 0, re-run after the commits landed. `cargo fmt --all -- --check` exit 0. `./script/clippy` over
the four crates exit 0.

CI had been red on `develop` before this session began, and the previous session caused it:
`cargo fmt --all -- --check` is its own CI step and `./script/clippy` never checks formatting, so
five unformatted files from the agent-notification work reached `develop` behind a green clippy run.
Committed separately as `9c149f2`.

## Still unverified by machine

The painted surface of the standalone windows, whether the dock and editor-tab hosts now double-paint,
dragging the floating pane by hand, and the kubeconfig picker against a real cluster — this machine
has no `~/.kube/`.
