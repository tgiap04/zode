---
source_artifact: docs/features/F014_VimEmulation/technical-spec.md
claims_total: 13
claims_with_evidence: 13
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F014_VimEmulation/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section              | Evidence (file:line)                  | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | -------------------- | ------------------------------------- | ---------- |
| (enum + `Display`/`is_visual`/`is_helix`), `crates/vim/src/vim.rs:1181-1330` (`switch_mode` transit… | Polymorphic Behavior | crates/vim/src/state.rs:42-81         | ○          |
| , `crates/vim/src/vim.rs:551-553`                                                                    | Cross-Cutting Logic  | crates/editor/src/editor.rs:3665-3670 | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/vim/src/vim.rs:1181-1216       | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/vim/src/state.rs:417-478       | ○          |
| , `crates/vim/src/state.rs:42-81`                                                                    | Cross-Cutting Logic  | crates/vim/src/vim.rs:1181-1330       | ○          |
| (enum), `crates/vim/src/normal.rs:383-393` (`None => self.move_cursor(...)`), `crates/vim/src/motio… | User Stories         | crates/vim/src/motion.rs:46-176       | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/vim/src/normal.rs:383-393      | ○          |
| (`actions!` declarations), `crates/vim/src/vim.rs:1181-1216` (`switch_mode`)                         | User Stories         | crates/vim/src/normal.rs:36-58        | ○          |
| (`actions!` + enter logic), `crates/vim/src/vim.rs:1247-1277` (selection adjustment on mode switch)  | User Stories         | crates/vim/src/visual.rs:23-68        | ○          |
| (`command_interceptor`), `crates/vim/src/command.rs:1725-1741` (`GoToLine`/`YankCommand`/`WithRange… | User Stories         | crates/vim/src/command.rs:1845-1988   | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/vim/src/command.rs:1366-1374   | ○          |
| (`actions!` + `should_replay`/`repeatable_insert`)                                                   | User Stories         | crates/vim/src/normal/repeat.rs:1-59  | ○          |
| (`Object` enum), `crates/vim/src/object.rs:567-598` (`range`, `Word`/`Subword` inner-vs-around bran… | User Stories         | crates/vim/src/object.rs:21-45        | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
