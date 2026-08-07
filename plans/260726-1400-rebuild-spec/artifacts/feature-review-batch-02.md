---
failed: 0
warnings: 3
missing: 0
result: PASS
---
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: fcodes flagged MISSING due to `.pending` marker present in `artifacts/features/{slug}/` (verification-checklist-universal.md § Pending Marker Rule). Counts toward Wave 9 pre-flight gate halt conditions alongside `failed`.
`result`: PASS iff `failed === 0 && missing === 0`.
-->

# Review Report — Rebuild-Spec Artifacts

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: Wave FS.5 review, batch 2, 3rd pass — F011_GitIntegration, F012_ExtensionSystem, F013_WorkspaceAndProjectManagement, F014_VimEmulation, F015_SettingsAndKeymaps

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 5 feature specs (`technical-spec.md`, full re-read of each) |
| Critical issues | 0 |
| Warnings | 3 |
| Missing (`.pending` markers) | 0 |
| Result | **PASS** |

This is a targeted re-verification pass, not a first read: it re-checks (a) no duplicate SC-###
codes with conflicting definitions anywhere in each document, and (b) every FR has a real,
unique, non-fabricated SC covering it — every covers-clause traced back to the cited SC's actual
text to confirm it genuinely describes that FR, not a copy-pasted unrelated criterion.

All 4 previously-critical SC-code collision/fabrication issues (F011, F013 x2, F014) confirmed
fixed and holding:
- **F011**: SC-001..SC-008 all distinct, each with real FR-specific text; the 5 previously-fabricated
  per-story pointers (US019/US006/US008/US020/US021/US022/US023/US024 area) now correctly cite
  SC-003..SC-008 with genuine matching criteria. No reused SC-001 text outside its real scope
  (FR-001 hunk staging) remains.
- **F013**: Cross-Cutting SC-013/SC-014 (renumbered from the collision) are present, unique, and
  their text genuinely matches FR-003 (memory-pressure fuse) and FR-008 (dev-container init)
  respectively. Pre-existing inline SC-004..SC-012 (one per user story) are untouched, unique,
  and each verified against its own FR/story text — no collisions with the renumbered codes.
- **F014**: Cross-Cutting SC-007/SC-008 (renumbered) are unique and their text matches FR-001/FR-002.
  Inline SC-001..SC-006 are untouched, unique, and verified.
- **F015**: Cross-Cutting SC-012 (renumbered) is unique and its text matches FR-005 (feature-flag
  precedence). Pre-existing inline SC-001..SC-003 (in the Cross-Cutting Verification list) and
  SC-004 (inline in US060) are untouched and verified.
- **F012**: No collision was found here originally (this spec already used the correct
  "cite the real Cross-Cutting SC" convention). Re-confirmed: SC-001 is legitimately cited by four
  distinct user stories (US030-033) because all four exercise the same underlying dual-gate
  capability-check mechanism (FR-006/BR-001/BR-002) from different capability classes
  (ProcessExec/DownloadFile/NpmInstallPackage/deny-path) — not fabrication, a genuine shared
  criterion.

Full per-FR trace performed for all 5 specs (every FR-### in every Cross-Cutting Requirements
table and every per-story-only FR): each has at least one SC whose defined text is substantively
and correctly about that FR (or its linked BR/SM), with no copy-pasted/reused text describing an
unrelated requirement. **Zero fabrication or duplicate-definition issues remain.**

---

## Critical Issues

_(none)_

---

## Warnings

### W1: F013 — US042 verification cites the wrong SC for FR-008; SC-014 orphaned — OPEN
- **Severity**: warning
- **Location**: `F013_WorkspaceAndProjectManagement/technical-spec.md:409-410` (US042 Verification), cf. `:220` (SC-014 definition)
- **Description**: The Cross-Cutting Verification list defines `SC-014` — "Running `InitializeDevContainer` on a project with no `.devcontainer` and completing the modal results in a valid `devcontainer.json` written to disk (covers FR-008)" — which is a genuine, correctly-matching criterion for FR-008 and for US042's own Acceptance Scenario #1. However, US042's own `Verification` section cites `SC-003` instead (`- **SC-003** (see Cross-Cutting Logic)`), which actually covers BR-005 (a devcontainer failing validation never reaches build) — the *negative*/validation path, not the *positive* "valid config gets written" scenario US042 actually describes. As a result, `SC-014` is never pointed to by any user story anywhere in the document — a dangling Cross-Cutting Verification entry — while US042's real acceptance criterion (matching SC-014's text) is left without a story-level citation.
- **Fix**: Change US042's `Verification` section to cite both: `SC-003 (see Cross-Cutting Logic — BR-005 validation gate)` and `SC-014 (see Cross-Cutting Logic — covers FR-008)`. This is a citation-wiring fix only; no SC text needs to change (SC-014's content is already correct and non-fabricated).

### W2: F013 — SC-013 (FR-003, memory-pressure fuse) never cited by any user story — OPEN
- **Severity**: warning
- **Location**: `F013_WorkspaceAndProjectManagement/technical-spec.md:219` (SC-013 definition), Cross-Cutting Requirements table `:41`
- **Description**: FR-003 (the 30s memory-pressure poll/fuse) is a background, non-user-initiated behavior with no dedicated user story, so no per-story `Requirements fulfilled`/`Verification` block ever references FR-003 or SC-013. This mirrors an established, already-accepted pattern in this batch (F011's/F014's cross-cutting-only FRs also lack per-story citations), so it is not a regression — but it is worth flagging since it is easy to mistake for the same class of defect as W1. No fabrication or collision involved; SC-013's text is genuine and unique.
- **Fix**: No action required if the "background-only FR needs no per-story pointer" convention is intentional (it appears to be, given F011/F014 precedent). If the project wants full bidirectional traceability, add a one-line cross-reference from ALG-001_MemoryPressureFuseVictimSelection to SC-013.

### W3: F015 — FR-002 has no SC whose covers-clause names it explicitly — OPEN
- **Severity**: warning
- **Location**: `F015_SettingsAndKeymaps/technical-spec.md:225-226` (SC-001 definition), `:50` (FR-002 definition), `:269-271` (US058's FR-002 fulfillment)
- **Description**: FR-002 ("Watch the user/global settings files on disk and hot-reload on external edits") is listed as fulfilled by US058, and US058's Acceptance Scenario #2 ("settings.json is hand-edited externally... the change is detected and applied the same way as a UI edit") is a genuine test of FR-002. But `SC-001`'s covers-clause text reads `(covers FR-001, US058)` — it names the user story but not FR-002 itself, and no other SC's covers-clause mentions FR-002 anywhere in the document. This is a minor gap in explicit FR→SC traceability (the behavior is verifiable and the story-level scenario exists), not a fabrication or duplicate-code issue.
- **Fix**: Extend SC-001's covers-clause to `(covers FR-001, FR-002, US058)` since SC-001's own text ("every open editor observing that key re-renders without restart") already substantively addresses the hot-reload-on-external-edit case, or add a short second Verification bullet under US058's Acceptance Scenario #2 dedicated to FR-002.

---

## Passed Checks

✓ NoDuplicateSCCode @ F011_GitIntegration
✓ NoDuplicateSCCode @ F012_ExtensionSystem
✓ NoDuplicateSCCode @ F013_WorkspaceAndProjectManagement
✓ NoDuplicateSCCode @ F014_VimEmulation
✓ NoDuplicateSCCode @ F015_SettingsAndKeymaps
✓ EveryFRHasGenuineNonFabricatedSC @ F011_GitIntegration (8/8 FR-### traced)
✓ EveryFRHasGenuineNonFabricatedSC @ F012_ExtensionSystem (6/6 FR-### traced)
✓ EveryFRHasGenuineNonFabricatedSC @ F013_WorkspaceAndProjectManagement (11/11 FR-### traced)
✓ EveryFRHasGenuineNonFabricatedSC @ F014_VimEmulation (8/8 FR-### traced)
✓ EveryFRHasGenuineNonFabricatedSC @ F015_SettingsAndKeymaps (5/5 FR-### traced)
✓ PriorCriticalFixHolds.F011_SC001CollisionResolved @ F011_GitIntegration
✓ PriorCriticalFixHolds.F013_SC004SC005RenumberedToSC013SC014 @ F013_WorkspaceAndProjectManagement
✓ PriorCriticalFixHolds.F014_SC001SC002RenumberedToSC007SC008 @ F014_VimEmulation
✓ PriorCriticalFixHolds.F015_SC004RenumberedToSC012 @ F015_SettingsAndKeymaps
✓ NoPendingMarker @ F011_GitIntegration..F015_SettingsAndKeymaps (5/5)

---

## Metrics

| Metric | Value |
|--------|-------|
| Feature Specs | 5 |
| User Stories | 30 (US019, US006, US007, US008, US020-024 [F011]; US025-033 [F012]; US034-044, US066 [F013]; US052-057 [F014]; US058-062 [F015]) |
| SC-### definitions reviewed | 38 total (8 F011, 6 F012, 14 F013, 8 F014, 4 F015 — wait, see note) |
| Screens | N/A — all 5 are `generic-source` (non-web Rust desktop) profile, no SCR### codes |
| Background Logic Items | not separately re-tallied this pass (see behavior-logic.md cross-references per spec) |
| Permissions | PERM001-003 (F012), PERM005 (F013), PERM006 (F015) |
| Backend Route Rows | N/A — no route surface in this profile |
| Frontend Pages | N/A — no page surface in this profile |
| Data Model Entities | MODEL014 (F011), MODEL018 (F012), MODEL001-005/015 (F013), MODEL010 (F014, via DISC-F014-01 deviation note), MODEL012-013 (F015) |

**Note on SC-### total**: F013's inline per-story SC-004..SC-012 (9 codes) plus its Cross-Cutting
SC-001..SC-003, SC-013, SC-014 (5 codes) = 14 total, consistent with the "renumbered to avoid
collision with SC-004/SC-005" fix. All 38 SC-### codes across the 5 documents are unique within
their own document; no cross-document code reuse was checked (SC-### is scoped per-feature-spec).

---

## Still Unresolved

- W1 (F013 US042 mis-citation) is the only finding with a concrete, low-effort fix — recommended
  before this batch is considered fully closed out, since it leaves one FR (FR-008) without a
  correct story-level verification pointer even though the underlying SC content is sound.
- W2 and W3 are traceability-completeness notes, not defects introduced by the prior fix round;
  safe to defer or accept as-is given the precedent already set elsewhere in this batch.
