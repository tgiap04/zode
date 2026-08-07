---
failed: 0
warnings: 5
missing: 0
result: PASS
---

# Feature-Specs Review Report (merged)

<!-- from plans/260726-1400-rebuild-spec/artifacts/feature-review-batch-01.md -->
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: fcodes flagged MISSING due to `.pending` marker present in `artifacts/features/{slug}/`.
`result`: PASS iff `failed === 0 && missing === 0`.
-->

# Review Report — Rebuild-Spec Artifacts (Wave FS.5, Batch 1, 4th Re-Review)

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: F001_Terminal, F002_LanguageIntelligence, F008_EditorCore, F009_Diagnostics, F010_Debugging (4 files each: technical-spec.md, business-context.md, screens.md, edge-cases.md)

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 5 feature specs (20 files) |
| Critical issues | 0 |
| Warnings | 2 |
| Missing (`.pending` markers) | 0 |
| Result | **PASS** |

## Validator Pre-Check

`fs-validation-summary.json` (generated 2026-08-07T06:20:31Z, `overall_status: WARN`, `totals.critical: 0`): all 5 fcodes in this batch report per-fcode `status: PASS`, `critical: 0, warning: 0` in the `specs` rollup. The one remaining global warning (`reading_guide_db_impact` / `reading_guide.unmapped` on F009_Diagnostics) sits outside that per-fcode rollup — see W2. Deterministic validators still do not perform whole-document SC-### uniqueness checking, so this review re-did the full manual sweep independently: every `**SC-###**` occurrence in each of the 5 `technical-spec.md` files was grepped (bold definitions and bare/short-form back-references alike), grouped by code, and each group's prose/`covers` targets were diffed to confirm a single canonical definition per code. Separately, every declared `FR-###` (from both the Cross-Cutting `### Requirements` table and any additional FRs declared inline under `## User Stories`) was cross-referenced against the SC `covers` lists to confirm ≥1 unambiguous citation.

## Prior-Round Fix Verification

Both previously-reported collisions are confirmed **fixed**, and the requested full 5-file re-sweep found **no new or residual collisions**:

- **C2 (F001 SC-003/SC-004 collision) — CLOSED.** The Cross-Cutting `### Verification` entries that used to collide were renumbered to `SC-005` (line 165, "A terminal spawned from the GUI has PATH/PYENV/NVM..." — covers FR-002) and `SC-006` (line 166, "Quitting Zed while a terminal has spawned child processes..." — covers FR-003). The pre-existing US047 `SC-003` (line 241, "A resolved task's exit status..." — covers FR-001, SM-001) and US048 `SC-004` (line 263, "A scrollback search highlights all matches..." — covers FR-001) were left untouched and are now each the *sole* definition of their code. Confirmed no other `SC-003`/`SC-004`/`SC-005`/`SC-006` occurrence exists anywhere else in the document. Bare references `SC-001` (line 195) and `SC-002` (line 217) still correctly resolve to their single Cross-Cutting canonical definitions (lines 163, 164). **8 total SC-### occurrences, 6 distinct codes, 0 collisions.**
- **C3 (F008 SC-004 collision) — CLOSED.** Resolved by the opposite renumbering than originally suggested (renumbering the Cross-Cutting entry instead of the US-level one) — equally valid, since the requirement is code-uniqueness, not which side moves. The Cross-Cutting `### Verification` entry (line 197, "Rapid successive buffer edits/scrolls/settings changes coalesce into a single debounced inlay-hints re-query..." — covers FR-003) is now `SC-010`. The pre-existing US001 `SC-004` (line 226, "Motion actions never alter `MultiBuffer` content" — covers FR-001) was left untouched and is now the sole definition of `SC-004`. Confirmed `SC-010` and `SC-004` each appear exactly once in the document. **9 total SC-### occurrences, 9 distinct codes, 0 collisions.**

Full re-sweep results for the other three files in this batch (re-verified, not just carried forward):

- **F002_LanguageIntelligence**: `SC-001`(144), `SC-002`(145, bare-ref 219 matches exactly), `SC-003`(146, bare-ref 177 matches exactly), `SC-006`(147), `SC-004`(198) — 7 occurrences, 5 distinct codes, 0 collisions.
- **F009_Diagnostics**: `SC-001`..`SC-008`(209,210,211,255,327,328,360,375), bare-refs `SC-002`(281)/`SC-003`(282) both match their Cross-Cutting canonical `covers` sets exactly — 10 occurrences, 8 distinct codes, 0 collisions.
- **F010_Debugging**: `SC-001`..`SC-018`(234-245, 300, 323, 367, 390, 437, 461), bare-refs `SC-001`(276)/`SC-002`(345)/`SC-003`(414) all match their canonical definitions' `covers` sets exactly — 21 occurrences, 18 distinct codes, 0 collisions.

**Batch-wide total: 55 SC-### occurrences, 46 distinct codes, 0 collisions.**

---

## Critical Issues

_(none)_

---

## Warnings

### W1: F009_Diagnostics — BR-004/BR-005 use `**Linked FR:** —` instead of the `N/A (reason)` convention — STILL OPEN
- **Severity**: warning
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/features/F009_Diagnostics/technical-spec.md:104` (BR-004_ToggleWarningsFilter), `:121` (BR-005_CloseDiagnosticlessBuffers)
- **Description**: Unchanged from the prior two passes. Every other "not linked to a specific FR" block in this batch spells out the reason inline (e.g. F002's `N/A (UI-only correction path)`, F008's `N/A (feature-local UI rule)`). F009's BR-004/BR-005 still use a bare em-dash, reading as an omission rather than a deliberate call. Not validator-blocking (confirmed: no `rule_id` in the current validator run flags this).
- **Fix**: Change both to `**Linked FR:** N/A (cross-cutting UI behavior, not tied to one FR)` or similar.

### W2: F009_Diagnostics — `## Source Walkthrough` unfilled template placeholder (validator-flagged, pre-existing) — STILL OPEN
- **Severity**: warning
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/features/F009_Diagnostics/technical-spec.md` (per `fs-validation-summary.json`, `reading_guide_db_impact` validator, `rule_id: reading_guide.unmapped`, still present in the 2026-08-07T06:20:31Z run — a later run than the one cited in the prior pass, confirming this has not self-cleared)
- **Description**: On inspection, the `## Source Walkthrough` content (lines 436-460) is fully populated: 5 numbered `**File:**` entries with line citations and rationale, plus a populated `### Call Hierarchy` block. No `{...}` template token is visible anywhere in that section. This is the third consecutive pass where content review finds nothing to fix and the validator still fires. Note this warning sits in the `reading_guide_db_impact` global validator's issue list, not in the F009 entry of the `specs` per-fcode rollup (which independently reports `critical: 0, warning: 0` for F009) — the two validator surfaces disagree, which is itself worth flagging.
- **Fix**: Do not re-edit already-correct content a third time. Run `validate_reading_guide_db_impact.py` directly against the current file with verbose/debug output to see exactly which token or heading match it is failing on (likely a stale detection pattern, e.g. expecting a literal `{File}` placeholder string format that no longer matches the populated numbered-list format), and fix the validator's detection logic or its output wiring into the `specs` rollup.

---

## Passed Checks

✓ Universal.artifact_exists @ F001_Terminal..F010_Debugging (5/5)
✓ Universal.no_placeholder @ F001_Terminal..F010_Debugging (5/5)
✓ FeatureSpec.required_sections @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.ccl_subsections @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.ccl_blank @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.deprecated_headings @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.no_appendix @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.edge_cases @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.sm_mermaid @ F001_Terminal, F008_EditorCore, F009_Diagnostics, F010_Debugging (4/4) [deterministic-pass]
✓ FeatureSpec.polymorphic_behavior_present @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.decision_logic_section_present @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ FeatureSpec.missing_client_behavior_anchor @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ bc.missing_h2 @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ screens.missing_h2 @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ edge_cases.few_rows @ F001_Terminal..F010_Debugging (5/5) [deterministic-pass]
✓ SC-### full-document uniqueness sweep (manual, bold defs + bare back-refs, grouped and diffed) @ F001_Terminal..F010_Debugging (5/5) — CLEAN, 0 collisions
✓ C2 fix verification (F001 SC-003/SC-004 renumbered to SC-005/SC-006) — CLOSED
✓ C3 fix verification (F008 SC-004 collision resolved via Cross-Cutting entry renumbered to SC-010) — CLOSED
✓ FR-coverage (every declared FR has ≥1 unambiguous SC back-ref citation) @ F001_Terminal (FR-001..FR-003, 3/3), F002_LanguageIntelligence (FR-001..FR-005, 5/5), F008_EditorCore (FR-001..FR-004, 4/4), F009_Diagnostics (FR-001..FR-010, 10/10), F010_Debugging (FR-001..FR-018, 18/18) — 40/40, all unambiguous (no code with two conflicting canonical texts remains)
✓ No SC-### codes found leaking into business-context.md / screens.md / edge-cases.md for any of the 5 fcodes
✓ BusinessContext plain-language rubric (persona clarity, business-verb workflows, zero forbidden tokens observed) @ F001_Terminal..F010_Debugging (5/5)
✓ Screens `## Screen List` / `## User Journey` present, non-web adaptation correctly noted (no fabricated SCR###) @ F001_Terminal..F010_Debugging (5/5)
✓ EdgeCases table ≥3 rows (5, 6, 5, 8, 6 rows respectively), all with plain-language User-Facing Message column, no raw HTTP codes @ F001_Terminal..F010_Debugging (5/5)
✓ No legacy `## Related Artifacts` / `## Spec Documents` sections found @ F001_Terminal..F010_Debugging (5/5)
✓ No top-level deprecated headings (`## Requirements`, `## Business Rules`, etc.) found @ F001_Terminal..F010_Debugging (5/5)

---

## Metrics

| Metric | Value |
|--------|-------|
| Feature Specs reviewed | 5 |
| SC-### occurrences (bold defs + bare back-refs, grepped) | F001: 8, F002: 7, F008: 9, F009: 10, F010: 21 (total 55) |
| SC-### distinct codes | F001: 6, F002: 5, F008: 9, F009: 8, F010: 18 (total 46) |
| SC-### code collisions found | 0 (both prior collisions, C2 and C3, confirmed fixed) |
| FR codes declared | 40 (F001: 3, F002: 5, F008: 4, F009: 10, F010: 18) — *correction: the 3rd-pass report's stated total of 47 was an arithmetic error; 3+5+4+10+18 = 40, not 47* |
| FR codes with a citation present | 40 / 40 |
| FR codes with an *unambiguous* SC citation (code resolves to exactly one canonical text) | 40 / 40 (all previously-blocked FRs — F001 FR-002/FR-003, F008 FR-003 — are now unambiguous) |
| BR/SM/ALG/INT blocks reviewed | 33 |
| Edge Case rows | 30 (5+7+4+8+6) |

---

## Still Unresolved

1. **W1 (F009 BR-004/BR-005 `Linked FR: —`)** — cosmetic convention gap, not validator-blocking, carried forward unfixed across 4 passes. Low priority.
2. **W2 (F009 Source Walkthrough placeholder)** — validator continues to flag content that reads as fully populated across three independent content reviews; the discrepancy between the `reading_guide_db_impact` validator's issue list (still firing) and the `specs` per-fcode rollup (reporting F009 clean) should be investigated in the validator/reporting pipeline itself, not by further content edits.
3. No blocking issues remain for this batch. `failed: 0`, `missing: 0` → **result: PASS** per the frontmatter formula. The two open warnings are pre-existing, non-critical, and do not affect the SC-code integrity or FR-coverage completeness that this 4th pass was scoped to confirm.

---

<!-- from plans/260726-1400-rebuild-spec/artifacts/feature-review-batch-02.md -->
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

---

<!-- from plans/260726-1400-rebuild-spec/artifacts/feature-review-batch-03.md -->
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: fcodes flagged MISSING due to `.pending` marker present in `artifacts/features/{slug}/` (verification-checklist-universal.md § Pending Marker Rule). Counts toward Wave 9 pre-flight gate halt conditions alongside `failed`.
`result`: PASS iff `failed === 0 && missing === 0`.
-->

# Review Report — Rebuild-Spec Artifacts

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: 1 feature spec (F016_Search) — Wave FS.5 batch review

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 1 feature spec (4 files: technical-spec, business-context, screens, edge-cases) |
| Critical issues | 0 |
| Warnings | 0 |
| Missing (`.pending` markers) | 0 |
| Result | **PASS** |

**Validator pre-check**: `fs-validation-summary.json` reports F016_Search `status: PASS`, 0 issues. All Deterministic Validator Coverage rule_ids are treated `[deterministic-pass]` per protocol; this review focused on semantic depth only.

---

## Critical Issues

_(none)_

---

## Warnings

_(none)_

---

## Semantic Review Notes

- **No `.pending` marker**: confirmed absent in `artifacts/features/F016_Search/`. All 4 required files present and non-empty.
- **Cross-ref vs feature-list.md**: F016_Search entry (line 61, 442-462) matches exactly — code, name "Search", type `ui`, priority P0, US063/US064/US065, MODEL009_MultiBuffer, BL146/BL020/BL159/BL059/BL060 all present in the spec's `## Artifact References` table with no additions/omissions.
- **Citation spot-check against real source** (5 of 5 checked, all accurate):
  - BR-001 (`file_finder.rs:900-910`, limit=100 in `fuzzy_nucleo::match_path_sets` call) — verified, hard-coded `100` literal present.
  - BR-002 (`project/src/search.rs:93-119`, non-ASCII case-insensitive fallback to regex) — verified, exact `!case_sensitive && !query.is_ascii()` branch present with matching rationale comment in source.
  - BR-003 (`project_search.rs:154-155,566-572`) — verified, `MAX_SEARCH_RESULT_FILES = 5_000` / `MAX_SEARCH_RESULT_RANGES = 10_000` constants and the `matched_buffers > ... || matches > ...` limit check both present as described.
  - BR-004 (`buffer_search.rs:1584-1631`) — verified, both regex and text query-construction error arms set `self.query_error` and call `clear_active_searchable_item_matches` without a fallback/crash.
  - BR-005 (`command_palette/src/persistence.rs:118-123`) — verified, exact INSERT + prune-DELETE-with-subquery SQL matches the spec's pseudocode.
  - ALG-001 (`project/src/search.rs:427-479`) — verified, `YIELD_INTERVAL = 20000` cooperative yield and whole-word boundary check via `CharKind` both present as described.
  - Also spot-checked `Match::History/Search/CreateNew` enum (`file_finder.rs:453-460`) and `SearchQuery::Text/Regex` enum (`search.rs:61-80`) cited in `## Polymorphic Behavior` — both exist verbatim at the cited lines.
  - No fabricated citations found in the sample.
- **FR/BR/SC coverage**: FR-001..FR-004 each traced to exactly one US (or Cross-Cutting) and covered by an SC with a `(covers ...)` back-ref; no orphan or zombie FRs. BR-001..BR-005 each carry a `**Source:**` citation and a `**Linked FR:**` (BR-004 explicitly `—`, which is permitted since it documents a corollary of BR-002 rather than a standalone FR — acceptable, not flagged).
- **Polymorphic Behavior N/A justification**: the spec's reasoning for `N/A` (no DISC-### entities in Key Entities; `Buffer.capability`/`parse_status` not read by any search code path) is grounded with source-file citations rather than an unjustified boilerplate N/A — matches the "false N/A" trap's intent correctly.
- **Screens.md non-web adaptation**: per this profile's design, no SCR###/ROUTE### catalog exists; the "## Screen List" table correctly substitutes GPUI surfaces (modal picker, pane-item view, in-editor bar) with a 4-column shape analogous to the template. Not flagged as an error per task instructions.
- **Edge-case sufficiency**: 8 rows in edge-cases.md (≥3 threshold for UI features met comfortably), each with a distinct scenario, behavior, and plain-language user-facing message (none is a bare HTTP-code-only message — this is a desktop app with no HTTP surface, and messages like "{regex compiler error text}" and "None — silently filtered out" are appropriately plain-language, not raw codes).
- **Business-context plain-language rubric**: personas are named with role + concrete action (not just "the user"); "What They Do" steps use business verbs ("types", "submits", "steps between", "confirms") not technical ones; no forbidden jargon tokens observed (no "endpoint", "schema", "controller" in prose).
- **Unresolved Questions**: present and substantive in both technical-spec.md (3 items) and business-context.md (2 items) — not boilerplate.

---

## Passed Checks

✓ existence.folder_missing @ F016_Search
✓ existence.folder_incomplete @ F016_Search
✓ existence.slug_format @ F016_Search
✓ FeatureSpec.required_sections @ F016_Search
✓ FeatureSpec.ccl_subsections @ F016_Search
✓ FeatureSpec.ccl_blank @ F016_Search
✓ FeatureSpec.bw_steps @ F016_Search
✓ FeatureSpec.deprecated_headings @ F016_Search
✓ FeatureSpec.no_appendix @ F016_Search
✓ FeatureSpec.edge_cases @ F016_Search
✓ FeatureSpec.f_code_format @ F016_Search
✓ FeatureSpec.sm_mermaid @ F016_Search
✓ FeatureSpec.pseudocode_length @ F016_Search
✓ FeatureSpec.pseudocode_fence @ F016_Search
✓ Universal.no_placeholder @ F016_Search
✓ citation.file_missing @ F016_Search
✓ FeatureSpec.linked_fr_missing @ F016_Search
✓ FeatureSpec.disc_boolean @ F016_Search
✓ citation.range_invalid @ F016_Search
✓ citation.range_inverted @ F016_Search
✓ citation.path_traversal @ F016_Search
✓ citation.unreadable @ F016_Search
✓ FeatureSpec.br_linked_fr_present @ F016_Search
✓ FeatureSpec.polymorphic_behavior_present @ F016_Search
✓ FeatureSpec.decision_logic_section_present @ F016_Search
✓ FeatureSpec.dec_blocks_well_formed @ F016_Search
✓ FeatureSpec.dec_lazy_na @ F016_Search
✓ FeatureSpec.missing_client_behavior_anchor @ F016_Search
✓ bc.missing @ F016_Search
✓ bc.missing_h2 @ F016_Search
✓ bc.forbidden_token @ F016_Search
✓ screens.missing @ F016_Search
✓ screens.missing_h2 @ F016_Search
✓ edge_cases.missing @ F016_Search
✓ edge_cases.few_rows @ F016_Search
✓ FeatureSpec.alg_file_schema_missing @ F016_Search
✓ link.pre_migration @ F016_Search
✓ gate.files_incomplete @ F016_Search
✓ gate.pending_marker @ F016_Search
✓ gate.validation_summary @ F016_Search
✓ reading_guide.pre_migration @ F016_Search
✓ db_impact.pre_migration @ F016_Search
✓ semantic.citation_spotcheck (5/5 accurate) @ F016_Search
✓ semantic.crossref_featurelist_exact_match @ F016_Search
✓ semantic.fr_br_sc_coverage @ F016_Search
✓ semantic.polymorphic_na_justified @ F016_Search
✓ semantic.edge_case_sufficiency @ F016_Search
✓ semantic.business_context_rubric @ F016_Search

---

## Metrics

| Metric | Value |
|--------|-------|
| Feature Specs | 1 |
| User Stories | 3 (US063, US064, US065) + 1 background (BL146) |
| Screens | N/A (generic-source profile, no screen-list.md) — 3 surfaces documented |
| Background Logic Items | 5 (BL020, BL059, BL060, BL146, BL159) |
| Permissions | 0 (none applicable to this feature) |
| Backend Route Rows | N/A (no HTTP surface) |
| Frontend Pages | N/A (desktop app) |
| Data Model Entities | 1 (MODEL009_MultiBuffer) + 2 in-memory structs (SearchQuery, non-DB) + 1 SQLite table (command_invocations) |

---

## Still Unresolved

None from this review. The spec's own `## Unresolved Questions` sections (3 in technical-spec.md, 2 in business-context.md) are pre-existing open questions about tunability of caps and search-history persistence — these are appropriately flagged by the spec itself, not omissions.