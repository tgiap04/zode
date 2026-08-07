---
failed: 0
warnings: 1
missing: 0
result: PASS
---

# Review Report — JobList (`--jobs` pass, Wave J.3)

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: `plans/260726-1400-rebuild-spec/artifacts/job-list.md` — 164 JOB### entries (2 scheduled-job, 62 queue-worker, 100 custom-command)

---

## Summary

| Metric | Value |
|--------|-------|
| JOB### entries reviewed | 164 (full index) + 15 detail sections spot-checked |
| Critical issues | 0 |
| Warnings | 1 |
| Missing | 0 |
| Result | **PASS** |

Deterministic pre-check (J.2 `validate_job_list.py`): PASS, 0 critical, 0 warnings. Citation presence, JOB### regex/uniqueness, BL Ref presence+resolution, `.job-list.completed` marker, and secrets gate are `[deterministic-pass]` — not re-checked here.

---

## Critical Issues

_(none)_

---

## Warnings

### W1: Inconsistent `**BL Ref**` citation format (bare BL### vs full BL###_Slug) — OPEN
- **Severity**: warning
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/job-list.md:189` (JOB001, representative of the pattern; affects JOB001–033 and JOB067–164, 131/164 rows)
- **Description**: `behavior-logic.md` canonically names every BL entry as `BL###_NameSlug` (e.g. `docs/generated/behavior-logic.md:255` → `## BL001_ActivityIndicatorStatusActions`). Only JOB034–JOB066 (33/164 rows, e.g. `job-list.md:981` `**BL Ref**: BL034_RegisterZedSchemeAction`) cite the full slugged form. The other 131 rows (JOB001–033, JOB067–164) cite the bare numeric form only (e.g. `job-list.md:189` `**BL Ref**: BL001`, `job-list.md:1773` `**BL Ref**: BL067`). Numeric resolution is unaffected (J.2 validator passed), but the inconsistency suggests the three batch authors used different citation conventions and a slug-based cross-ref lookup (e.g. grep for the exact anchor) would silently miss 80% of rows.
- **Fix**: Normalize all `**BL Ref**` values to the full `BL###_NameSlug` form used in `behavior-logic.md`, matching the JOB034–066 style.

---

## Passed Checks

✓ JOB-S1 @ JOB101_DebouncedDelayTimer (crates/project/src/debounced_delay.rs:26)
✓ JOB-S1 @ JOB102_GitDiffDebounceRecalculation (crates/project/src/project.rs:3616)
✓ JOB-S1 @ JOB103_LogCommandInvocation (crates/command_palette/src/command_palette.rs:561)
✓ JOB-S1 @ JOB120_CommitStagedChanges (crates/git_ui/src/git_panel.rs:2280)
✓ JOB-S1 @ JOB133_LoadBufferContentsOffThread (crates/project/src/buffer_store.rs:848)
✓ JOB-S1 @ JOB162_CreateWorktreeEntryOnDisk (crates/worktree/src/worktree.rs:845)
✓ JOB-S1 @ JOB163_WatchThemeFileChanges (crates/zed/src/main.rs:1543)
✓ JOB-S1 @ JOB164_RunKeymapOrSettingsMigration (crates/zed/src/zed/migrate.rs:247,250)
✓ JOB-S2 (type filter: scheduled-job/queue-worker/custom-command) @ BL067, BL100, BL144, BL145, BL146, BL150, BL165, BL180, BL200, BL207 (10/10 sampled)
✓ JOB-S3 (no fabricated schedule/retry — traceable or explicit N/A) @ JOB101..JOB164 (25/25 sampled)
✓ JOB-S4 (DRY — no verbatim BL### Description/Related Data Models copy) @ JOB101, JOB102, JOB120, JOB133 (4/4 sampled)
✓ JOB-S5 (no secret leakage beyond deterministic gate) @ job-list.md (full-file grep, 0 matches for secret/token/password/API-key patterns)
✓ JOB-S6 (systemd-timer pairing) — N/A, no systemd-sourced jobs present (native GPUI desktop app; confirmed via full-file grep for "systemd"/".timer"/"ExecStart", 0 matches)
✓ JOB### sequencing (file-global, contiguous 001–164, no gaps/duplicates) @ job-list.md (164/164, deterministic + independently re-verified via grep/awk)

---

## Edge Cases Turned Up

- The Job Index table's `Schedule/Trigger` column is truncated (character-clipped, no ellipsis) for readability in a markdown table — cosmetic only, full text is present in each per-job `### Schedule / Trigger` section. Not a defect.
- JOB030 (`ActionsMacroDefinition`) and its BL ref correctly document a compile-time macro with `N/A` schedule/retry — verified this isn't a lazy boilerplate slap-on, since it's the one entry in the sample set with a genuinely non-runtime nature and the N/A carries a specific rationale ("compile-time declarative macro, not an invokable command").
- `custom-command` entries are explicitly scoped in the doc header as GPUI `actions!()` dispatch, not CLI batch jobs — this framing is honest about the taxonomy mismatch rather than force-fitting Zed's UI actions into a traditional "job" mental model.

---

## Done Well

- Citation accuracy is strong: every one of the 8 explicitly re-verified `file:line` citations (across scheduled-job, queue-worker, and custom-command types, spanning early/mid/late batches) resolves to the exact function/symbol described.
- `## Purpose` and `## Data Touched` sections consistently paraphrase and add job-specific detail (read/write direction, trigger conditions) rather than echoing `behavior-logic.md`'s `### Description` / `### Related Data Models` verbatim — DRY boundary is respected in every sampled row.
- `N/A` boilerplate for "no persisted data" / "no retry policy" is applied honestly — sampled rows with real data or retry behavior (e.g. JOB005/JOB008/JOB010 `DebugSession` read/write, JOB102/JOB133's error-propagation-not-retry semantics) get concrete descriptions instead of the boilerplate.
- JOB### sequencing survived the 5-batch merge cleanly: no gaps, no duplicates, contiguous 001–164 across all batch boundaries.

---

## Actions In Order

1. Normalize the 131 bare-form `**BL Ref**` citations to the full `BL###_NameSlug` form for consistency with `behavior-logic.md` and the 33 rows that already do this correctly (W1).

---

## Numbers

- JOB### entries: 164 (100% present, sequential, no gaps/duplicates)
- Citation spot-check pass rate: 8/8 (100%)
- Type-filter spot-check pass rate: 10/10 (100%)
- DRY spot-check pass rate: 4/4 (100%)
- Secret-leakage grep: 0 matches
- Systemd-timer entries: 0 (N/A for this codebase)

---

## Still Unresolved

- W1 (BL Ref format inconsistency) — cosmetic/consistency issue only, does not block promotion; recommend fixing before this artifact is treated as a stable cross-ref source for future tooling.
