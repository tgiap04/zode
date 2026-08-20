---
source_artifact: docs/features/F001_Terminal/technical-spec.md
claims_total: 11
claims_with_evidence: 11
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F001_Terminal/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section              | Evidence (file:line)                                | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | -------------------- | --------------------------------------------------- | ---------- |
| (enum definition), `:2178-2187` (`client_side_working_directory` match)                              | Polymorphic Behavior | crates/terminal/src/terminal.rs:846-852             | ○          |
| (BL118), `crates/util/src/shell_env.rs` (BL122), `crates/util/src/process.rs` (BL120)                | Cross-Cutting Logic  | crates/util/src/command.rs:1-40                     | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/terminal/src/terminal.rs:1328-1335           | ○          |
| (`Project::create_terminal_task`), `crates/project/src/terminals.rs:318-457` (`create_terminal_shel… | Cross-Cutting Logic  | crates/project/src/terminals.rs:63-160              | ○          |
| (`TerminalView::serialize`)                                                                          | Cross-Cutting Logic  | crates/terminal_view/src/terminal_view.rs:1724-1756 | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/terminal/src/terminal.rs:918-928             | ○          |
| (dispatch), `crates/util/src/command/darwin.rs` (macOS `posix_spawnp` path)                          | Cross-Cutting Logic  | crates/util/src/command.rs:1-40                     | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/project/src/terminals.rs:318-457             | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/terminal_view/src/terminal_panel.rs:45-72    | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/project/src/terminals.rs:63-160              | ○          |
| , `crates/terminal_view/src/terminal_view.rs:1821-1904`                                              | User Stories         | crates/terminal/src/terminal.rs:2148-2159           | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
