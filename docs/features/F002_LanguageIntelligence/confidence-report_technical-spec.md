---
source_artifact: docs/features/F002_LanguageIntelligence/technical-spec.md
claims_total: 10
claims_with_evidence: 10
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F002_LanguageIntelligence/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section             | Evidence (file:line)                                          | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | ------------------- | ------------------------------------------------------------- | ---------- |
| , `crates/lsp/src/lsp.rs:61,1098-1105`, `crates/toolchain_selector/src/toolchain_selector.rs:909-95… | Cross-Cutting Logic | crates/project/src/lsp_store.rs:423-500                       | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/project/src/lsp_store.rs:448-486                       | ○          |
| ,1090-1110`                                                                                          | Cross-Cutting Logic | crates/lsp/src/lsp.rs:61                                      | ○          |
| , `crates/workspace/src/persistence.rs:2388-2423`                                                    | Cross-Cutting Logic | crates/toolchain_selector/src/toolchain_selector.rs:909-951   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/language_selector/src/language_selector.rs:209-227     | ○          |
| (full file; per `behavior-logic.md:2507-2532`, `BL111_VueLanguageServerTsServerBridge`)              | Cross-Cutting Logic | crates/project/src/lsp_store/vue_language_server_ext.rs:1-133 | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic | crates/project/src/prettier_store.rs:336-420                  | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/toolchain_selector/src/toolchain_selector.rs:909-951   | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/language_selector/src/language_selector.rs:209-227     | ○          |
| (unlabeled claim)                                                                                    | User Stories        | crates/project/src/lsp_store.rs:11717-11766                   | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
