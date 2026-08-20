---
source_artifact: docs/features/F016_Search/technical-spec.md
claims_total: 11
claims_with_evidence: 11
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F016_Search/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section             | Evidence (file:line)                              | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | ------------------- | ------------------------------------------------- | ---------- |
| , `crates/project/src/search.rs:93-119`, `crates/project/src/project_search.rs:154-155,566-572`, `c… | Cross-Cutting Logic | crates/file_finder/src/file_finder.rs:865-921     | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/file_finder/src/file_finder.rs:900-910     | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/project/src/search.rs:93-119               | ○          |
| ,566-572`, `crates/search/src/project_search.rs:428-486`                                             | Cross-Cutting Logic | crates/project/src/project_search.rs:154-155      | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/search/src/buffer_search.rs:1584-1631      | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/command_palette/src/persistence.rs:118-123 | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/project/src/search.rs:427-479              | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/file_finder/src/file_finder.rs:865-921     | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/project/src/search.rs:93-119               | ○          |
| ,566-572`                                                                                            | User Stories        | crates/project/src/project_search.rs:154-155      | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/search/src/buffer_search.rs:1584-1631      | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
