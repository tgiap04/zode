---
phase: 11
title: "Green gates and privacy verification"
status: pending
effort: "2d"
---

# Phase 11: Green gates and privacy verification

## Context Links

- [Workspace surgery research](./reports/research-workspace-surgery.md) §4 dead-reference tooling, §5 network verification
- [`research/baseline-metrics.md`](./research/baseline-metrics.md) — from Phase 1

## Overview

**Priority:** P1 · **Build state:** 🔴 RED → ✅ **GREEN** · **Depends on:** Phase 10

The build returns to green and the privacy claim gets proven rather than asserted. Two independent gates:

1. **Compile/lint/test gates** — the mechanical proof that the surgery is complete.
2. **Runtime network verification** — the proof that the actual product goal was met. A green build says nothing about whether the binary still phones home.

No single tool can prove a negative about network traffic, so §11b is a layered recipe.

## Key Insights

- `script/clippy` runs `--release --all-targets --all-features -- --deny warnings` plus `cargo machete` and `typos`. It is the **final gate**, not an iteration tool — running it earlier wastes time on code about to be deleted.
- `--all-features` matters: a feature flag can still name a deleted crate long after the default build is green.
- `cargo machete` was baselined in Phase 1 specifically so findings here are attributable to this work.
- `[patch.crates-io]` entries for `livekit`, `libwebrtc`, `webrtc-sys` (`Cargo.toml:872-880`) are now dead. Cargo ignores unused patches, so this is cleanup, not a fix.
- Layered network verification: static grep → `lsof` snapshot → `nettop` continuous log → hosts-file blackhole (proves *graceful degradation*, which is the falsifiable part) → Little Snitch/LuLu once, pre-release, to catch connection *attempts* that a blackhole would hide.
- **The blackhole test must carve out the extension registry** (red team finding 2). An earlier draft blackholed `zed.dev api.zed.dev collab.zed.dev` wholesale while also requiring "extension install still works" two lines above — a self-contradiction that would have sent the tester chasing a phantom regression. `api.zed.dev` is **allowed, disclosed egress** until Phase 12 decides on an independent registry.
- Hang-trace writing (kept in Phase 6d) must be confirmed still working — it is the only diagnostic left.

## Requirements

**Functional**
- `cargo check --workspace`, `cargo build --release`, `script/clippy`, and the test suite all green.
- Zero outbound connections to **telemetry, crash, LLM, or collab** endpoints during a full manual QA pass.
- Extension install and LSP auto-download still work. **These are explicitly allowed egress** — see the carve-out below.
- SSH remote development still works.

**Non-functional**
- Verification steps reproducible and recorded, suitable for a release checklist.

## Architecture

```
GATE A (mechanical)                    GATE B (behavioural)
────────────────────                   ─────────────────────
cargo check --workspace                static grep for endpoints
cargo build --release                  lsof -i snapshot
cargo nextest run --workspace          nettop continuous log
script/clippy (--all-features)         /etc/hosts blackhole → graceful degradation
cargo machete                          Little Snitch / LuLu (once, pre-release)
cargo tree -i <deleted> → not found    manual QA: extensions, LSP, SSH remote, hang traces
ripgrep sweep for stale refs
```

## Related Code Files

**To modify (cleanup only)**
- `Cargo.toml` `:872-880` — prune dead `[patch.crates-io]` entries
- Any survivor `Cargo.toml` flagged by `cargo machete`

**To create**
- `research/post-deletion-metrics.md` — the before/after comparison
- `research/network-verification.md` — the reproducible privacy recipe and its results

## Implementation Steps

### 11a. Gate A — mechanical

1. ```sh
   cargo check --workspace
   cargo check --workspace --all-features    # a feature may still name a deleted crate
   ```
2. ```sh
   cargo build --release
   ```
   Record binary size and compare against `baseline-metrics.md`.
3. ```sh
   cargo nextest run --workspace --no-fail-fast
   ```
   Diff against `research/post-deletion-tests.txt` from Phase 10. Any new failure is a regression introduced by cleanup.
4. ```sh
   ./script/clippy
   ```
   This includes `cargo machete` and `typos`. Fix every finding — `--deny warnings` means unread struct fields and unused imports left behind in Phases 7–8 surface here.
5. Prove the deleted crates are unreachable:
   ```sh
   for c in agent collab call channel copilot auto_update crashes language_models edit_prediction sidebar; do
     cargo tree -i "$c" -e normal 2>&1 | head -2
   done   # each must report "not found"
   ```
6. Ripgrep sweep for stale references the tools miss:
   ```sh
   rg -n 'crates/(agent|collab|call|channel|copilot|auto_update|crashes|sidebar)' --type toml --type yaml .
   rg -n 'zed_visual_test_runner|sentry-fetch' .github/ script/
   ```
7. Prune the dead `[patch.crates-io]` entries (`livekit`, `libwebrtc`, `webrtc-sys`) and re-run step 1.
8. Write `post-deletion-metrics.md`: crate directories 232→179 and packages 240→187 (both, per Phase 1's precise counting), check time, build time, binary size, LOC delta, test counts — all against the Phase 1 baseline.

### 11b. Gate B — behavioural privacy verification

9. **Static sweep** for surviving endpoints:
   ```sh
   rg -n 'zed\.dev|api\.zed\.dev|collab\.zed|livekit|sentry|MINIDUMP' crates/ --type rust
   ```
   **Expected survivors — exactly two, both known:**
   - `crates/client/src/zed_urls.rs` — rewritten or deleted in Phase 12
   - `crates/http_client/src/http_client.rs:214-224` `build_zed_api_url`, reached from `assets/settings/default.json:2472` `server_url` and consumed by `extension_host.rs:693`, `:792`, `:837`

   Anything else is a finding. Note honestly in `network-verification.md` that the extension registry is a **retained, disclosed dependency on Zed Industries' API**, pending Phase 12's decision.
10. **Launch and snapshot.** With the release binary running, exercise every surface: open a project, edit, save, use LSP completions, open the terminal, run git operations, install an extension, open settings.
    ```sh
    lsof -i -P | grep -i zed
    ```
11. **Continuous log** across a full QA pass:
    ```sh
    sudo nettop -p <pid> -J bytes_in,bytes_out -x -L 0 | tee research/nettop-session.log
    ```
12. **Blackhole test** — the falsifiable one. Two tiers, because the two categories have different verdicts:

    **Tier 1 — must never be contacted.** Blackhole telemetry/crash/LLM/collab endpoints:
    ```
    127.0.0.1 collab.zed.dev telemetry.zed.dev api.anthropic.com api.openai.com
    ```
    Any traffic here is a **failure**.

    **Tier 2 — allowed but must degrade gracefully.** Blackhole `api.zed.dev` (extension registry) **in a separate run**:
    ```
    127.0.0.1 api.zed.dev
    ```
    Extensions will fail to list/install — that is expected. What must **not** happen: crash, hang, or retry storm. Everything else in the editor keeps working.

    Do not combine the tiers into one hosts file. Tier 2's expected failure would mask a Tier 1 pass.
13. **Authoritative observer, once, before release**: run a full session under Little Snitch (or LuLu) in silent/log-only mode. It captures connection *attempts* with resolved domains even when blackholed — the one thing `lsof`/`nettop` cannot show.
14. Record the recipe and results in `research/network-verification.md` so it can be re-run for every future release.

### 11c. Functional regression pass

15. **Extensions**: install one from the registry, confirm download + activation. This egress is intentionally allowed.
16. **LSP auto-download**: open a file in a language whose server is not yet installed; confirm it downloads and attaches.
17. **SSH remote development**: connect to a remote host and open a project. This exercises the Phase 3d replacement for `AutoUpdater::download_remote_server_release` — **the single least-tested change in the plan.**
18. **Hang traces**: confirm the Phase 6d retained hang detector still writes to disk.
19. **Legacy config**: launch with a pre-fork `settings.json` and `keymap.json`; confirm no panic and only expected warnings.
20. **Base keymaps**: cycle through all seven base keymap settings (repeat of Phase 9d step 15 against the release build).

## Todo List

- [ ] 11a `cargo check --workspace` **and** `--all-features` green
- [ ] 11a `cargo build --release` green; binary size recorded
- [ ] 11a Test suite green; no new failures vs Phase 10
- [ ] 11a `script/clippy` clean (`--deny warnings`, `cargo machete`, `typos`)
- [ ] 11a `cargo tree -i` reports "not found" for all sampled deleted crates
- [ ] 11a Ripgrep sweep clean
- [ ] 11a Dead `[patch.crates-io]` entries pruned
- [ ] 11a `post-deletion-metrics.md` written
- [ ] 11b Static endpoint sweep — only expected survivors
- [ ] 11b `lsof` snapshot clean
- [ ] 11b `nettop` session log clean
- [ ] 11b Blackhole **Tier 1** (telemetry/crash/LLM/collab) — zero traffic
- [ ] 11b Blackhole **Tier 2** (`api.zed.dev`, separate run) — graceful degradation, no crash/hang/retry storm
- [ ] 11b Little Snitch session run and reviewed
- [ ] 11b `network-verification.md` written
- [ ] 11c Extension install works
- [ ] 11c LSP auto-download works
- [ ] 11c **SSH remote development works**
- [ ] 11c Hang traces written to disk
- [ ] 11c Legacy `settings.json`/`keymap.json` load cleanly
- [ ] 11c All 7 base keymaps clean

## Success Criteria

- Every command in Gate A green, including `--all-features` and `script/clippy`.
- Zero connections to any telemetry/crash/LLM/collab endpoint under `nettop` + Little Snitch during a full QA pass.
- The **only** `zed.dev` traffic observed is the extension registry, and it is documented as a known retained dependency.
- Both blackhole tiers pass: Tier 1 silent, Tier 2 degrades gracefully.
- Extensions, LSP download, and SSH remote dev all functional.
- `post-deletion-metrics.md` and `network-verification.md` committed.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| `--all-features` skipped | A feature flag still names a deleted crate; breaks for someone later | Step 1 runs it explicitly |
| Clippy findings suppressed with `#[allow]` | Dead fields and unused imports ship as permanent debt | Fix, don't suppress; `--deny warnings` is the gate |
| Network verified only by grep | Claim is unproven; a runtime call path is missed | Five-layer recipe; Little Snitch catches what grep cannot |
| SSH remote dev untested | The Phase 3d replacement is the least-exercised change in the plan | Step 17 is a mandatory checklist item |
| Only tested with a fresh config | Real users have legacy configs | Step 19 explicitly tests a pre-fork config |
| Test failures rationalised away | Regression ships | Compare against two recorded baselines, not intuition |

## Security Considerations

This is the phase where the security/privacy claim becomes defensible.

- Review the **cumulative diff** of the branch with security eyes: removing crash reporting and telemetry is security-relevant, and so is the new remote-server binary download path added in Phase 3d. **Confirm that download uses HTTPS and verifies a checksum or signature** — an unauthenticated fetch that then executes on a remote host is a supply-chain hole.
- Confirm no credential-handling code path remains live (`ClientCredentialsProvider` may exist as a type but must have no caller).
- Record the verification recipe so the claim can be re-established for every release, not just this one.

## Next Steps

Phase 12 — rebrand and packaging. Requires the new brand name, the one open question that has been deferred throughout.
