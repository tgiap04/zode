---
source_artifact: docs/features/F013_WorkspaceAndProjectManagement/technical-spec.md
claims_total: 23
claims_with_evidence: 23
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F013_WorkspaceAndProjectManagement/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                                                                                | Section              | Evidence (file:line)                                  | Status ○/△ |
| ---------------------------------------------------------------------------------------------------- | -------------------- | ----------------------------------------------------- | ---------- |
| (unlabeled claim)                                                                                    | Polymorphic Behavior | crates/workspace/src/workspace.rs:1422                | ○          |
| (enum), `:4740-4768` (`set_activity`), `:4788-4807` (`reconcile_resource_activity`)                  | Polymorphic Behavior | crates/project/src/project.rs:342-357                 | ○          |
| (actions!), `:1692-1737` (`schedule_hibernate`), `:283-310` (fuse constants)                         | Cross-Cutting Logic  | crates/workspace/src/multi_workspace.rs:37-55         | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/project/src/project.rs:4740-4768               | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/project/src/project.rs:4838-4919               | ○          |
| (unlabeled claim)                                                                                    | Cross-Cutting Logic  | crates/project/src/project.rs:4770-4807               | ○          |
| (`TrustedWorktreesStore::can_trust`), consumed at `crates/project/src/lsp_store.rs:449`, `crates/pr… | Cross-Cutting Logic  | crates/project/src/trusted_worktrees.rs:452           | ○          |
| (`build_type`, `validate_devcontainer_contents`)                                                     | Cross-Cutting Logic  | crates/dev_container/src/devcontainer_json.rs:269-303 | ○          |
| ,4740-4807`                                                                                          | Cross-Cutting Logic  | crates/project/src/project.rs:342-357                 | ○          |
| (`run`), `crates/dev_container/src/docker.rs:188-249`                                                | Cross-Cutting Logic  | crates/dev_container/src/devcontainer_json.rs:364     | ○          |
| (`poll_memory_fuse`/`select_memory_fuse_victim`), `:283-310` (constants)                             | Cross-Cutting Logic  | crates/workspace/src/multi_workspace.rs:1769-1850     | ○          |
| (`check_for_docker`), `crates/dev_container/src/docker.rs:188-249`                                   | Cross-Cutting Logic  | crates/dev_container/src/devcontainer_api.rs:295-300  | ○          |
| , `crates/recent_projects/src/recent_projects.rs:1139-1170`                                          | User Stories         | crates/workspace/src/welcome.rs:367-395               | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/project_panel/src/project_panel.rs:343-380     | ○          |
| , `:1835` (`confirm_edit`)                                                                           | User Stories         | crates/project_panel/src/project_panel.rs:2091-2094   | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/git_ui/src/worktree_picker.rs:305-344          | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/workspace/src/multi_workspace.rs:37-45         | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/workspace/src/multi_workspace.rs:1937-1967     | ○          |
| (`ProjectActivity::Hibernated`), `:4740` (`set_activity`), `crates/project/src/lsp_store.rs:11612`…  | User Stories         | crates/project/src/project.rs:355                     | ○          |
| (`wake_resources`), `crates/sidebar/src/project_item.rs:70-79` (hibernated-entry UI)                 | User Stories         | crates/project/src/project.rs:4958                    | ○          |
| , `crates/dev_container/src/devcontainer_json.rs:264-266` (`deserialize_devcontainer_json`)          | User Stories         | crates/dev_container/src/lib.rs:154-156               | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/dev_container/src/docker.rs:188-249            | ○          |
| (unlabeled claim)                                                                                    | User Stories         | crates/dev_container/src/devcontainer_json.rs:364     | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
