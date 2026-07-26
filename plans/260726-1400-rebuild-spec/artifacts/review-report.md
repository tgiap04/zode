---
failed: 0
warnings: 5
missing: 0
result: PASS
---
<!--
`failed`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`missing`: fcodes flagged MISSING due to `.pending` marker present in `artifacts/features/{slug}/` (verification-checklist-universal.md § Pending Marker Rule). Counts toward Wave 9 pre-flight gate halt conditions alongside `failed`.
`result`: PASS iff `failed === 0 && missing === 0`.
-->

# Review Report — Rebuild-Spec Artifacts (Wave 7a, Core) — FINAL RE-CHECK #2

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-07-26
**Scope**: 8 core artifacts (system-overview.md, architecture.md, data-model.md, behavior-logic.md, permissions.md, permissions-matrix.md, user-stories.md, feature-list.md). RouteList/ScreenList/ScreenFlow/ApiContracts/FeatureSpec/ProcessFlow SKIPPED per task scope (generic-source, screen_source:none, no --flows/--api-contracts/--feature-specs pass run). Full independent pass over all 8 artifacts, not just the 3 previously-patched spots.

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 8 core |
| Critical issues | 0 |
| Warnings | 5 |
| Result | **PASS** |

**All 3 previously-tracked critical issues are confirmed FIXED, verified independently:**

- **C1 (BL012 orphan)** — `feature-list.md:86` now cites `BL012_StandaloneCliBinaries` under F001_ProjectFolderNavigation's Related Background Logic (`zed <path>` CLI entry point). Grep of all 13 BL### codes across feature-list.md confirms every code (BL001–BL013) now has ≥1 citation. `feature-list.md:749` ("all referenced across features") and `:761` are now truthful.
- **C2 (ProjectPanel orphan)** — `feature-list.md:82` now lists `ProjectPanel` under F001's Related Data Models (also present at `feature-list.md:748` Total Data Models summary). Cross-checked all 16 data-model.md top-level entity headings (`data-model.md`: Workspace, Project, Worktree, Entry, TextBuffer, Buffer, MultiBuffer, Editor, Thread, Message, SettingsStore, Theme/ThemeFamily, GitStore/Repository, ProjectPanel, Terminal, ExtensionManifest = 16) against feature-list.md citations — all 16 headings (18 entity names, Theme/ThemeFamily and GitStore/Repository each counted as 2) have ≥1 non-summary-line citation.
- **C3 (BehaviorLogic Index table misalignment)** — `behavior-logic.md:39-53` Index table now correctly populates all 4 columns (Code | Name | Type | Trigger) for all 13 BL### rows, matching the per-BL### detail sections below.

**This independent re-pass found zero new critical issues.** One candidate false-positive was investigated and ruled out: system-overview.md appears to be missing the checklist's literal "## System Architecture / ### High-Level Architecture (Mermaid) / ### Technology Stack / ## Data Flow" sections — but cross-checking against the actual `system-overview-template.md` (not the possibly-stale checklist prose) confirms this project's template explicitly delegates diagrams/tech-stack to architecture.md ("For architecture diagrams and tech stack details, see architecture.md") and requires only Executive Summary → Key Design Decisions → Security Overview → Scalability, which system-overview.md has in full. architecture.md independently carries all 3 required Mermaid diagrams (graph TB, graph LR, sequenceDiagram) and a Layer|Technology|Version tech-stack table. Not a defect.

---

## Critical Issues

_(none)_

---

## Warnings

All 5 previously-known warnings were re-checked in this pass and reconfirmed as still open — stale cross-reference-validation checkbox prose only, no impact on underlying data correctness.

### W1: behavior-logic.md Cross-Reference Validation contains stale claims — OPEN (unchanged)
- **Severity**: warning
- **Location**: `behavior-logic.md:354-355`
- **Description**: Still states "N/A this pass, no upstream US artifact generated" / "N/A this pass, no upstream FeatureList generated." Both user-stories.md and feature-list.md exist; feature-list.md now cites all 13/13 BL### codes (C1 fixed).
- **Fix**: Update both checkboxes to `[x]` now that C1 is resolved (all 13 BL### cited in feature-list.md).

### W2: user-stories.md Cross-Reference Validation / Unresolved Question #1 claims no upstream FeatureList exists — OPEN (unchanged)
- **Severity**: warning
- **Location**: `user-stories.md:1182`, `user-stories.md:1188`
- **Description**: Still states "N/A this pass; no upstream FeatureList generated in this session." feature-list.md's US###→F### Coverage Matrix (`feature-list.md:767-795`) correctly maps all 27 US### codes, each exactly once.
- **Fix**: Update checkbox to checked; resolve/remove Unresolved Question #1.

### W3: permissions-matrix.md Cross-Reference Validation cites a stale reason for its unchecked PERM→FeatureList box — OPEN (unchanged)
- **Severity**: warning
- **Location**: `permissions-matrix.md:225`
- **Description**: Still unchecked with reason "N/A this pass, no upstream FeatureList generated." feature-list.md exists and correctly cites all 8 PERM### codes (verified — no PERM orphans).
- **Fix**: Check the box; update the stated reason to reflect full PERM### coverage confirmed in feature-list.md.

### W4: feature-list.md — residual stale-prose risk around the now-fixed C1/C2/C3 area — OPEN (informational, confirmed non-critical)
- **Severity**: warning
- **Location**: `feature-list.md:24-27`, `feature-list.md:748-750`, `feature-list.md:799-800`
- **Description**: The previously-critical C1/C2/C3 are confirmed fixed in substance, and re-reading these specific lines in this pass found no remaining false statement. Flagged only for continued extra scrutiny in future edit passes since this is exactly the aggregate-claim text that produced the prior defects.
- **Fix**: No action required beyond normal proofreading; not blocking.

### W5: Workspace crate-count "~241" vs. root Cargo.toml member count — OPEN (unchanged, confirmed non-critical)
- **Severity**: warning
- **Location**: `system-overview.md:7`, `architecture.md:4,11,145,196`
- **Description**: system-overview.md/architecture.md cite "~241"/"241 workspace members" and separately "~230 in-workspace library crates" (subset statically linked into the `zed` binary) and "232 under crates/" (a different subset). All figures are hedged ("~") and each is individually well-scoped, so this is phrasing-precision noise, not a raw contradiction.
- **Fix**: Optional — recount and cite one single, precisely-labeled aggregate figure if exactness is desired; not blocking.

---

## Passed Checks

✓ SystemOverview.required_sections @ system-overview.md (verified against system-overview-template.md, not checklist prose)
✓ SystemOverview.key_design_decisions_min2 @ system-overview.md
✓ Architecture.mermaid_diagrams_valid @ architecture.md (3 diagrams: graph TB, graph LR, sequenceDiagram)
✓ Architecture.tech_stack_matches_manifests @ architecture.md
✓ DataModel.erd_present @ data-model.md
✓ DataModel.required_sections_order @ data-model.md
✓ DataModel.discriminator_sequential_no_gaps (DISC-001..DISC-017) @ data-model.md
✓ DataModel.disc_scope_min2_values_no_boolean_only @ data-model.md
✓ DataModel.no_duplicate_discriminators @ data-model.md
✓ BehaviorLogic.bl_codes_unique_and_format_valid (BL001..BL013) @ behavior-logic.md
✓ BehaviorLogic.index_table_4col_format_correct (FIXED, re-verified) @ behavior-logic.md
✓ BehaviorLogic.per_item_required_fields_present @ behavior-logic.md
✓ BehaviorLogic.cardinality_total_gap (13 inventory vs 13 BL = 0% = PASS) @ behavior-logic.md
✓ BehaviorLogic.bl_orphan_check (FIXED: 0 orphans, all 13 referenced in feature-list.md) @ behavior-logic.md
✓ Permissions.required_sections_order @ permissions.md
✓ Permissions.valid_auth_system_type (hybrid) @ permissions.md
✓ Permissions.no_perm_codes_in_curated_view @ permissions.md
✓ PermissionsMatrix.perm_codes_unique_and_format_valid (PERM001..PERM008) @ permissions-matrix.md
✓ PermissionsMatrix.index_table_4col_format_correct @ permissions-matrix.md
✓ PermissionsMatrix.perm_orphan_check (0 orphans, all 8 referenced in feature-list.md) @ permissions-matrix.md
✓ UserStories.us_codes_unique_and_format_valid (US001..US027) @ user-stories.md
✓ UserStories.single_intent @ user-stories.md
✓ UserStories.human_actor @ user-stories.md
✓ UserStories.system_us_has_bl_mapped (US020-024) @ user-stories.md
✓ FeatureList.f_codes_unique_and_format_valid (F001..F023) @ feature-list.md
✓ FeatureList.us_to_f_mapping_complete (US001-027, all mapped exactly once) @ feature-list.md
✓ FeatureList.perm_codes_all_mapped (PERM001-008, re-verified, 0 orphans) @ feature-list.md
✓ FeatureList.data_model_orphan_check (FIXED: 0 orphans, all 16 entity headings / 18 names referenced) @ feature-list.md
✓ FeatureList.no_duplicate_feature_names @ feature-list.md
✓ FeatureList.c1_c2_c3_fix_verified (BL012 cited, ProjectPanel cited, Index table 4-col rebuilt) @ feature-list.md + behavior-logic.md

---

## Metrics

| Metric | Value |
|--------|-------|
| Feature Specs | 0 (not in scope this pass) |
| User Stories | 27 (US001–US027) |
| Screens | N/A (screen_source:none, not produced) |
| Background Logic Items | 13 (BL001–BL013; 0 orphaned) |
| Permissions | 8 (PERM001–PERM008; 0 orphaned) |
| Backend Route Rows | 0 (no HTTP routes; not produced) |
| Frontend Pages | N/A (not produced) |
| Data Model Entities | 16 headings / 18 names (0 orphaned) |

---

## Unresolved Questions

1. W1-W3 (stale "N/A this pass, no upstream X generated" checkbox prose in behavior-logic.md, user-stories.md, permissions-matrix.md) are now factually stale given feature-list.md exists and fully cross-references all of them — batchable into one proofreading sweep, not blocking Wave 9 promotion.
2. W5 (crate-count phrasing precision) is optional polish, not blocking.
3. No blocking issues remain. Clean PASS — ready for Wave 9 promotion as far as this core-artifact scope is concerned.
