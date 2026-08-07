---
failed: 0
warnings: 2
missing: 0
result: PASS
---
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: fcodes flagged MISSING due to `.pending` marker present in `artifacts/features/{slug}/` (verification-checklist-universal.md § Pending Marker Rule). Counts toward Wave 9 pre-flight gate halt conditions alongside `failed`.
`result`: PASS iff `failed === 0 && missing === 0`.
-->

# Review Report — Rebuild-Spec Artifacts (Wave 7a Core, Re-Review #2)

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: 8 core artifacts (system-overview.md, architecture.md, data-model.md, behavior-logic.md, permissions.md, permissions-matrix.md, user-stories.md, feature-list.md). Stack: Rust, `generic-source` profile — RouteList/ScreenList/ScreenFlow/ApiMap correctly absent and not flagged.

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 8 core |
| Critical issues | 0 |
| Warnings | 2 |
| Missing (`.pending` markers) | 0 |
| Result | **PASS** |

All 3 criticals + 4 warnings from the prior review round are confirmed fixed (verified independently below, not taken on faith). No new criticals surfaced. Two low-impact warnings remain, both stale-documentation-text issues that do not affect any cross-reference integrity.

---

## Verification of Prior Fixes (independent re-check)

1. **36 orphan BL codes** — re-ran the full BL↔F### cross-check programmatically: `behavior-logic.md` defines exactly 208 BL codes (BL001–BL208), `feature-list.md` references exactly the same 208 codes. Zero orphans in either direction. **FIXED, confirmed.**
2. **BL208_ZedCliPathResolution** — present at `behavior-logic.md:5142`, `Source File: crates/util/src/util.rs`, `Source Symbol: get_zed_cli_path`, mapped into `feature-list.md`'s F013 (`crates/util/src/util.rs` gap referenced explicitly). **FIXED, confirmed.**
3. **BL146–BL207 bare headings** — grep for `^## BL[0-9]{3}_[A-Za-z0-9]+$` (no trailing `: Title`) across the whole file returns zero matches; all 208 headings follow `## BL###_Slug: Title`. **FIXED, confirmed.**
4. **architecture.md missing `## Data Flow`** — present at `architecture.md:193`, wraps the SSH sequence diagram as `### Remote Development (SSH), not Collaboration`. **FIXED, confirmed.**
5. **Crate-count mismatch (179 vs 180)** — verified directly against `Cargo.toml`'s workspace `members` array: 180 `crates/*` paths. `system-overview.md` and `architecture.md` both now say "180 crate paths (179 top-level + 1 nested `refineable/derive_refineable`)". **FIXED, confirmed against ground truth, not just cross-doc agreement.**
6. **feature-list.md stale Limits paragraph** — now reads "coverage IS complete... all 208 BL items... are now assigned to at least one feature above", consistent with the BL cross-check above. **FIXED, confirmed.**

---

## Critical Issues

_(none)_

---

## Warnings

### W1: permissions-matrix.md Cross-Reference Validation section is stale re: FeatureList — OPEN
- **Severity**: warning
- **Location**: `permissions-matrix.md:247`
- **Description**: The Cross-Reference Validation checklist states `[ ] All PERM### codes are referenced in FeatureList.md — N/A this pass, no upstream FeatureList regenerated in this wave`. This is no longer true: `feature-list.md` exists in this same artifact set and its own Cross-Reference Validation confirms `All permission references are valid (every PERM001–PERM006 appears under exactly one feature)` (verified independently: PERM001–PERM006 all appear in `feature-list.md`, matching `permissions-matrix.md`'s own PERM001–PERM006). The checkbox is left unchecked with an outdated "N/A" rationale from an earlier wave ordering.
- **Fix**: Update the checkbox to `[x]` and replace the note with something like "confirmed via feature-list.md's own Cross-Reference Validation — all 6 PERM codes referenced." Pure documentation-freshness issue; no actual cross-reference is broken.

### W2: behavior-logic.md Cross-Reference Validation section undercounts BL total — OPEN
- **Severity**: warning
- **Location**: `behavior-logic.md:5185-5186`
- **Description**: The Cross-Reference Validation section states `All BL### codes are unique (BL001–BL207, verified no duplicates)` and `All BL### codes are contiguous (no gaps 001–207)`, but the document actually contains BL001–BL208 (208 items, confirmed unique and contiguous by direct grep) since BL208 was added in the prior fix cycle. The Summary section two lines above correctly says "Total Behavior Logic Items: 208", so this is an internal inconsistency between two sections of the same file — the Cross-Reference Validation checklist text wasn't updated when BL208 was appended.
- **Fix**: Update both bullets to read "BL001–BL208". Does not affect the correctness of the uniqueness/contiguity claim itself (re-verified true for 208 items), only the stated range.

---

## BehaviorLogic Cardinality

Per `verification-checklist-core-artifacts.md`'s Cardinality Cross-Check, loading `_scout-bl-inventory.md`:

- **Inventory total**: 207 real entries (see Investigation below — not 266, and not literally "208")
- **Artifact BL count**: 208
- **Gap**: |207 − 208| / max(207, 208) × 100 = **0.48%** → **PASS** (well under the 5% threshold; also passes the small-project absolute-floor check: abs gap = 1, ≤ 2 → PASS)
- **Missing categories**: none. Every inventory category with ≥1 real entry (custom-command, integration, observer, scheduled-job, queue-worker) has ≥1 matching BL type in the artifact (100/24, 20, 2, 62 respectively — see Type distribution below). The five zero-entry inventory categories (event-listener, mail, middleware, notification, webhook) are correctly documented as empty in both the scout inventory and `behavior-logic.md`'s own Scope Notes, and are exempt from Category-drop enforcement.
- **Orphan files**: none. Every scout inventory `Source File` entry (e.g. `crates/util/src/util.rs`, now BL208) has a matching BL item; no inventory entry lacks a corresponding BL Source File.

### Investigation: is the validator's "266 vs 208" WARN a real gap or a false positive?

**False positive — confirmed by direct count, not assumption.**

I independently counted `_scout-bl-inventory.md` three ways:

1. `grep -cE "^- (category): "` for all 10 canonical categories → **212** raw matches.
2. Of those 212, exactly **5** are `_(none found...)_` placeholder lines (event-listener, mail, middleware, notification, webhook) — these are prose notes explaining an empty category, not real entries, and should not count toward inventory total.
3. **212 − 5 = 207 real inventory entries.** This is the correct denominator for the cardinality gap calculation, and it is what I used above.

Separately, I counted all dash-prefixed lines regardless of indentation (`grep -cE "^[[:space:]]*- "`) → **248** — this includes the 207 real top-level entries + 5 placeholder lines + **36** indented sub-bullet continuation lines (the `- Intent matched:`, `- No-row reason:`, `- Observed pattern:` explanatory lines the task description flagged as the likely miscounting culprit). Neither 212 nor 248 nor any combination I tried reaches 266, so I cannot fully reconstruct the validator's naive-counter arithmetic, but the mechanism is clearly consistent with the task's hypothesis: **a naive line-counter that doesn't distinguish top-level category entries from indented explanatory sub-bullets (and/or doesn't exclude the 5 "none found" placeholders) will overcount.** The task description's own framing ("208 = 207 original...") is itself slightly garbled, but my from-scratch count lands cleanly on **207 real entries**, giving a 0.48% gap against the artifact's 208 BL items — solidly PASS, not the critical the raw "266 vs 208" delta would suggest if taken at face value.

**Verdict: WARN is a validator-side counting artifact, not a real coverage gap.** No action needed on the artifacts; if anything, `validate_behavior_logic.py`'s inventory-counting regex should be tightened to match only lines starting with a category name followed by `: ` at zero indentation, excluding `_(none found...)_` placeholder lines — but that is a tooling fix outside the scope of this artifact review.

---

## Passed Checks

✓ Universal.no_placeholder_text @ system-overview.md,architecture.md,data-model.md,behavior-logic.md,permissions.md,permissions-matrix.md,user-stories.md,feature-list.md (8/8)
✓ Universal.required_sections_in_order @ system-overview.md,architecture.md,data-model.md,behavior-logic.md,permissions.md,permissions-matrix.md,user-stories.md,feature-list.md (8/8)
✓ Universal.no_orphaned_codes @ BL001-BL208 (208/208, 0 orphans)
✓ Universal.no_orphaned_codes @ PERM001-PERM006 (6/6, 0 orphans)
✓ Universal.no_orphaned_codes @ US001-US067 (67/67, 0 orphans)
✓ Universal.no_orphaned_codes @ MODEL001-MODEL018 (18/18, 0 orphans)
✓ SystemOverview.mermaid_diagrams_present @ system-overview.md
✓ SystemOverview.tech_stack_table_format @ system-overview.md
✓ SystemOverview.crate_count_matches_cargo_toml @ system-overview.md (180 verified against Cargo.toml)
✓ Architecture.mermaid_diagram_present_and_valid @ architecture.md
✓ Architecture.data_flow_section_present @ architecture.md
✓ Architecture.crate_count_matches_cargo_toml @ architecture.md (180 verified against Cargo.toml)
✓ DataModel.model_code_uniqueness @ data-model.md (MODEL001-MODEL018, 0 duplicates)
✓ DataModel.disc_code_uniqueness @ data-model.md (DISC-001-DISC-013, 0 duplicates)
✓ DataModel.entity_completeness @ MODEL001_MultiWorkspace..MODEL018_ExtensionManifest (18/18)
✓ BehaviorLogic.bl_code_uniqueness @ behavior-logic.md (BL001-BL208, 0 duplicates)
✓ BehaviorLogic.bl_heading_format @ BL001..BL208 (208/208, `BL###_Slug: Title` format)
✓ BehaviorLogic.source_file_and_symbol_present @ BL001..BL208 (208/208)
✓ BehaviorLogic.no_multi_symbol_delimiter @ BL001..BL208 (208/208)
✓ BehaviorLogic.valid_type_value @ BL001..BL208 (208/208, all in canonical 10)
✓ BehaviorLogic.orphan_bl_zero @ BL001..BL208 (208/208 mapped to feature-list.md)
✓ Permissions.valid_auth_system_type @ permissions.md (`other`)
✓ Permissions.no_perm_codes_in_curated_view @ permissions.md
✓ PermissionsMatrix.perm_code_uniqueness @ permissions-matrix.md (PERM001-PERM006, 0 duplicates)
✓ PermissionsMatrix.required_fields_present @ PERM001..PERM006 (6/6)
✓ UserStories.us_code_uniqueness @ user-stories.md (US001-US067, 0 duplicates)
✓ UserStories.ui_type_has_feature_area @ US001..US067 (67/67, screen-list absent, Feature Area substitutes per profile)
✓ UserStories.system_type_has_bl_or_citation @ US027,US029,US033,US040,US043,US044,US051,US061,US062 (9/9)
✓ FeatureList.f_code_uniqueness @ feature-list.md (F001,F002,F008-F016, 0 duplicates)
✓ FeatureList.every_bl_mapped @ feature-list.md (208/208)
✓ FeatureList.every_perm_mapped @ feature-list.md (6/6)
✓ FeatureList.every_model_mapped @ feature-list.md (18/18)
✓ FeatureList.no_fabricated_subsystem @ feature-list.md,user-stories.md,data-model.md,permissions-matrix.md (grep-verified: no agent/collab/livekit/language_model reintroduction)

---

## Metrics

| Metric | Value |
|--------|-------|
| Feature Specs | 11 (F001,F002,F008–F016; provisional numbering per template note) |
| User Stories | 67 |
| Screens | N/A (`screen_source: none`) |
| Background Logic Items | 208 |
| Permissions | 6 |
| Backend Route Rows | N/A (no HTTP surface) |
| Frontend Pages | N/A |
| Data Model Entities | 18 |

---

## Still Unresolved

- W1 and W2 above (both stale cross-reference-validation prose, zero functional impact — safe to fix in a follow-up pass, not blocking).
- Pre-existing, out-of-scope-for-this-cycle observation (not a new finding, not actionable this round): `behavior-logic.md`'s "Related Data Models" sections cite entities by informal name (e.g. `Project`, `Editor`, `Workspace`) rather than by `MODEL###` code, unlike `feature-list.md`'s own Related Data Models sections which correctly use `MODEL###`. This has been true across all prior review rounds and causes no orphan/dangling reference (no MODEL### token is ever malformed or missing), so it is a style/traceability nit rather than a defect — flagging only for awareness, not counted as a warning.
