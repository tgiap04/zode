# Post-deletion metrics — Phase 11

Compared against `research/baseline-metrics.md` (Phase 1, base commit `7ac6829`).

## Crate counts

| Method | Baseline | Expected (Phase 1 estimate) | Actual (Phase 11) |
|---|---|---|---|
| `crates/` directories | 232 | 179 | 178 |
| workspace members in Cargo.toml | 239 | 186 | 187 |
| cargo metadata packages (`--no-deps`) | 240 | 187 | 186 |

The small (±1) variance from the Phase 1 estimate is expected — those were educated guesses
made before execution, per the baseline doc's own framing.

## Whole-plan diff (`7ac6829` → HEAD, including this phase's uncommitted work)

```
908 files changed, 57100 insertions(+), 395832 deletions(-)
```

Net: **-338,732 lines**.

## Build

- `cargo check --workspace`: green.
- `cargo check --workspace --all-features`: green.
- `cargo build --release`: green, **15m12s** wall time, single machine (Apple Silicon, macOS 25.6).
- Release binary (`target/release/zed`): **216 MB**, measured both before and after this phase's
  cuts — unchanged at this rounding. `libwebrtc`/`webrtc-sys` were statically-linked native code
  reachable only through the now-deleted `EchoCanceller`; removing that dependency mainly shrinks
  *link/compile time*, not final binary size, against a 216 MB binary dominated by tree-sitter
  grammars, the WASM extension runtime, and bundled language servers.
- `./script/clippy` (`cargo clippy --release --all-targets --all-features -- --deny warnings`,
  plus `cargo machete`): **green** — `cargo machete` reports zero unused dependencies workspace-wide.
  `typos`/`buf` skipped — neither tool is installed in this environment; `script/clippy` degrades
  gracefully per its own logic (`which typos || exit 0`).

## Tests

- `cargo nextest run --workspace --no-fail-fast`: **3993 tests run, 3992 passed, 1 failed, 11
  skipped** (Phase 10 baseline: 4001 run, 4000 passed, 1 failed, 11 skipped — 193 binaries in
  both runs). The one failure is the **same, unchanged, pre-existing regression** —
  `editor::inlays::inlay_hints::tests::test_refresh_requested_multi_server`, bisected in Phase 10
  to the Phase 4 structural cut, still open, still not fixed (the fix lives in `editor`/`language`
  server-resolution territory, deliberately kept at zero change across this whole fork — see
  `phase-10-tests-and-docs.md`).
- **The 8-test delta is fully accounted for**, verified by diffing this run's test names against
  `research/post-deletion-tests.log` (Phase 10's actual run log, not the separate all-features
  test-inventory list used for Phase 10's static diffing): all 8 are
  `client::proxy::{http_proxy,socks_proxy}::tests::*` — the parsing unit tests belonging to the
  `proxy.rs` module deleted in this phase alongside the dead connection-establishment chain. Two
  entries appeared to be "new" in a naive diff (`audio_pipeline::rodio_ext::tests::instant_replay::
  {continues_after_history,samples_ready}`); they were present in both runs, just flagged `LEAK`
  instead of `PASS` in the Phase 10 log (nextest's leaky-test detector is non-deterministic) —
  confirmed by grepping the raw Phase 10 log directly, not a real difference.
- No new failures introduced by this phase's cuts.

## What this phase found and fixed (beyond the plan's explicit checklist)

`--deny warnings` under `--all-targets --all-features` surfaces dead code invisible to a plain
`cargo check`. Every item below was verified dead (zero callers, checked with `rg` across the
whole workspace) before deletion — nothing was suppressed with `#[allow]`.

1. **`crates/audio`'s `libwebrtc`/`webrtc-sys` dependency** — used only by an `EchoCanceller`
   whose owning `Audio` struct has zero external readers/writers anywhere in the workspace (the
   entire mic-capture/AEC surface was orphaned once collab voice calls were removed in earlier
   phases). Removed the `echo_canceller` module, the field, and the dependency. This also made the
   `[patch.crates-io]` entries for `livekit`/`libwebrtc`/`webrtc-sys` (`Cargo.toml`) genuinely
   dead — pruned those too, along with the now-unused `livekit`/`libwebrtc`/`webrtc-sys` entries
   in `[workspace.dependencies]`.
2. **`client`'s dead connection-establishment chain** — `connect_with_credentials`,
   `set_connection`, `establish_connection`, `rpc_url`, `establish_websocket_connection` (client.rs)
   plus the entire `proxy.rs`/`proxy/http_proxy.rs`/`proxy/socks_proxy.rs` module they were the only
   caller of, plus `ZED_RPC_URL`. This is exactly the ~500 lines Phase 5 deliberately left ("kept
   on a false pretext") — rustc's own dead-code lint confirmed all of it unreachable in a release
   build (`cargo build --release`'s warning log, 26 warnings in the `client` crate, all resolved).
   **Left alone, deliberately**: the `authenticate`/`establish_connection`/`rpc_url` fields on
   `Client` and their `override_*` setters plus `Client::teardown` — these are genuinely `pub` API
   surface (defined at the crate root, so rustc correctly treats them as always-reachable) with
   zero real callers, which is precisely the "may exist as a type but must have no caller" bar this
   phase's own Security Considerations section sets for exactly this situation.
3. **`crates/client/src/user.rs`'s `CURRENT_ORGANIZATION_ID_KEY`** constant — unused.
4. **Unused workspace dependencies** (cargo-machete, each individually verified with `rg` before
   removal): `client` Cargo.toml (`chrono`, `cloud_api_types`, `credentials_provider`, `db`,
   `derive_more`, `feature_flags`, `fs`, `serde_json`, `serde_urlencoded`, `tiny_http`,
   `zed_credentials_provider`); `title_bar` (`anyhow`, `icons`, `notifications`,
   `remote_connection`, `rpc`, `semver`); `settings_ui` (`cpal`, `regex`, `rodio`,
   `shell_command_parser`, `zed_credentials_provider`); `file_finder` (`client`, `fuzzy`);
   `onboarding` (`collections`); `audio` (`collections`, `parking_lot`, `util` — orphaned by the
   `echo_canceller` cut above).
5. **`settings_ui`'s dead builder methods** — `SettingsInputField::{with_id, display_confirm_button,
   display_clear_button, clear_on_confirm, action_slot, with_buffer_font, color}` and
   `SettingsSectionHeader::{icon, no_padding}`. These predate this fork (ordinary unused
   builder-pattern methods in a component library) but fail `--deny warnings` regardless of origin,
   so they were removed per this phase's mandate to fix rather than suppress.
6. **Three pre-existing clippy correctness/style lints**, unrelated to the fork's own deletions,
   caught only because this is the first time `--release --all-targets --all-features -- --deny
   warnings` has actually completed against this branch: `clippy::int_plus_one` in
   `remote_connection/src/remote_server_release.rs`, and `clippy::redundant_clone` in
   `git_ui/src/conflict_view.rs` and `zed/src/zed.rs` / `zed/src/main.rs`, plus one genuinely
   unused `use prompt_store::PromptBuilder;` in `zed.rs`'s test module.

## What this phase deliberately did *not* touch

- **`ProjectClientState::Collab`** (`crates/project/src/project.rs`) — flagged "never constructed"
  by a plain `cargo build --release` (no `--all-targets`), but constructed correctly once
  `--all-targets` is added, via the intentionally-kept `Project::mark_as_collab_for_testing`
  test-support helper (Phase 8's deliberate choice). Confirmed with
  `cargo check -p project --all-targets --all-features`: zero warnings. Not a real Gate A finding;
  a false alarm from testing the wrong configuration.
- **`.github/workflows/background_agent_mvp.yml`** and `script/run-background-agent-mvp-local` — a
  dormant (manual-dispatch-only since 2026-02-24) Sentry-crash-autofix CI pipeline. It references
  `script/sentry-fetch`, which does not exist in this repository at any point in its history
  (confirmed back to the initial commit) — so it predates and is independent of this fork's
  surgery. Since this fork sends no crash reports anywhere, the workflow is now fully orphaned, but
  touching CI configuration is a confirm-first action and out of this phase's scope. Flagged for
  the maintainer's decision (Phase 12 or a dedicated follow-up).
- **`context_server/src/oauth.rs`'s `CIMD_URL`** (`https://zed.dev/oauth/client-metadata.json`) —
  not an egress path from this app; it's the client-identifier this fork presents to *third-party*
  MCP OAuth servers (which may themselves dereference that URL). Functionally harmless, but it
  means this fork currently identifies itself as "zed.dev" to external auth servers — a branding/
  disclosure question for Phase 12, not a privacy leak. See `network-verification.md`.
