---
source_artifact: docs/features/F015_SettingsAndKeymaps/technical-spec.md
claims_total: 14
claims_with_evidence: 14
confidence_derived: 1.0
generated_by: derive_confidence_report.py
---

# Confidence Report -- docs/features/F015_SettingsAndKeymaps/technical-spec.md

> **Self-reported citation-coverage stat -- NOT a correctness verification.** This report is derived deterministically by parsing the artifact's own inline `**Source:** file:line` citations and `[UNVERIFIED]`/`[INFERRED]`/`[NEEDS_DOMAIN_CONFIRMATION]` marker tags. It does NOT verify that citations are accurate or that claims are true. For blind truth verification, see `claude/skills/audit-doc-parity/`.

## Claims ↔ Evidence

Legend: `○` = cited (Source file:line present) · `△` = marker-tagged (uncertain, no citation).

| Claim                                               | Section              | Evidence (file:line)                                | Status ○/△ |
| --------------------------------------------------- | -------------------- | --------------------------------------------------- | ---------- |
| (struct), discriminator `Ord`/`SettingsFile`        | Polymorphic Behavior | crates/settings/src/settings_store.rs:145           | ○          |
| (`Appearance` enum).                                | Polymorphic Behavior | crates/theme/src/theme.rs:54-59                     | ○          |
| (FR-001/002), `crates/zed/src/zed.rs`               | Cross-Cutting Logic  | crates/settings/src/settings_store.rs:350-401       | ○          |
| (`merged_settings` field), `data-model.md` DISC-010 | Cross-Cutting Logic  | crates/settings/src/settings_store.rs:432-441       | ○          |
| (`KeymapFile::update_keybinding`)                   | Cross-Cutting Logic  | crates/settings/src/keymap_file.rs:867-901          | ○          |
| (`write_keymap_migration`), `:308-324`              | Cross-Cutting Logic  | crates/zed/src/zed/migrate.rs:283-299               | ○          |
| (`FeatureFlag` trait defaults),                     | Cross-Cutting Logic  | crates/feature_flags/src/feature_flags.rs:114-132   | ○          |
| (`update_settings_file_inner`)                      | Cross-Cutting Logic  | crates/settings/src/settings_store.rs:552-607       | ○          |
| (`watch_config_file`, BL137),                       | Cross-Cutting Logic  | crates/settings/src/settings_file.rs:165            | ○          |
| (unlabeled claim)                                   | User Stories         | crates/settings/src/settings_store.rs:552-635       | ○          |
| (unlabeled claim)                                   | User Stories         | crates/settings/src/settings_store.rs:350-401       | ○          |
| (unlabeled claim)                                   | User Stories         | crates/onboarding/src/base_keymap_picker.rs:174-199 | ○          |
| (unlabeled claim)                                   | User Stories         | crates/zed/src/zed/migrate.rs:278-326               | ○          |
| , 313-324`                                          | User Stories         | crates/zed/src/zed/migrate.rs:288-299               | ○          |

## Missing Info

Candidate sections to check for `△` (marker-tagged) claims -- best-effort only, not authoritative:

_(none -- no marker-tagged claims)_

## Risk Flags

_(none)_
