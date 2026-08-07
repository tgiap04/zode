---
source_artifact: docs/features/F010_Debugging/technical-spec.md
claims_total: 22
claims_with_evidence: 21
confidence_derived: 0.9545
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F010_Debugging/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim | Section | Evidence (file:line) | Status ○/△ |
|---|---|---|---|
| (BL177_ForwardBreakpointToggleToRemote) | Polymorphic Behavior | crates/project/src/debugger/breakpoint_store.rs:553-565 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/project/src/debugger/breakpoint_store.rs:610-614 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/project/src/debugger/session.rs:2716-2744 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/debugger_ui/src/debugger_ui.rs:161-215 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/project/src/debugger/dap_store.rs:196-215 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/project/src/debugger/breakpoint_store.rs:553-565 | ○ |
| (enum definition), `session.rs:589-635` (transitions) | Cross-Cutting Logic | crates/project/src/debugger/session.rs:99-107 | ○ |
| , `crates/project/src/debugger/session.rs:3144-3170` | Cross-Cutting Logic | crates/project/src/debugger/dap_store.rs:196-215 | ○ |
| , `crates/dap/src/client.rs` | Cross-Cutting Logic | crates/project/src/debugger/session.rs:2716-2760 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/project/src/debugger/session.rs:3144-3170 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/debugger_ui/src/attach_modal.rs:360-395 | ○ |
| (unlabeled claim) | Cross-Cutting Logic | crates/project/src/debugger/breakpoint_store.rs:553-565 | ○ |
| (unlabeled claim) | User Stories | crates/debugger_ui/src/debugger_ui.rs:123-125 | ○ |
| (unlabeled claim) | User Stories | crates/debugger_ui/src/debugger_ui.rs:166-207 | ○ |
| (unlabeled claim) | User Stories | crates/debugger_ui/src/debugger_ui.rs:208-237 | ○ |
| (unlabeled claim) | User Stories | crates/debugger_ui/src/debugger_panel.rs:161-167 | ○ |
| (unlabeled claim) | User Stories | crates/debugger_ui/src/session/running/console.rs:273-306 | ○ |
| , 852-896` | User Stories | crates/debugger_ui/src/session/running/variable_list.rs:565-598 | ○ |
| , `crates/project/src/debugger/session.rs:2716-2744` | User Stories | crates/debugger_ui/src/session/running/variable_list.rs:898-917 | ○ |
| (unlabeled claim) | User Stories | crates/debugger_ui/src/attach_modal.rs:360-380 | ○ |
| , 954-972` | User Stories | crates/debugger_tools/src/dap_log.rs:151 | ○ |
| Bind session id to window id on serialization flush \| workspace DB (crates/db, per-workspace) \| ses… | DB Impact per Event | — | △ |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

- DB Impact per Event: Bind session id to window id on serialization flush \| workspace DB (crates/db, per-workspace) \| ses…

## Risk Flags

_(none)_
