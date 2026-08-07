---
source_artifact: docs/features/F009_Diagnostics/technical-spec.md
claims_total: 18
claims_with_evidence: 18
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F009_Diagnostics/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim | Section | Evidence (file:line) | Status ○/△ |
|---|---|---|---|
| , `crates/diagnostics/src/diagnostics.rs:220-240`, `crates/diagnostics/src/diagnostics.rs:371-410`,… | Cross-Cutting Logic | crates/diagnostics/src/diagnostics.rs:143-178 | ○ |
| ,239,371-380` | Cross-Cutting Logic | crates/diagnostics/src/diagnostics.rs:93 | ○ |
| , `crates/project/src/lsp_store.rs:8046-8058` | Cross-Cutting Logic | crates/diagnostics/src/diagnostics.rs:143-178 | ○ |
| , `crates/project_panel/src/project_panel.rs:6296-6308`, `crates/project/src/lsp_store.rs:8060-8086` | Cross-Cutting Logic | crates/project_panel/src/project_panel.rs:1064-1102 | ○ |
| ,253-264,289-304,449-451` | Cross-Cutting Logic | crates/diagnostics/src/diagnostics.rs:65-67 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/diagnostics/src/diagnostics.rs:326-369 | ○ |
| ,608-628` | Cross-Cutting Logic | crates/activity_indicator/src/activity_indicator.rs:280-295 | ○ |
| , `crates/diagnostics/src/diagnostics.rs:257-264` | Cross-Cutting Logic | crates/project/src/project_settings.rs:342-349 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/activity_indicator/src/activity_indicator.rs:123-181 | ○ |
| (unlabeled claim) | User Stories | crates/diagnostics/src/buffer_diagnostics.rs:212-254 | ○ |
| (unlabeled claim) | User Stories | crates/diagnostics/src/buffer_diagnostics.rs:212-224 | ○ |
| (unlabeled claim) | User Stories | crates/diagnostics/src/diagnostics.rs:416-447 | ○ |
| (unlabeled claim) | User Stories | crates/activity_indicator/src/activity_indicator.rs:267-296 | ○ |
| (unlabeled claim) | User Stories | crates/activity_indicator/src/activity_indicator.rs:298-307 | ○ |
| (unlabeled claim) | User Stories | crates/activity_indicator/src/activity_indicator.rs:267-296 | ○ |
| (unlabeled claim) | User Stories | crates/etw_tracing/etw_tracing.rs:441-500 | ○ |
| (unlabeled claim) | User Stories | crates/etw_tracing/etw_tracing.rs:490-496 | ○ |
| (unlabeled claim) | User Stories | crates/system_specs/src/system_specs.rs:20-34 | ○ |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
