# Dev builds asked for unpublished 0.1.1 and got a 404; drivers now come from latest, and the server keeps one set

**Date**: 2026-10-15 01:10
**Severity**: High — no dev build could download a database driver, and the shipped 0.1.4 cannot either
**Component**: `database` (`install/download.rs`, `install/store.rs`), `database_ui` (`driver_registry.rs`), web `backend/src/drivers/`, `script/publish-drivers-to-web`
**Status**: ongoing — zode 35effe8 and a084844 on `feat.release-v0.1.5`; web 1421aff live on `main`; no in-app download has been run

## What Happened

A dev build's dialog read `version 0.1.1 publishes no driver manifest (fetching it returned 404 Not Found)`. The app asked for drivers at its own version, 0.1.1, and nothing was published under that number. Production held only 0.1.4, and the shipped 0.1.4 sends `0.1.4+stable`, which gets a 400.

The decision: a publish deletes every other version, and the client downloads whatever `latest` serves, with no version check. Installed drivers stay until removed. Web 1421aff deployed to `main`. Review and tests then caught four defects in the change, each fixed below. Production now answers `latest` with 200, `no-cache`, version 0.1.4, 24 drivers.

## The Brutal Truth

This one stings a little. The fix was right, and it still broke what the old lookup had been protecting. The staging directory was safe only because the lookup matched one exact version. Widening the lookup made a half-unpacked directory look like an install. The reviewer found it reading the diff, and the tester's red test confirmed it; the implementation's own test had planted only `0.2.0.incoming`, which is not a version.

## Technical Details

- Before the change: `0.1.1` → 404, `0.1.4` → 200, `latest` → 400. The `:version` route took `latest` as a version and its DTO refused it.
- Defect 1 (red in the tester run): `store::is_version` accepted `0.1.5-rc.1.incoming`, because `VERSION_PATTERN` (`backend/src/drivers/drivers.paths.ts`) admits alphanumeric pre-release identifiers. Failure: `left: Some("…/postgres/0.1.5-rc.1.incoming/zode-db-postgres")`, `right: None`. Fix: dot-prefixed scratch names (`.<version>.incoming`, `.<version>.download`), and a failed unpack removes its staging.
- Defect 2: two publishes of different versions could each prune the other, and a test reproduced `[]`. Fix: a `publishTail` queue; a failed prune logs instead of returning a 500.
- Defect 3: `openAsset` ran `stat` then `createReadStream`. A prune between them answered 400 with the absolute store path, per the review record; not re-run for this entry. Fix: one file handle.
- Defect 4: the `@Header(... immutable)` decorator ran before the handler, so 404s carried it. Now set in the handler (`drivers.controller.ts:78`); the e2e test asserts a pruned 404 has no `immutable`.
- Gates, exit 0: database 101 tests, database_ui 63, web unit 334, web e2e 120. Clippy was a cache hit (0.72 s), so not a fresh lint.
- Not run: an in-app download or removal.

## What We Tried

1. **Set `DRIVER_RETENTION_VERSIONS=1` and leave the prune code alone.** Set aside: the survivor would have been picked by manifest mtime. The user chose a prune that keeps the version just published, by name, and the setting was removed.
2. **`<version>.incoming` as the staging name.** Held while lookup was an exact match; failed once lookup accepted any version (defect 1). Replaced by dot-prefixed scratch names.
3. **A decorator-level cache header and an unqueued publish.** Both were replaced: the header moved into the handler (defect 4), and publishes now run through `publishTail` (defect 2).

## Root Cause Analysis

The key was wrong. Drivers were chosen by the app's version string, so the server had to hold a set published under exactly that number. A dev build's 0.1.1 never was.

Defect 1 is the subtler one. The staging name was safe only because the check was narrow. The plan did ask what the wider lookup would match: it added the version-shape check to keep `.incoming` out. What it missed is that a pre-release version with `.incoming` appended is still a valid version.

## Lessons Learned

- Before widening a lookup, list everything the wider rule now matches.
- Set cache headers where the outcome is known. Keeping one set means every publish 404s the previous set's URLs, and a 404 carrying a one-year immutable header is cacheable for a year. When checked, Cloudflare answered `cf-cache-status: DYNAMIC`, so it was not caching that 404; any other cache on the way could have.
- A green exit is not the bar. `backend/vitest.config.ts` uses array-form thresholds that Vitest 4 ignores, so `npm run test:cov` exits 0 with `src/auth` at 82.92 against 85.

## Next Steps

- **User**: run the path by hand before tagging v0.1.5: `rm -f target/debug/zode-db-*`, `make build && make dev`, download SQLite, relaunch, remove it.
- **User**: merge PR #43, then tag v0.1.5. The backend is already live, so the client can ship.
- **Unassigned, deferred**: retry once on a brief 404 during a prune; route `deleteVersion` through the publish queue; sweep `.replaced-*` orphans; split `download.rs` (1,238 lines) and `store.rs` (591) under the 200-line guideline.
