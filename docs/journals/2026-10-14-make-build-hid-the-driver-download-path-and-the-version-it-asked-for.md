# make build hid the driver download path, and the version it asked for was refused

**Date**: 2026-10-14 22:07
**Severity**: High — no build could download a database driver, release 0.1.4 included
**Component**: `database_ui` (`driver_registry.rs`), `Makefile`, `script/bundle-mac`, `docs/src/development.md`
**Status**: ongoing — both fixes pushed to PR #43 on `feat.release-v0.1.5`; no live in-app download has been run

## What Happened

Testing the remove-a-downloaded-driver button from 4b9a723, I could not make it appear. It needs a driver whose origin is the download store (`DriverOrigin::Store`). `make build` also compiled the four `zode-db-*` sidecars beside `target/debug/zode`, and `resolve_driver` checks beside the executable first, then the store, then `ZODE_DB_<ID>`. The local copy always won, so no download was attempted.

Asking where a dev build downloads from exposed a worse defect. `driver_version()` formatted the whole app version, build metadata included: a dev build asked for `0.1.1+dev`, a release for `0.1.4+stable`. Production refuses that:

```
GET /api/drivers/0.1.4+stable/zode-db-drivers-manifest.json  → 400
GET /api/drivers/0.1.4/zode-db-drivers-manifest.json         → 200
```

No build could download a driver, release 0.1.4 included. Nobody had noticed: Zode.log on this machine shows the installed 0.1.4 app finding no driver beside its executable and offering the download, but no download was ever attempted, so the 400 never surfaced.

## The Brutal Truth

This one stings. The download path had green tests, and the first real attempt to use it from dev hit a wall. The version bug sat in the production path the whole time, and no test could see it.

The tests never built the string the app builds. `connection_modal.rs` uses `const TEST_VERSION: &str = "0.0.0"` and injects it straight into the installer. The docs said a dev build is `0.0.0` too. It is `crates/zed/Cargo.toml`'s `0.1.1`, and a plain dev build asks production for that and gets a 404.

The galling part: "how will I see this work in dev?" found a production bug the feature work never touched.

## Technical Details

- `AppVersion::load` (`crates/release_channel/src/lib.rs`) puts the channel name in `version.build`, plus a build id and sha when set. The fix clears it before formatting and keeps pre-release tags such as `0.1.5-rc.1`.
- Test `the_driver_version_leaves_out_build_metadata`, red against the old code: `left: "0.1.4+stable.abc1234"`, `right: "0.1.4"`. Green after the fix.
- `make build` compiles the `zode` binary only. `make drivers` stays opt-in.
- `script/bundle-mac -n` skips building and packaging drivers. `make bundle` passes it. The three release workflows call `./script/bundle-mac <triple>` without it, so release is unchanged.
- The driver list is a string, not an array: under `set -u`, macOS's bash 3.2 treats an empty array as unbound. Checked under `/bin/bash` 3.2.57, both ways.
- Checked: `database_ui` 63 tests, `database --lib` 73, scoped clippy clean, and a real `make build` leaves no `zode-db-*` beside the binary.
- Not run: full-workspace tests and clippy, a live in-app download.

## What We Tried

1. **Keep drivers out of `make build`.** This fixed the default, so a clean dev build reaches the download path. It did not fix a stale checkout: drivers already beside a binary keep winning until deleted.
2. **Automate removal of stale sidecars.** Set aside. The user chose to document it (`rm target/debug/zode-db-*`) in the `Makefile` and `docs/src/development.md`.
3. **Ask where a dev build downloads from.** This found the 400. Stripping build metadata in `driver_version()` fixed it.

## Root Cause Analysis

Two causes, stacked.

The `drivers` step was added so a fresh checkout would not fail with `command not found: zode-db-postgres`. That was right for its problem, but it put a local copy ahead of every download on every `make build`, so the download path was unreachable from development.

The version bug is a formatting choice. Semver's `Display` prints build metadata, and `AppVersion` puts the channel there. The tests injected their own string, so nothing exercised the real producer. Both are the same mistake: testing a stand-in instead of what the app actually does.

## Lessons Learned

- A contract test that builds its own input proves the consumer, not the contract. My new test repeats the trap in smaller form: it hands `init_test` a hand-written `0.1.4+stable.abc1234`, so it proves the stripping, not that `AppVersion::load` produces that shape.
- A precedence that silently shadows another path hides that path. "Local beats downloaded" is right for shipped bundles and a trap in dev. A build step that produces the shadowing copy should be opt-in.
- Ask "how do I see this work in dev?" before calling a feature done.

## Next Steps

- **Tobi**: before PR #43 merges, run the path by hand: `ZED_APP_VERSION=0.1.4 make dev`, download a driver, remove it.
- **Tobi, or whoever next touches `driver_registry.rs`**: have the version test build its input through `AppVersion::load`.
- **Anyone on a checkout older than this fix**: `rm target/debug/zode-db-*` before testing a download.
- **Undecided**: whether release 0.1.4 gets a patch. By the code and the 400 above, released binaries cannot download any driver; that has not yet been seen in the app itself. Nothing in this session settled it.
