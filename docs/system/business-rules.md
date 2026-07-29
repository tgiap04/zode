<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Business Rules (DRAFT)

> [!CAUTION]
> **STALE — do not treat this document as a description of the current code.**
> It was generated on 2026-07-26 against the pre-fork tree of 240 packages /
> 232 crates. The hard fork has since removed 54 crates and gutted several
> more; the workspace is now 186 packages / 178 crates.
>
> Anything here describing accounts, sign-in, collaboration, calls, channels,
> AI agents, LLM providers, edit prediction, auto-update or crash reporting is
> **fiction** — that code no longer exists. Feature codes F007, F008, F013,
> F019, F020, F021 and F022 in particular no longer have an implementation.
>
> Regeneration is deliberately deferred until the fork is green and verified
> (`/tkm:rebuild-spec` after phase 11). Running it against a half-cut tree
> would just produce a second stale document.


**Project**: Zed (zode)
**Generated**: 2026-07-26
**Status**: DRAFT — promoted from behavior-logic.md + data-model.md validation rules

> **Source note**: `behavior-logic.md` for this pass contains no `BR-###`-coded business rules
> (this is a `generic-source`, no-screen-list profile pass covering background/system logic
> only — `docs/system/business-rules.md draft deferred` marker in that file confirms no
> user-facing BR/DEC/SM/ALG material was extracted). The substantive product/system rules below
> are instead summarized from the BL### item descriptions in `behavior-logic.md` and the
> `Validation Rules` section of `data-model.md`, rewritten in plain language per instructions.
> NO PERM### or BL### codes appear below.

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

**Applies when:** A user or a remote collaborator attempts to edit an open file (or a combined
multi-file view built from one or more open files).
**Says:** Every open file carries its own editable/read-only status. Edits are silently rejected
(no user-facing error) when the file's status is not fully editable — this covers files
deliberately toggled to view-only as well as files a remote collaborator was never granted write
access to.
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

### Collaboration Role Gates Both Visibility and Editing

**Applies when:** A user joins someone else's shared coding session or channel.
**Says:** The role they're assigned determines two independent things — whether they can see the
shared project/channel content at all, and whether their edits are actually forwarded to the
host. A viewer-only participant's edit attempts are turned away before reaching the host, not just
hidden in their own UI.
**Source artifact:** [Permissions](permissions.md)

---

### Destructive Git Operations Are Host-Only Regardless of Role

**Applies when:** A collaborator (of any role) attempts a small set of destructive git-worktree
operations (removing/renaming a worktree, creating or restoring an archive checkpoint) inside a
shared project.
**Says:** These specific operations are always denied for non-hosting participants, independent of
whether their assigned role otherwise permits editing. This is a fixed safety rule layered on top
of the normal role system, not a configurable permission.
**Source artifact:** [Permissions](permissions.md)

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

- No feature-list or user-facing feature specs exist yet for this repo (`generic-source` profile,
  no `--feature-specs` pass run) — these rules are system-level, not tied to any F### feature.
- The 13 BL### items in `behavior-logic.md` are explicitly a representative sample of pattern
  categories in a 1.3M-LOC codebase, not an exhaustive enumeration — additional business rules
  likely exist in areas not sampled (LSP-specific behavior, per-vendor LLM integration quirks,
  debugger/DAP session rules) and were not surfaced by this pass.
- This draft should be reviewed against `docs/decisions/ADR-*.md` (human-authored) if/when those
  exist, since ADRs often encode the *why* behind a rule that source alone cannot show.
