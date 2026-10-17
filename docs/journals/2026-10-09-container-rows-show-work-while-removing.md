# Container rows show work while they are removed

**Date**: 2026-10-09
**Severity**: Low — a removal looked like it had done nothing until the list reloaded
**Component**: `container_ui`
**Status**: Resolved on `feat.release-v0.1.5`; not yet checked by hand against a real engine

## What Happened

Removing a container, or pruning, showed no sign of work. `ContainerPanel::act` recorded a running Start or Stop in `in_flight`, and the row said "Start...". `ContainerPanel::destroy`, which runs a confirmed removal, recorded nothing, so the row sat unchanged with all its buttons while `docker rm` ran.

## Technical Details

- **One busy state for every operation:** `Busy::{Action(ResourceAction), Removing}`. `in_flight` holds an `InFlight { busy, token, settled, displaced }` per row id. `destroy` marks every target of the plan as `Removing`, which covers both a single removal and a prune.
- **No button flash:** on success the entry is marked settled, not removed, and the next completed reload clears it. The row stays busy from the moment the command returns until the list no longer has it. A list read that started earlier cannot clear it first, because `reload` replaces any read in flight in the same update. On failure the entry is cleared at once and the error shows as before.
- **Stale outcomes:** each operation's token is its task key, so an outcome from before a switch of engine, kind or config target, or from an older operation on the same row, cannot settle a newer one. Switching clears the map.
- **Prune over a running action:** review found that a prune touching a row mid-Stop would overwrite the Stop, and a refused prune would then show the row's buttons while the Stop was still running. The overwritten entry is now kept as `displaced`. A refused removal restores it, and an outcome for the displaced operation updates it in place, so a restore never revives an action that already finished.
- **Rendering:** one helper draws a spinner keyed by resource id plus a label ("Starting…", "Stopping…", "Restarting…", "Pausing…", "Resuming…", "Removing…"). It is used by the list row and the detail header. The row's buttons are hidden while it is busy. The old "Start..." text is gone.

## Gates

`./script/clippy` exit 0. `cargo test -p container_ui -p container`: 69 and 59 passed. The 11 ignored tests need a real engine and were ignored before this change. The busy tests were run repeatedly with no failure. `zode` compiles. `rustfmt --check` exit 0.

## Still Unverified

Not run against a real Docker or Kubernetes engine.
