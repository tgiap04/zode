---
failed: 0
warnings: 0
missing: 0
result: PASS
---
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: N/A for this sidecar pass (test-cases.md is never a MISSING-eligible artifact).
`result`: PASS iff `failed === 0`.
-->

# Review Report — Test Cases Sidecar (Wave TC.3 / TC.4 re-review)

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: 5 `test-cases.md` drafts — F001_Terminal, F002_LanguageIntelligence, F008_EditorCore,
F009_Diagnostics, F010_Debugging. Checklist scope: TestCases semantic rules TC-S1..TC-S6 only
(`references/verification-checklist-test-cases.md`). Core/feature-spec sections are out of scope
for this pass.

This report replaces the prior batch-01 result. F001/F002/F008/F009 were already PASS in the
first pass and are carried forward unchanged (not re-diffed — no edits landed in those files
between passes). F010_Debugging is re-verified below after its fix cycle (Wave TC.4).

---

## Summary

| Metric | Value |
|--------|-------|
| Files reviewed | 5 `test-cases.md` sidecars (140 total test-case rows) |
| Critical issues | 0 |
| Warnings | 0 |
| Missing | 0 |
| Result | **PASS** |

**Validator notes:** TC.2 (`validate_test_cases.py`) passed clean (0 critical, 0 warnings) for
all 5 files, including the re-run after F010's fix cycle — `TC###` regex/uniqueness, `Type` enum,
`Traces-to` presence, and citation-source-family match are `[deterministic-pass]` and were not
re-checked here.

---

## F010_Debugging Re-Review (Wave TC.4)

Two issues from batch 1 were fixed and re-verified against
`docs/features/F010_Debugging/technical-spec.md`:

### C1 (was critical) — TC016 citation retargeted — RESOLVED
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/features/F010_Debugging/test-cases.md:37`
- **Fix verified**: `Traces-to` now reads `crates/project/src/debugger/session.rs:3144-3170`.
  Cross-checked against technical-spec.md:171 (`ALG-001_JsDebugCompanionVersionSelection` Source
  includes this range) and :207 (`INT-002_JsDebugCompanionNpmInstall` Source is exactly this
  range). ALG-001's description ("`get_or_install_companion` compares the newest locally
  installed version against the latest published npm version and, if newer, installs it in the
  background (`install_latest_version`) while the current session keeps using the
  already-installed binary") matches TC016's Given/When/Then claim precisely. TC-S1 now holds.

### W1 (was warning) — DISC-002/DISC-003 rows reshaped — RESOLVED
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/features/F010_Debugging/test-cases.md:28-35`
- **Fix verified**: TC007-009 (DISC-002 `Collab`/`Local`/`Shared`) and TC013-014 (DISC-003
  `Active`/`Warm`) rewritten as concrete developer-facing scenarios (guest clicking the gutter,
  a solo developer's local edit, an idle-timer/memory-pressure check, a defocus-plus-autosave
  race) rather than reassembled Render/Validation/Persistence table-cell text. Spot-checked
  against `technical-spec.md`'s DISC-002 (lines 22-30) and DISC-003 (lines 32-40) tables — the
  factual outcomes (host-forward via `proto::ToggleBreakpoint`/`upstream_client`; local-only
  write; host-commit-and-relay via `downstream_client`/`rpc`; no hibernation interference for
  Active; `hibernate_retry`-deferred hibernation for Warm) are unchanged and still accurate, and
  none of the five rows read as verbatim table-cell concatenation anymore. TC-S6 now holds.
- TC015 (DISC-003 `Hibernated`) was not in the fix list but was re-checked as a regression guard —
  it was already a reshaped scenario (not verbatim) in batch 1 and remains so; no drift.

Spot-check of TC001-006, TC010-012, TC017-019, and the UAT rows (TC021-029) found no regressions —
citations, Given/When/Then content, and table formatting (pipes, escaping) are unchanged from the
already-passing batch-01 state, consistent with TC.2's clean deterministic re-run.

---

## Passed Checks

✓ TC-S1 @ F001_Terminal
✓ TC-S2 @ F001_Terminal
✓ TC-S3 @ F001_Terminal
✓ TC-S4 @ F001_Terminal
✓ TC-S5 @ F001_Terminal
✓ TC-S6 @ F001_Terminal
✓ TC-S1 @ F002_LanguageIntelligence
✓ TC-S2 @ F002_LanguageIntelligence
✓ TC-S3 @ F002_LanguageIntelligence
✓ TC-S4 @ F002_LanguageIntelligence
✓ TC-S5 @ F002_LanguageIntelligence
✓ TC-S6 @ F002_LanguageIntelligence
✓ TC-S1 @ F008_EditorCore
✓ TC-S2 @ F008_EditorCore
✓ TC-S3 @ F008_EditorCore
✓ TC-S4 @ F008_EditorCore
✓ TC-S5 @ F008_EditorCore
✓ TC-S6 @ F008_EditorCore
✓ TC-S1 @ F009_Diagnostics
✓ TC-S2 @ F009_Diagnostics
✓ TC-S3 @ F009_Diagnostics
✓ TC-S4 @ F009_Diagnostics
✓ TC-S5 @ F009_Diagnostics
✓ TC-S6 @ F009_Diagnostics
✓ TC-S1 @ F010_Debugging
✓ TC-S2 @ F010_Debugging
✓ TC-S3 @ F010_Debugging
✓ TC-S4 @ F010_Debugging
✓ TC-S5 @ F010_Debugging
✓ TC-S6 @ F010_Debugging

---

## Edge Cases Turned Up

- F010's DISC-003 `Warm` (TC014) and `Hibernated` (TC015) rows still look superficially similar
  (both describe "hibernation gets deferred while a session runs") but target genuinely distinct
  trigger conditions (an in-flight `Warm` transition vs. an idle project that would otherwise move
  to `Hibernated`) — confirmed not padding, carried forward from batch 1, no action needed.
- Confirmed TC016's new citation does not collide with TC006's — TC006 still correctly owns
  `dap_store.rs:196-215` (construction-time version pruning, `BR-004`), and TC016 now owns
  `session.rs:3144-3170` (npm-compare-and-background-install, `ALG-001`/`INT-002`). No duplicate
  coverage, no gap.

## Done Well

- Every `UAT` row across all 5 files cites a real, verifiable `screens.md § User Journey step N`
  or `business-context.md § What They Do item N` section — none smuggle a code reference dressed
  as a section citation (TC-S2).
- `edge-cases.md` reuse is faithful row-for-row against the actual edge-cases.md scenario text in
  every instance checked (TC-S4).
- `[NO_TEST_CASE]` Coverage Notes rationale is substantive in every case (TC-S5).
- F010's fix cycle addressed both open findings precisely — the citation fix targets the correct
  narrower source range rather than a broad over-citation, and the DRY rewrite preserves factual
  accuracy while adding real scenario texture (TC-S1, TC-S6).

## Actions In Order

None — all findings from batch 1 are resolved.

## Numbers

- Rows reviewed in depth (TC-S1 code/file:line cross-check): 61 of 140 (all IT/UT rows citing a
  code or file:line across all 5 files), plus TC016 re-verified individually post-fix
- UAT rows cross-checked against screens.md/business-context.md (TC-S2): 39 of 39
- edge-cases.md citations cross-checked (TC-S4): 22 of 22
- `[NO_TEST_CASE]` rationale entries reviewed (TC-S5): 6 of 6
- F010 rows re-verified post-fix (TC-S1/TC-S6 targeted re-check): 6 of 29 (TC007-009, TC013-015,
  TC016), plus a regression spot-check of the remaining 23
- Lint/deterministic findings: 0 (TC.2 clean pre- and post-fix)

## Still Unresolved

None.
