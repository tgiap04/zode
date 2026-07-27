---
phase: 4
title: Atomic structural cut
status: completed
effort: 0.5d
---

# Phase 4: Atomic structural cut

## Context Links

- [Workspace surgery research](./reports/research-workspace-surgery.md) §3 — cargo mechanics
- [Final delete set](./research/final-delete-set.txt) — the authoritative 53
- [plan.md](./plan.md)

## Overview

**Priority:** P1 · **Build state:** 🔴 **GOES RED HERE — stays red until Phase 11** · **Depends on:** Phase 3

One commit removes 53 crates: workspace member lines, `[workspace.dependencies]` entries, and the directories themselves. **These three edits cannot be sequenced** — a stale path in `[workspace.dependencies]` fails `cargo metadata` for the entire workspace before any per-crate error can surface.

This phase is short in wall-clock and enormous in consequence. Everything after it is recovery.

## Key Insights

- The three structural edits are atomic by cargo's own semantics, not by preference. A partial edit produces a workspace that cannot even be *queried*, let alone compiled.
- Do **not** run `cargo update --workspace` to regenerate the lockfile — it also bumps external crate versions, mixing an unrelated change into a structural commit and destroying the reviewability of the diff. Plain `cargo check` drops unreachable packages on its own.
- `[patch.crates-io]` (`Cargo.toml:872-880`) patches `livekit`, `libwebrtc`, `webrtc-sys` — these become dead once `livekit_client`/`livekit_api` go. Cargo **ignores unused patches**, so this is harmless. Prune in Phase 11, not here.
- `default-members = ["crates/zed"]` (`:254`) is unaffected — `zed` survives. A bare `cargo build` is therefore a cheap first smoke test.
- After this commit, `cargo check --workspace` will emit hundreds of errors. **Do not start fixing from that output.** Capture it as a census (step 7) and then work crate-by-crate in the Phase 1 topological order.

## Requirements

**Functional**
- All 53 crates removed from members, workspace dependencies, and disk, in one commit.
- `cargo metadata` must still *parse* (this is the only green-ish signal available in this phase).

**Non-functional**
- The commit must be revertible as a single unit.
- `Cargo.lock` changes limited to package removals — zero version bumps.

## Architecture

```
Cargo.toml
├── [workspace] members            :3-253    ── delete 53 path strings
├── [workspace.dependencies]       :260-493  ── delete 53 entries      ┐ ATOMIC
└── (leave [patch.crates-io] :872-880 alone)                           │
crates/<name>/                               ── rm -rf × 53            ┘
Cargo.lock                                   ── regenerate via `cargo check`, never `cargo update`
```

## Related Code Files

**To delete (53 directories under `crates/`)**

Collab (6): `collab` `collab_ui` `call` `channel` `livekit_client` `livekit_api`
Copilot (3): `copilot` `copilot_chat` `copilot_ui`
Agent (5): `agent` `agent_ui` `agent_servers` `agent_settings` `ai_onboarding`
ACP (2): `acp_thread` `acp_tools`
Language models (4): `language_models` `language_model` `language_model_core` `language_models_cloud`
Edit prediction (5): `edit_prediction` `edit_prediction_ui` `edit_prediction_cli` `edit_prediction_context` `edit_prediction_metrics`
Web search (2): `web_search` `web_search_providers`
Vendors (10): `anthropic` `open_ai` `google_ai` `bedrock` `deepseek` `mistral` `ollama` `open_router` `lmstudio` `codestral`
Cloud (2): `cloud_llm_client` `cloud_api_client`
Auto-update (3): `auto_update` `auto_update_ui` `auto_update_helper`
Crash (1): `crashes`
Eval (2): `eval_cli` `eval_utils`
Sidebar (1): `sidebar`
Auto-orphaned (7): `aws_http_client` `opencode` `rules_library` `streaming_diff` `vercel` `x_ai` `zeta_prompt`

**To modify**
- `Cargo.toml` — members + `[workspace.dependencies]`

**Explicitly NOT deleted** (verified keep-list): `edit_prediction_types` · `remote` · `remote_connection` · `remote_server` · `context_server` · `cloud_api_types` · `notifications` · `client` · `telemetry` · `telemetry_events` · `proto` · `rpc` · `audio` · `feature_flags` · `denoise`

## Implementation Steps

1. Confirm Phase 3 landed and the tree is clean and green, then **tag the escape hatch**:
   ```sh
   git status --short && cargo check --workspace 2>&1 | tail -3
   git tag pre-structural-cut
   ```
2. Re-run the delete-set script one final time; treat its output as authoritative:
   ```sh
   python3 research/final-delete-set.py | tee research/final-delete-set.txt
   ```
3. **Before deleting**, build the authoritative reverse-dependency map for the record:
   ```sh
   for c in $(cat research/delete-list.txt); do
     echo "=== $c"; cargo tree -i "$c" -e normal 2>/dev/null | head -20
   done > research/pre-delete-reverse-deps.txt
   ```
4. Delete the 53 directories: `rm -rf crates/<name>` for each.
5. `Cargo.toml` — delete the 53 lines from `[workspace] members` (`:3-253`).
6. `Cargo.toml` — delete the 53 matching entries from `[workspace.dependencies]` (`:260-493`). **Same commit as step 5. Non-negotiable.**
7. Regenerate the lockfile the safe way and take the error census:
   ```sh
   cargo metadata --no-deps --format-version 1 > /dev/null   # must PARSE, even though code won't compile
   cargo check --workspace --message-format=json 2>/dev/null \
     | jq -r 'select(.reason=="compiler-message") | .target.name' \
     | sort | uniq -c | sort -rn > research/post-cut-error-census.txt
   ```
   This census is a *map*, not a worklist. Compare it against the predicted 17 survivors.
   > `onboarding` and `project` will appear even though the crate-level graph did not predict them — expected, see plan.md findings 4 and 5.
8. Verify `Cargo.lock` contains **only removals**:
   ```sh
   git diff Cargo.lock | grep '^+' | grep -v '^+++' | grep 'version =' | head
   ```
   Any line here means a version bump slipped in — revert the lockfile and redo with plain `cargo check`.
9. Commit as one unit: `refactor!: remove auth, collab, AI and cloud subsystems (53 crates)`.

## Todo List

- [ ] Phase 3 confirmed landed, workspace green
- [ ] `final-delete-set.py` re-run; list is authoritative
- [ ] Pre-delete reverse-dependency map captured
- [ ] 53 directories removed
- [ ] 53 `[workspace] members` lines removed
- [ ] 53 `[workspace.dependencies]` entries removed
- [ ] `cargo metadata` parses
- [ ] Error census captured to `research/post-cut-error-census.txt`
- [ ] `Cargo.lock` diff contains **only** removals — zero version bumps
- [ ] Single atomic commit

## Success Criteria

- `cargo metadata --no-deps` **parses without error** — the only success signal available in this phase.
- `crates/` contains 179 directories (down from 232); `cargo metadata` reports 187 packages (down from 240).
- The error census names roughly the predicted 17 survivors (15 from the graph + `onboarding` + `project`) and no unexpected crate.
- `Cargo.lock`: removals only.
- One revertible commit.

## Abort & rollback protocol

**Red team finding 4.** Phase 4 is described as "one revertible commit" — true in isolation, false in practice once Phases 5–8 land on top of it. By mid-Phase-8, `client` has been gutted (Phase 5) on the assumption `cloud_api_client` is gone. Reverting Phase 4 alone would resurrect the crate while leaving Phase 5's deletions in place — a workspace that is neither the pre-cut nor the post-cut state.

**Therefore: `git revert <phase-4-commit>` is not a rollback strategy after Phase 5 begins.** The real strategies are:

| Situation | Action |
|---|---|
| Problem found **before** Phase 5 starts | `git revert` the Phase 4 commit. Clean. |
| Problem in one survivor patch (Phases 5–8) | Revert **that crate's commit only**, in reverse chronological order. This is why every phase mandates one commit per crate. |
| Problem is systemic — the delete set itself is wrong | `git switch main` and re-branch. **Do not** try to unwind incrementally. Phases 1–3 are re-runnable from scratch in under two days; the analysis artifacts in `research/` survive because they are committed. |
| Phase 8 exceeds **12 working days** (1.5× the 8-10d estimate) | **Stop and reassess.** Do not push through. Either narrow the delete set (see the Phase 3 keep-list pattern) or accept the incremental path after all. |

**Tag before cutting** so the escape hatch is unambiguous:
```sh
git tag pre-structural-cut
```

**Commit granularity is the rollback mechanism.** Every phase from here specifies one commit per crate. That is not tidiness — it is the only unit of recovery available during the red period.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Something goes irrecoverably wrong mid-Phase-8 | No viable rollback; days of work in an unbuildable state | The abort protocol above; `pre-structural-cut` tag; per-crate commits; the 12-day Phase 8 tripwire |
| Member list and workspace.dependencies edited in separate commits | `cargo metadata` fails; nothing is diagnosable | Steps 5–6 explicitly in one commit; step 7 verifies parse |
| `cargo update` used for the lockfile | Version bumps buried in a structural diff | Step 8 asserts removals-only |
| Census shows an **unexpected** crate erroring | A dependency edge was missed in analysis | Investigate before proceeding — do not just fix it; the delete set may be wrong |
| Attempting to fix errors from the `--workspace` output | Hours of chasing errors that vanish when their dependency is fixed | Census is explicitly labelled a map, not a worklist; Phases 7–8 use `-p` in topological order |
| Deleting a crate still referenced by an asset/data file | Runtime panic, invisible to the compiler | Phase 9 handles all data files; do not attempt it here |

## Security Considerations

This commit removes the entire outbound-network surface for auth, telemetry upload, crash upload, and LLM providers. It is the commit that delivers the privacy goal. It also removes `crashes`, so from here until Phase 11 the app has no crash instrumentation — expected and accepted.

## Next Steps

Phase 5 — gut `crates/client`. Single-threaded, done alone, highest care of any survivor patch.
