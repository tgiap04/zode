<!-- layout-exempt: plan-dir draft -->

# Design Intent

**Project**: Zode (Zed fork)

**Generated**: 2026-08-07

**Status**: EXPERIMENTAL — report-only (see disclaimer below)

<!-- disclaimer:start -->

> **⚠ EXPERIMENTAL — read before trusting anything below.**
>
> This document infers "why the system was built this way" — architecture choices, patterns,
> and trade-offs — from ADRs (when present), curated docs, and source-code patterns. This is
> the highest-hallucination-risk artifact this skill produces: "why" claims are inherently more
> inferential than the structural "what" claims in every other artifact.
>
> - Every claim below either cites its source (`ADR-###`, `business-rules.md`,
>   `architecture.md`, or a `file:line`) or is tagged **`[INFERRED]`** with a one-line reason.
>   An uncited, untagged assertion is a contract defect, not an acceptable shortcut.
> - This report is written to the **plan directory only** on first generation. It is
>   **NOT auto-promoted** to `docs/system/design-intent.md` — promotion happens only after a
>   human explicitly confirms the content is accurate and useful (see the pass's completion
>   handoff). Treat every reading of this file as a DRAFT until that confirmation has happened.
> - Graduation from EXPERIMENTAL to default-promote requires a pilot across 3 repos of
>   differing stacks with `[INFERRED]` ≤25%, zero fabricated citations, and human confirmation —
>   see `CHANGELOG.md` v26.1.0 sub-entry 3 for the full criteria. Until then, treat this
>   artifact as advisory, not authoritative.

<!-- disclaimer:end -->

**Non-duplication boundary**: this file holds ONLY cross-cutting **architectural rationale** —
the "why" behind a choice spanning multiple rules/entities/layers. It does not restate
`business-rules.md`'s per-rule As-Is/To-Be content or `architecture.md`'s structure/stack
description — it may cite either, never re-narrate them.

**Zero-ADR note (upfront)**: `docs/decisions/` does not exist in this repo (confirmed:
directory listing returns "No such file or directory"). No ADRs exist anywhere in the tree.
Every rationale below therefore comes from (a) git commit messages, which in this repo are
unusually detailed engineering-log entries functioning as de-facto decision records, (b) dense
in-source doc comments that already state the "why" explicitly (this codebase's own convention
for non-obvious logic, per its `CLAUDE.md`: "Comments should only be written... to explain
why"), (c) `docs/system/architecture.md` / `business-rules.md`, or (d) `[INFERRED]` tags.

---

## Architecture Choices

### Why SSH remote development instead of a collaboration server

Zode strips real-time collaboration entirely and keeps only SSH-based remote development.
Per `docs/system/architecture.md` § System Architecture: "There is no client/server split, no
collaboration backend, and no AI/agent subsystem in this fork — `client`, `rpc`, and `proto`
exist solely to support **remote development**... not real-time multiplayer collaboration."
The commit `c3e2ac3` ("refactor!: remove auth, collab, AI and cloud subsystems (54 crates)")
states the scope directly: "removes the Zode account, real-time collaboration, all AI and
edit-prediction features, in-app auto-update, and crash reporting... 372,180 lines deleted."
That same commit's body documents the removal was forced to be atomic by tooling, not by
design preference — "The removal is one commit because cargo makes it one: a stale path in
`[workspace.dependencies]` fails `cargo metadata` for the entire workspace before any per-crate
error can surface" — which is evidence about _how_ the fork was executed, not _why_ collab was
targeted for removal in the first place. The _why_ itself is not stated in any commit message
or doc found. [INFERRED] — no commit message or doc states the business motive; inferred from
the pattern that every removed subsystem (collab, auth, cloud API, AI/edit-prediction,
auto-update, crash reporting) requires either a hosted backend or a vendor account, and what
survives (`client`/`rpc`/`proto`/`remote`/`remote_connection`/`remote_server`) is exactly the
slice needed for a user's own SSH-reachable host — consistent with turning a
backend-dependent multiplayer product into a self-contained, single-user desktop tool with zero
required external services.

**One instance of that inference has since been falsified.** In-app auto-update was listed above
as requiring "a hosted backend or a vendor account"; it does not. It was restored in
`crates/auto_update` reading this repository's own published GitHub releases — no backend, no
account, no credential, and no identifier sent. The inference still holds for collab, auth, the
cloud API, AI/edit-prediction and crash reporting; it was simply wrong about this one, and the
distribution stance it justified (leave updates to package managers) was reversed deliberately.
See `plans/260824-1914-in-app-update-from-github-releases/`.

### Why a three-state hibernation model (Active/Warm/Hibernated) instead of open/closed

`docs/system/business-rules.md` § Hibernation Lifecycle states the mechanism: "Each open
project carries an activity label — Active..., Warm..., or Hibernated.... A project with an
active debug session or an in-flight autosave is not force-hibernated — the transition is
deferred and retried later instead, rather than forcing dirty work to be discarded." A binary
open/closed model cannot express this deferred-but-still-open middle state, nor "shrink but
don't discard" resource handling (terminal scrollback trimming, stale-but-visible diagnostic
counts) that the same section describes. The three-state design is directly driven by a
measured resource-pressure requirement, not aesthetic preference: `crates/workspace/src/multi_workspace.rs:239-243`
comments the "memory-pressure fuse" as "FR3 (Phase 6 of multi-project-window-switching): how
the memory-pressure fuse learns about system memory pressure" with NFR1 requiring "polling must
not cost anything noticeable" (`crates/workspace/src/multi_workspace.rs:269-270`). The
`MEMORY_FUSE_MIN_WARM_DURATION` constant's doc comment
(`crates/workspace/src/multi_workspace.rs:290-303`) states the FR4b rule verbatim: "a project
must have sat `Warm` for at least this long before the fuse may pick it as a victim — it must
never hibernate a project the user only just defocused." This is a named, numbered functional
requirement (FR3/FR4b) traceable in source, not an inferred design — the three states exist
because "Active, never a candidate" / "idle but recoverable without cost" / "torn down to
reclaim memory" are three genuinely different resource-liveness commitments the system has to
make and test independently.

### Why an always-visible project rail layered onto the panel

Commit `093f3e1` ("sidebar: add an always-visible project rail") states the motivating gap
directly: "The project switcher was reachable only through the 300px panel, which starts closed
and has no UI entry point at all — just the `cmd-alt-j` keybinding. A fixed 48px rail now sits
beside it and is always drawn." `crates/sidebar/src/rail.rs:71-74`'s doc comment confirms the
same rationale in source: "Unlike the panel, this is not gated on `MultiWorkspace::sidebar_open`
-- it is the primary way to switch projects, and the panel (with its filter input and project
names) is the secondary, on-demand view over the same data." `RAIL_WIDTH` is a fixed
`px(48.0)` constant (`crates/sidebar/src/rail.rs:10`), matching the commit's stated width. The
commit also explains a specific design constraint driven by that goal: "The rail lists every
project regardless of the panel's filter query. Filtering it too would strand a project with no
way to reach it while the panel is closed, so `rebuild_contents` keeps the unfiltered list
alongside the filtered one." This is a directly cited, first-person rationale from the change's
own author, corroborated by the surviving source comment and constant — not inferred.

### Why `WeakEntity<T>` for cross-entity back-references

`CLAUDE.md` (project root) states the rule generically: "Avoid `Rc`/`Arc` reference cycles — use
`Weak` for back-references (parent/child, observer/observed) so cycles don't keep memory alive
forever." The codebase applies this densely — 594 occurrences of `WeakEntity<` across 158 files
(`rg -o "WeakEntity<" -g "*.rs" crates/ | wc -l`; `rg -c "WeakEntity<" -g "*.rs" crates/ | wc -l`
on 2026-08-07), including `debugger_ui`'s session views holding `WeakEntity<Project>`/
`WeakEntity<Workspace>` (e.g. `crates/debugger_ui/src/debugger_panel.rs`,
`crates/project/src/connection_manager.rs`). `crates/workspace/src/multi_workspace.rs:352-364`
gives an explicit, non-generic instance of the reasoning at the call-site level: `warm_since`
holds a `WeakEntity<Project>` (not `Workspace`) "Deliberately a _project_ handle, not a
workspace one: nothing in `MultiWorkspace` keeps a defocused, never-independently-retained
workspace's `Entity<Workspace>` alive at all once `activate()` reassigns
`self.active_workspace` away from it... the shell can be dropped out from under this bookkeeping
while its `Project` lives on." This shows the choice of _which_ entity to hold weakly is itself
load-bearing engineering judgment, not boilerplate — a `WeakEntity<Workspace>` "would go stale
exactly when [the workspace shell drops]; the project itself does not"
(`crates/workspace/src/multi_workspace.rs:363-364`).

### Why extension capabilities are declared upfront and immutable at runtime, not prompted interactively

`docs/system/business-rules.md` § Extension System: "Extensions are WASM-sandboxed and cannot
perform three specific dangerous operations — running a process, downloading a file, or
installing an npm package — unless the extension's manifest explicitly pre-declared exactly
that capability... This allowlist is fixed when the extension is authored; there is no
user-facing runtime prompt to grant additional capabilities on the fly." The permissions matrix
(`docs/generated/permissions-matrix.md:44-46`, PERM001–PERM003) traces this to
`crates/extension/src/extension_manifest.rs:168` (`allow_exec`) and the mirrored `DownloadFile`/
`NpmInstallPackage` match arms; the runtime enforcement point is
`crates/extension_host/src/capability_granter.rs`'s `CapabilityGranter::grant_exec`, which
checks the manifest's static `allow_exec` AND the caller-supplied `granted_capabilities` list
before allowing the WASM guest to shell out. The design intent — sandbox at build-declaration
time rather than at run-time-prompt time — matches `docs/features/F012_ExtensionSystem/business-context.md`'s
framing: "letting someone else's code run on your machine is risky, so this feature exists to
give developers a fast way to install, reload, and iterate on extensions while also making sure
an extension can never do more than it explicitly said it would do up front." `docs/features/F012_ExtensionSystem/technical-spec.md:424`
flags that this static model may be an intentional trade-off or a genuine gap versus upstream
Zode's own history: "no UI surface was found for a user to grant/revoke
`granted_extension_capabilities` from inside the app... Confirm whether this is intentional or a
gap." [INFERRED] — no ADR/comment resolves this; treated here as an open question, not settled
rationale (carried into `## Open Questions` below).

### Why worktree trust is hierarchical and persistent rather than per-action

`docs/system/business-rules.md` § Workspace / Worktree Trust: "Opening a new folder or file does
not automatically permit Zode to spawn tooling... the folder starts untrusted... Trust is
hierarchical: trusting a single file is the narrowest grant, trusting a directory covers
everything inside it, and trusting a parent directory transitively trusts every subdirectory...
Once granted, trust persists across restarts so the same folder isn't re-prompted every
session." This is a single explicit trade-off documented in the curated doc: minimize prompt
fatigue (persistent, hierarchical grants) while still gating the two riskiest operation classes
(spawning language tooling, certain git operations) behind an explicit first-use decision. The
permissions matrix backs this with the enforcement sites: `crates/project/src/trusted_worktrees.rs`
(`TrustedWorktreesStore::can_trust`), consumed at `crates/project/src/lsp_store.rs:449` and
`crates/project/src/git_store.rs:1595` (`docs/generated/permissions-matrix.md:48`).

---

## Patterns & Trade-offs

### GPUI single-writer Entity discipline as the concurrency backbone

`docs/system/architecture.md` § Concurrency & Event Model: "Entity model: nearly every major
struct (`Editor`, `Project`, `Workspace`) is held as `Entity<T>`, mutated only through
`cx.update`/`cx.update_in` — enforced single-writer discipline (per project `CLAUDE.md`)." This
is a foundational trade-off: it forecloses fine-grained lock-based concurrency in favor of a
single foreground thread owning all mutation, pushing true parallelism out to
`cx.background_spawn` for CPU/IO work that hands results back through the same single-writer
gate. The cost (no true shared-mutable-state parallelism inside GPUI entities) is accepted in
exchange for eliminating an entire class of data-race and reentrant-borrow bugs by construction
— `CLAUDE.md`'s explicit warning that "Trying to update an entity while it's already being
updated must be avoided as this will cause a panic" shows this is enforced as a hard runtime
invariant, not a convention.

### Fire-and-forget background tasks as the default failure-handling shape

`docs/system/business-rules.md` § Background Workers / Fire-and-Forget Operations describes a
single repeated shape across unrelated features (workspace-item serialization, remote-server
round-trips, SQLite writes, debounced git-diff recalculation): "kick off the work in the
background, update UI state only when/if it completes, and log rather than propagate failures
for operations where blocking the user would be worse than a quietly-logged error." This is a
deliberate, named trade-off (responsiveness over guaranteed-visible failure) applied uniformly
enough across features that it reads as house style rather than a per-feature decision —
[INFERRED] — no single comment states this as a project-wide rule, but the business-rules.md
synthesis independently observed the same shape recurring across git, terminal, LSP-restart, and
persistence code paths, which is stronger evidence of house style than any single instance.

### Memory-pressure hibernation victim selection as a testable, injectable abstraction

`crates/workspace/src/multi_workspace.rs:239-247` defines `MemoryPressureReader` as a trait
specifically so victim-selection logic is "unit-testable without touching the real OS.
Production uses `SysinfoMemoryPressureReader`; tests inject their own — deliberately never read
`sysinfo` directly from the decision logic itself (phase-06's Implementation Steps, step 7)."
The hysteresis constant `MEMORY_FUSE_HYSTERESIS_CYCLES` (`crates/workspace/src/multi_workspace.rs:308-311`)
is commented as existing to "[prevent] the fuse from re-arming and flapping while memory
oscillates right at the boundary" — a control-systems trade-off (debounce vs. responsiveness)
applied to a UI/resource-management decision, evidence this hibernation fuse was designed with
the same rigor as a systems-level backpressure mechanism rather than an ad hoc timer.

---

## [INFERRED] Appendix

- [INFERRED] The business motive for stripping collab/auth/AI/cloud (turning a multiplayer,
  backend-dependent product into a self-contained single-user desktop tool) — no commit message
  or doc states the motive directly; inferred from what was removed vs. what was deliberately
  kept (SSH-only remoting) (see § Why SSH remote development instead of a collaboration server).
- [INFERRED] Whether the extension capability model's lack of a runtime grant/revoke UI is
  intentional design or an unaddressed gap versus upstream Zed — `technical-spec.md:424` itself
  flags this as unresolved; carried forward, not resolved, here (see § Why extension
  capabilities are declared upfront...).
- [INFERRED] Fire-and-forget background-task handling as intentional project-wide house style
  rather than coincidental per-feature convergence — inferred from the same shape recurring
  across git/terminal/LSP/persistence code, independently observed in business-rules.md's
  synthesis (see § Fire-and-forget background tasks as the default failure-handling shape).

---

## Open Questions

- **No ADRs exist in this repo** (`docs/decisions/` absent). Every architectural "why" above is
  reconstructed from commit messages and in-source doc comments, which happen to be unusually
  rich in this repo (multi-paragraph rationale is a house convention, not a given) — this
  legitimately raises the achievable evidence quality above a typical zero-ADR repo, but it is
  still not a substitute for a real ADR trail. A future contributor should not assume this
  quality of embedded rationale exists for every future change; nothing enforces the convention
  beyond current author habit.
- **Why collab/auth/AI was removed** (the business motive, not the mechanics) is not stated
  anywhere found — see the [INFERRED] tag above. If a non-technical driver exists (e.g. licensing,
  product-positioning, or maintenance-burden reasons), it is not recorded in this repository.
- **Extension capability UI gap** (`docs/features/F012_ExtensionSystem/technical-spec.md:424`):
  genuinely unresolved in the source docs themselves, not just in this pass — flagged there as a
  question for the maintainers, not answered.
- **Why WASM specifically, over an alternative extension sandboxing approach** (e.g. subprocess
  isolation, native code review, or no sandboxing) has no comment or ADR found justifying the
  choice of WASM itself, only how it's used once chosen. Zero-signal on this specific sub-question
  — not enough evidence to say anything beyond "WASM was chosen"; no fabricated narrative offered.
- **Why `sqlez`/hand-rolled SQLite wrapper over an existing crate like `sqlx` or `rusqlite`
  directly** — `docs/system/architecture.md`'s tech-stack table records the choice
  (`db`/`sqlez`) but gives no rationale, and no comment or commit message addressing the
  build-vs-adopt decision was found in this pass's scope. Zero-signal — not investigated deeply
  enough to claim more than the table records.

---

## Handoff Summary

- **ADR-citation ratio**: 0/13 claims cite an ADR — no `docs/decisions/ADR-*.md` files exist in
  this repository (confirmed by directory listing). The remaining 13 claims cite
  `business-rules.md`, `architecture.md`, a commit message (git log/show — treated as the
  highest-trust available primary source in this ADR-less repo), a `file:line`, or are
  `[INFERRED]`.
- **[INFERRED] ratio**: 3 of 13 total substantive claims (§ Architecture Choices: 6, § Patterns
  & Trade-offs: 3, plus the collab-removal-motive sub-claim inside § Why SSH remote development)
  are explicitly `[INFERRED]` — roughly 23%. Every other claim carries a direct
  `business-rules.md`/`architecture.md` section citation, a commit-message quote, or a
  `file:line` code citation.
- **Zero-signal note**: this repo has zero ADRs and no `docs/decisions/` directory at all — every
  rationale above had to be reconstructed from commit messages and source comments, which in
  this repo are unusually rationale-dense (several read like inline ADRs). Two topics
  (WASM-as-sandboxing-choice, `sqlez`-vs-alternatives) returned genuinely no signal beyond "this
  is what was chosen" and are recorded honestly as zero-signal in `## Open Questions` rather than
  padded with generic engineering-practice filler.
