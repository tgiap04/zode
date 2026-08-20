---
source_artifact: docs/features/F008_EditorCore/technical-spec.md
claims_total: 17
claims_with_evidence: 16
confidence_derived: 0.9412
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F008_EditorCore/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section              | Evidence (file:line)                              | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | -------------------- | ------------------------------------------------- | ---------- |
| ; consumer gates verified at `crates/editor/src/editor.rs:3564-3577`.                                | Polymorphic Behavior | crates/language/src/buffer.rs:76-89               | ○          |
| (enum definition, 4 variants — see discrepancy noted in Unresolved Questions: data-model.md's DISC-… | Polymorphic Behavior | crates/editor/src/editor.rs:498-513               | ○          |
| (`SplittableEditor::toggle_split`); enum defined at `crates/settings_content/src/editor.rs:835-841`… | Polymorphic Behavior | crates/editor/src/split.rs:869-887                | ○          |
| ; `crates/editor/src/editor.rs:3564-3577`; `crates/editor/src/inlays/inlay_hints.rs:230-241`; `crat… | Cross-Cutting Logic  | crates/editor/src/actions.rs:1-90                 | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/language/src/buffer.rs:76-89               | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/editor/src/split.rs:869-887                | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/editor/src/items.rs:1346-1367              | ○          |
| (field declaration; enum at `crates/language/src/buffer.rs:174`; referenced in data-model.md MODEL0… | Cross-Cutting Logic  | crates/language/src/buffer.rs:120                 | ○          |
| , 2921-2932 (behavior-logic.md BL126 cross-reference)`                                               | Cross-Cutting Logic  | crates/editor/src/inlays/inlay_hints.rs:230-241   | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/editor/src/code_context_menus.rs:1156-1176 | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/editor/src/editor.rs:15476-15497           | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/editor/src/editor.rs:16616-16654           | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/editor/src/editor.rs:15522-15546           | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/editor/src/editor.rs:3564-3577             | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/editor/src/editor.rs:15522-15546           | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/editor/src/split.rs:869-887                | ○          |
| Fold state change for file-backed buffer (BL155/items.rs serialize) \| `editor_folds`, `file_folds`… | DB Impact per Event  | —                                                 | △          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

- DB Impact per Event: Fold state change for file-backed buffer (BL155/items.rs serialize) \| `editor_folds`, `file_folds`…

## Risk Flags

_(none)_
