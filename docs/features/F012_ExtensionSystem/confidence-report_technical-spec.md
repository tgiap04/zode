---
source_artifact: docs/features/F012_ExtensionSystem/technical-spec.md
claims_total: 18
claims_with_evidence: 18
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F012_ExtensionSystem/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                             | Section              | Evidence (file:line)                                                     | Status ○/△ |
| ----------------------------------------------------------------- | -------------------- | ------------------------------------------------------------------------ | ---------- |
| (unlabeled claim)                                                 | Polymorphic Behavior | crates/extension/src/capabilities.rs:11-20                               | ○          |
| (unlabeled claim)                                                 | Cross-Cutting Logic  | crates/extension_host/src/capability_granter.rs:23-83                    | ○          |
| (unlabeled claim)                                                 | Cross-Cutting Logic  | crates/extension_host/src/capability_granter.rs:23-47                    | ○          |
| ,1043-1051,764-777`                                               | Cross-Cutting Logic  | crates/extension_host/src/wasm_host/wit/since_v0_8_0.rs:870-889          | ○          |
| (unlabeled claim)                                                 | Cross-Cutting Logic  | crates/extension_host/src/extension_host.rs:929-1012                     | ○          |
| (unlabeled claim)                                                 | Cross-Cutting Logic  | crates/project/src/context_server_store.rs:49-108                        | ○          |
| (unlabeled claim)                                                 | Cross-Cutting Logic  | crates/context_server/src/listener.rs:33-80                              | ○          |
| (module docs), `on_notification` registration                     | Cross-Cutting Logic  | crates/context_server/src/protocol.rs:118-124                            | ○          |
| ,403-418`                                                         | User Stories         | crates/extension_host/src/extension_host.rs:188-223                      | ○          |
| ,111-140`; `crates/extension_host/src/extension_host.rs:929-1028` | User Stories         | crates/extensions_ui/src/extensions_ui.rs:44-49                          | ○          |
| (unlabeled claim)                                                 | User Stories         | crates/extension_host/src/extension_host.rs:1030-1064                    | ○          |
| (unlabeled claim)                                                 | User Stories         | crates/project/src/context_server_store.rs:40-46                         | ○          |
| (unlabeled claim)                                                 | User Stories         | crates/context_server/src/listener.rs:33-80                              | ○          |
| (unlabeled claim)                                                 | User Stories         | crates/context_server/src/protocol.rs:118-124                            | ○          |
| ; `crates/extension_host/src/capability_granter.rs:23-47`         | User Stories         | crates/extension/src/extension_manifest.rs:168-187                       | ○          |
| ; `crates/extension_host/src/capability_granter.rs:49-65`         | User Stories         | crates/extension/src/capabilities/download_file_capability.rs:13-46      | ○          |
| ; `crates/extension_host/src/capability_granter.rs:67-83`         | User Stories         | crates/extension/src/capabilities/npm_install_package_capability.rs:9-13 | ○          |
| (unlabeled claim)                                                 | User Stories         | crates/extension_host/src/capability_granter.rs:23-83                    | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
