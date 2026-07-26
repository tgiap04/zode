---
passed: true
issues: 0
warnings: 6
---

# Data Model Review — Wave 1.5 Gate (re-check)

## Passed Checks
- ✓ Check 1 — Entity completeness
- ✓ Check 2 — DISC-### scope
- ✓ Check 3 — MODEL### uniqueness
- ✓ Check 4 — DISC-### orphan check (critical items resolved; 1 pre-existing warning carried forward)
- ~ Check 5 — Relationship completeness cardinality (warning-only, unchanged from prior pass, does not gate)

## Check 1 — Entity completeness
PASS. All 16 entity blocks (Workspace, Project, Worktree, Entry, TextBuffer, Buffer, MultiBuffer, Editor, Thread, Message, SettingsStore, Theme/ThemeFamily, GitStore/Repository, ProjectPanel, Terminal, ExtensionManifest) have a name, description, and ≥1 field with name+type documented. Unchanged from prior pass.

## Check 2 — DISC-### scope
PASS — prior critical resolved:
- DISC-017 (`ExtensionCapability`, ExtensionManifest block, line 551) — now enumerates 3 named values (ProcessExec, DownloadFile, NpmInstallPackage) each with a distinct behavioral description, sourced to `crates/extension/src/capabilities.rs:14`. No longer single-value.

All other DISC codes (002-005, 007-010, 012-016) continue to have ≥2 named values with distinct behavioral outcomes.

## Check 3 — MODEL### uniqueness
PASS (trivially) — document contains zero `MODEL###` codes; only `DISC-###` codes are used throughout, 17 total, sequential, no gaps, no duplicates.

## Check 4 — DISC-### orphan check
PASS — prior 3 orphans now anchored to real attribute-table rows:
- DISC-001 (`OpenMode`, Workspace block) — anchored to new `open_mode (call-param)` row, data-model.md:65.
- DISC-006 (`ParseStatus`, Buffer/language block) — anchored to new `parse_status` row, data-model.md:230.
- DISC-011 (`Role`, Message block) — anchored to new `role (derived)` row, data-model.md:356.

- **WARNING (carried forward, not gating)** DISC-014 (`WorkDirectory`, GitStore/Repository block, line 460) — still no table row literally named `WorkDirectory` in the Repository attribute table (lines 436-449); values (`InProject`, `AboveProject`) appear only in the Discriminator Fields table and prose. Not part of this fix round; recommend anchoring in a future pass.

## Check 5 — Relationship completeness cardinality (warning-only, does not gate)
Unchanged from prior pass — 5 relationship lines still missing an explicit cardinality label:
- Editor block (line 299): "Referenced by `ProjectPanel` (...) and many other panels for inline rename/filter inputs" — no cardinality stated.
- SettingsStore block (line 392): "Referenced by nearly every other entity indirectly (...)" — no cardinality stated.
- Theme/ThemeFamily block (line 420): "Referenced (contributed) by `ExtensionManifest.themes` (...)" — no cardinality stated.
- ProjectPanel block (line 484): "References `Entry` (via worktree snapshots, not owned)" — no cardinality stated.
- ProjectPanel block (line 485): "Owns one `Editor` (rename/filename input)" — implies One-to-One but not labeled as such.

## Recommendation
PASS — all 4 prior critical issues (DISC-017 single-value, DISC-001/006/011 orphaned) are resolved. 6 warnings remain (1 orphan-adjacent: DISC-014; 5 cardinality-label gaps in Check 5), none gating per instructions.
