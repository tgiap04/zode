<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Business Rules (DRAFT)

**Rewritten 2026-08-04** against the post-fork tree (187 packages / 178 crates). This fork
removed authentication, cloud/collaboration, AI/agent, edit-prediction, auto-update, and
telemetry/crash-reporting. Two rules present in the original draft — "Collaboration Role Gates
Both Visibility and Editing" and "Destructive Git Operations Are Host-Only Regardless of Role" —
described the now-deleted real-time collaboration system and have been removed rather than
rewritten, since no collaboration session of any kind exists in this fork.

**Project**: Zode
**Status**: DRAFT — promoted from behavior-logic.md + data-model.md validation rules

### Extension Sandbox Capability Allowlisting

**Applies when:** A third-party extension attempts to execute a process, download a file, or
install an npm package.
**Says:** The action is only permitted if the extension's manifest explicitly declared a matching
capability grant (exact command+args, exact host, or exact package). Anything not pre-declared is
rejected before it runs — there is no runtime prompt or override; the allowlist is fixed at
extension-authoring time.
**Source artifact:** [Permissions](permissions.md)

---

### Buffer Read-Only Enforcement

**Applies when:** A user attempts to edit an open file (or a combined multi-file view built from
one or more open files).
**Says:** Every open file carries its own editable/read-only status (e.g. from the
`read_only_files` setting). Edits are silently rejected (no user-facing error) when the file's
status is not fully editable.
**Source artifact:** [Permissions](permissions.md)

---

### Settings Precedence Ordering

**Applies when:** The same setting is defined in more than one settings file (project-level,
server-provided, user/global, or built-in default).
**Says:** Project-level settings win over server-provided settings, which win over user/global
settings, which win over built-in defaults. Whenever any settings file changes, the system
re-parses and re-applies this precedence order automatically; parse errors are tracked per file
rather than failing the whole settings load.
**Source artifact:** [Architecture](../system/architecture.md)

---

### File Conflict Detection on External Changes

**Applies when:** A file open in the editor changes on disk (e.g. edited by another program)
without a corresponding save from within the editor.
**Says:** The system flags the open file as having a conflict and surfaces a notice to the user
("This file has changed on disk") rather than silently overwriting either version.
**Source artifact:** [Data Model](../generated/entities.md)

---

### Extension Manifest Versioning Determines Parse Path

**Applies when:** An extension's `extension.toml` manifest is loaded.
**Says:** The manifest's schema version determines which parser/migration path is applied —
older-format manifests are read through a legacy compatibility path rather than being rejected
outright, so previously-published extensions keep working after a manifest schema change.
**Source artifact:** [Data Model](../generated/entities.md)

---

## Limits / What This Draft Does Not Cover

- No feature-list or user-facing feature specs exist for this repo (`generic-source` profile, no
  `--feature-specs` pass run) — these rules are system-level, not tied to any F### feature.
- This is a representative sample of pattern categories, not an exhaustive enumeration —
  additional business rules likely exist in areas not sampled (LSP-specific behavior, debugger/DAP
  session rules, extension registry interaction) and were not surfaced by this pass.
- This draft should be reviewed against `docs/decisions/ADR-*.md` (human-authored) if/when those
  exist, since ADRs often encode the *why* behind a rule that source alone cannot show.
