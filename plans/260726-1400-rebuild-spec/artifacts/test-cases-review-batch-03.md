---
failed: 0
warnings: 1
missing: 0
result: PASS
---
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: fcodes flagged MISSING due to `.pending` marker present in `artifacts/features/{slug}/`.
`result`: PASS iff `failed === 0 && missing === 0`.
-->

# Review Report — Rebuild-Spec Test Cases (TC.3 semantic pass)

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: F016_Search / test-cases.md (17 rows: TC001-TC017)

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 1 feature (F016_Search test-cases.md) |
| Critical issues | 0 |
| Warnings | 1 |
| Missing (`.pending` markers) | 0 |
| Result | **PASS** |

TC.2 `validate_test_cases.py` passed clean (0 critical, 0 warnings) — TC### format/uniqueness,
Type enum, Traces-to presence, citation-source-family match, and coverage-gap are all
`[deterministic-pass]` and were not re-checked here. Semantic review (TC-S1..TC-S6) applied.

---

## Critical Issues

_(none)_

---

## Warnings

### W1: BR rule prose reused near-verbatim in Then column (TC-S6, DRY) — OPEN
- **Severity**: warning
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/features/F016_Search/test-cases.md:24,25,26`
- **Description**: TC003, TC004, and TC005's `Then` clauses closely mirror the clause structure
  and much of the wording of their cited `BR-###` rule sentences in
  `docs/features/F016_Search/technical-spec.md`, rather than being reshaped into an independent
  concrete scenario description:
  - TC003 Then: `"ProjectSearchView sets limit_reached = true and renders the match counter as
    "{index}/{count}+""` vs BR-003's rule text: `"The view sets limit_reached = true and renders
    the match counter as "{index}/{count}+" (trailing +) instead of the exact count."` (technical-spec.md:88)
    — the clause `sets limit_reached = true and renders the match counter as "{index}/{count}+"`
    is copied essentially word-for-word.
  - TC004 Then: `"sets query_error to the compile-error string, clears the active searchable
    item's highlighted matches, and returns without running a search or crashing"` vs BR-004:
    `"records the error string in self.query_error, clears any existing highlighted matches on
    the active searchable item, and returns without running a search — it never falls back to a
    text search or crashes."` (technical-spec.md:106) — same three clauses, same order, largely
    the same words.
  - TC005 Then: `"inserts the new row (command_name, user_query, last_invoked=unixepoch()) then
    deletes the row(s) with the minimum id, keeping the table capped near 1,000 rows"` vs
    BR-005: `"inserts one row into command_invocations (command_name, user_query, default
    last_invoked = unixepoch()), then immediately deletes the oldest row(s) whenever more than
    1,000 rows exist in total, keeping the table capped near 1,000 entries."` (technical-spec.md:124)
    — same structure, same field list, same closing clause almost verbatim.
  - Milder instances of the same pattern appear in TC001 (`"no result beyond rank 100 is ever
    computed"` vs BR-001's `"results beyond that rank are never computed/returned"`) and TC002
    (`"transparently re-dispatched to SearchQuery::regex"` copied directly from BR-002's
    `"transparently re-dispatches to SearchQuery::regex"`).
  - This is a DRY concern per TC-S6, not a citation-accuracy problem — none of these rows
    contradict or embellish the cited rule (TC-S1 holds), so this is warning-level, not critical.
- **Fix**: Reshape the `Then` cells for TC003/TC004/TC005 (and lightly for TC001/TC002) into
  concrete, scenario-specific outcomes phrased in the tester's own words — e.g. TC005 could read
  "the table still holds exactly 1,000 rows afterward, with the oldest entry gone" instead of
  restating the INSERT/DELETE mechanics already given verbatim in BR-005.

---

## Passed Checks

✓ TC-S1 @ F016_Search (TC001, TC002, TC003, TC004, TC006 checked against BR-001/002/003/004/ALG-001 — outcomes match cited behavior, no contradiction/embellishment)
✓ TC-S2 @ F016_Search (TC012-TC017 UAT rows all cite genuine screens.md § User Journey steps 1-5 or business-context.md § What They Do item 6 — no code-dressed-as-section citation found)
✓ TC-S3 @ F016_Search (TC007-TC017 are feature-specific edge/UAT coverage, not generic padding; Coverage Notes confirms no coverage-gap existed to paper over)
✓ TC-S4 @ F016_Search (TC007-TC011 edge-cases.md citations match the cited row's actual Scenario/What-Happens content)
✓ TC-S5 @ F016_Search (Coverage Notes rationale is specific and real — names why SM/DEC/DISC families are N/A rather than using a placeholder)
✓ [deterministic-pass] TC###-format/uniqueness, Type-enum, Traces-to-presence, citation-source-family-match, coverage-gap @ F016_Search (TC.2 validate_test_cases.py, 0 critical/0 warnings)

---

## Metrics

| Metric | Value |
|--------|-------|
| Feature Specs | 1 (F016_Search) |
| Test Cases | 17 (TC001-TC017: 6 UT, 5 IT, 6 UAT) |
| Coverage Notes entries | 0 (all BR/ALG codes traced; SM/DEC/DISC N/A) |
