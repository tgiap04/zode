---
source_artifact: docs/features/F011_GitIntegration/technical-spec.md
claims_total: 18
claims_with_evidence: 18
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F011_GitIntegration/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section             | Evidence (file:line)                       | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | ------------------- | ------------------------------------------ | ---------- |
| , `crates/git_ui/src/git_panel.rs:1489-1946,2127-2369`, `crates/git_ui/src/branch_picker.rs:827-895… | Cross-Cutting Logic | crates/git/src/repository.rs:2205-2405     | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git_ui/src/git_panel.rs:2317-2335   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git_ui/src/git_panel.rs:2297-2304   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git_ui/src/git_panel.rs:2165-2197   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git_ui/src/commit_view.rs:520-599   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git/src/repository.rs:1956-1997     | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git_ui/src/git_panel.rs:1489-1535   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git_ui/src/git_panel.rs:2165-2197   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/git/src/repository.rs:2205-2405     | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/git/src/repository.rs:2205-2230     | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/git/src/repository.rs:2232-2259     | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/git/src/repository.rs:1956-1997     | ○          |
| , `crates/git/src/repository.rs:1999-2033`                                                           | User Stories        | crates/git_ui/src/branch_picker.rs:466-490 | ○          |
| , `crates/git/src/repository.rs:2261-2285`                                                           | User Stories        | crates/project/src/git_store.rs:5429-5470  | ○          |
| , `crates/git/src/repository.rs:1408-1435`                                                           | User Stories        | crates/git_ui/src/git_panel.rs:1489-1535   | ○          |
| , `crates/git/src/repository.rs:2362-2405`                                                           | User Stories        | crates/git_ui/src/git_panel.rs:2280-2369   | ○          |
| ,408`                                                                                                | User Stories        | crates/git_ui/src/project_diff.rs:63-140   | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/git_graph/src/git_graph.rs:993-1170 | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
