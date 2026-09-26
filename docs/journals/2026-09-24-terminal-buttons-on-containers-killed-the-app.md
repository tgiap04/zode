# Clicking terminal buttons on containers killed the app — a nested entity lease under the wrong roof

**Date**: 2026-09-24 02:15
**Severity**: High — user-facing crash on a common interaction; the fix is a reordering, not new logic
**Component**: `container_ui`, `terminal_view`
**Status**: Resolved (commit `448cd29`)

## What Happened

Clicking either the "Open a shell in a terminal" or "Follow logs in a terminal" button on a Docker container row crashed the app instantly. Both buttons are drawn by one function, `render::render_trailing`, which the row list and the container detail header both call, so every instance of them died the same way. What did survive was the log terminal embedded in the detail view: it is built from `project.create_terminal_task` directly and never goes near `spawn_task`. The crash was deterministic and needed no setup.

## The Brutal Truth

The galling part is the comment left in the old code — it asserted the nested update was safe because the entities were different. True and irrelevant. Entity identity was the wrong question. The real trap: whether the callee could reach back into the entity you were still holding open. Here, it did.

The test suite had two tests over this path. Neither one stood a real terminal panel up and clicked. One short-circuited before the panel was even reached. The other checked that the handle was there without ever driving it. Neither proved anything about the actual button.

## Technical Details

`ContainerPanel::open_terminal` (`crates/container_ui/src/terminal.rs:53`) held an open `workspace.update` and called `TerminalPanel::spawn_task` from inside it. That method's first statement was `let project = workspace.read(cx).project().read(cx);` — it reached back into the same workspace entity to get the project. GPUI had leased that entity out of the entity map for the enclosing `update`, so `EntityMap::read` at `entity_map.rs:162` found `None` and fell through to `double_lease_panic` at line 207:

```
panic!("cannot read workspace::Workspace while it is already being updated")
```

The panic message came straight from `entity_map.rs:208–209`. Main thread, process died.

Both buttons went through the identical call path, so both crashed the same way, in the row list and in the detail header alike — `detail.rs` renders them through the same `render_trailing`.

## What We Tried

1. Fetched the `Entity<TerminalPanel>` using `workspace.read_with(cx, ...)` to release the lease before driving the panel.
2. Updated the panel directly with `terminal_panel.update(cx, ...)` instead of routing through the workspace.
3. Added a `/// # Panics` doc comment to `spawn_task` to record the constraint for the next caller.
4. Wrote a regression test that stands a real `TerminalPanel` up in the workspace via `TerminalPanel::new` + `workspace.add_panel`, then clicks both intents. It asserts nothing about the terminal itself: the fake engine's program is `fake`, which is not a binary, so the spawn fails for its own honest reason. Reaching the end of the call is the whole proof, because the defect was a panic.

The test was run **before** the fix and reproduced the exact panic, then passed after. Running a test only after a fix proves nothing.

## Root Cause Analysis

The comment claiming safety relied on entity identity — "different entity, so it is safe." Reachability was the right criterion. An entity is safe to hold open only if the code you call does not reach back into it. `spawn_task` did.

The trap: a nested `update` feels local (you are updating a different entity), but if the callee's very first line reads the one you leased, it panics before it can do any damage. The read happens on the main thread, and the panic stops the app.

## Lessons Learned

A nested call is safe only if you inspect what the callee does, not just what it is. "Different entity" is not a proof. "Does not read the one I hold" is the gate.

The test gap: a test that only checks a handle is present without clicking or driving it is a false green. Real interaction is required to prove the crash does not happen.

Two toolchain traps cost time in this session and will recur:

`export SDKROOT="$(xcrun --show-sdk-path)"` — without `--sdk macosx` — returns the Command Line
Tools SDK, whose `.tbd` files declare an `arm64e.x1` triple that Xcode's `ld` cannot parse. Exporting
it produced `tapi error: malformed file` on IOKit, CoreFoundation and Metal while linking
`cargo test`, and killed `rustix`'s build script under clippy. Only `xcrun --sdk macosx
--show-sdk-path` reaches Xcode's SDK. The `Makefile` already pins this, so `make run` is unaffected;
anything invoked outside `make` is not. Crate-scoped commands link fine with `SDKROOT` unset.

`./script/clippy ... 2>&1 | tail -30` reports **tail's** exit status, not clippy's. A run that died
with `error: could not compile rustix (build script)` came back as exit 0 and was reported as a
passing gate before its output had been read — the wrong turn of this session, and the reason the
gate was run a second time. Redirect to a file and read `$?` separately:
`cmd > log 2>&1; echo "exit=$?"; tail log`. A gate is never green on a piped exit code.

## Next Steps

None — the fix landed in commit `448cd29`. The `container_ui` crate still has no presence anywhere
under `docs/`, including `docs/generated/feature-list.md`. That gap predates this fix and was left
alone rather than filled with a feature doc invented around a bug.
