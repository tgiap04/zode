---
failed: 0
warnings: 2
missing: 0
result: PASS
---
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
