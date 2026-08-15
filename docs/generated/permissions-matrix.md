<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Permissions Matrix

**Project**: Zode (Zed fork)
**Generated**: 2026-08-07
**Analysis Scope**: Full monorepo, `generic-source` profile, no screen-list upstream. Zode is a
native Rust/GPUI desktop editor with no HTTP surface and — per this fork's architecture — no
collaboration/multiplayer server. There is no traditional web-style RBAC (no admin/user/manager
application roles). What this codebase calls "permissions" reduces to three independent
capability/trust gates: (a) the extension WASM sandbox capability allowlist, (b) the buffer
read/write capability, and (c) the local worktree-trust boundary that gates whether a
project/folder is allowed to spawn language servers and other tooling.

> **Corrects a stale prior pass.** An earlier draft of this file (2026-07-26) fabricated PERM
> items around `crates/collab`'s `ChannelRole`/RPC access-project guards (see retraction note below).
> `crates/collab` does not exist in this repository (verified: `ls crates` has no `collab`
> directory; only leftover deploy scripts — `Dockerfile-collab`, `script/deploy-collab` — remain
> from before the collab_ui removal noted in this fork's git history, e.g. commit `77bdc49`
> "restore the title bar lost with the collab_ui removal"). `proto::ChannelRole` still exists in
> `crates/proto/proto/channel.proto` and is referenced once in `crates/project/src/project.rs`
> (`Project::set_role`), but with no collab server crate and no `call` crate in this workspace
> (`ls crates` confirms no `call` directory either), it is a vestigial protocol type, not a live
> permission system — it is NOT included as a PERM item below. This pass replaces the fabricated
> collab items with the real local-trust boundary (`trusted_worktrees.rs`) that the prior pass
> missed entirely.

**Code Format**: All codes follow `PERM###_NameSlug` format.

**Permission Types**: `route-guard`, `screen-permission`, `action-permission`, `data-permission`,
`role-based`, `resource-ownership`, `field-permission`, `api-scope`, `feature-flag`, `experiment`,
`env-gate`, `locale-gate`.

**N/A note**: `route-guard`, `screen-permission`, `field-permission`, `resource-ownership`,
`api-scope`, `role-based`, `experiment`, `env-gate`, `locale-gate` have **no matches** — no
screen-list upstream (desktop app), no HTTP API surface with scoped tokens, no owner_id/created_by
resource model, no genuine role-based system (the only role-shaped enum, `ChannelRole`, is
vestigial protocol code with no live enforcement path — see note above), and no A/B experiment or
locale-conditioned access-control branch found in the scout inventory.

## Permissions Index

| Code | Name | Type | Enforced At |
|------|------|------|-------------|
| PERM001_ExtensionProcessExecCapability | ExtensionProcessExecCapability | action-permission | `crates/extension/src/extension_manifest.rs:168` (`allow_exec`) |
| PERM002_ExtensionDownloadFileCapability | ExtensionDownloadFileCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::DownloadFile` match arm, mirrors `allow_exec`) |
| PERM003_ExtensionNpmInstallCapability | ExtensionNpmInstallCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::NpmInstallPackage` match arm, mirrors `allow_exec`) |
| PERM004_BufferCapabilityGate | BufferCapabilityGate | data-permission | `crates/language/src/buffer.rs:78-89` (`enum Capability`, `editable()`) |
| PERM005_WorktreeTrustGate | WorktreeTrustGate | action-permission | `crates/project/src/trusted_worktrees.rs` (`TrustedWorktreesStore::can_trust`), consumed at `crates/project/src/lsp_store.rs:449` and `crates/project/src/git_store.rs:1595` |
| PERM006_StaffFeatureFlagGate | StaffFeatureFlagGate | feature-flag | `crates/feature_flags/src/feature_flags.rs:114-124` (`FeatureFlag::enabled_for_staff`), individual flags declared in `crates/feature_flags/src/flags.rs` |

---

## PERM001_ExtensionProcessExecCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs:168` (`ExtensionManifest::allow_exec`)

### Description

Before a sandboxed WASM extension may spawn a host process, `allow_exec(desired_command,
desired_args)` checks whether any of the extension's declared `capabilities` entries is an
`ExtensionCapability::ProcessExec(ProcessExecCapability)` whose match rule (exact command, plus
positional/wildcard/double-wildcard arg matching — verified in
`crates/extension/src/extension_manifest.rs` unit tests `test_allow_exec_exact_match`,
`test_allow_exec_wildcard_arg`, `test_allow_exec_double_wildcard`, `test_allow_exec_mixed_wildcards`)
matches the requested command+args. No match → the call errors out before the process is spawned.
This is a per-extension allowlist declared in the extension's manifest (`extension.toml`), fixed at
extension-authoring time — not a runtime user role and not user-prompted at install/run time.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension with a declared `ProcessExec` capability matching command+args | ✓ | Requested command+args match an allowlisted entry (exact or wildcard) |
| Extension with no matching capability declared | ✗ | Rejected before the process is spawned |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM002_ExtensionDownloadFileCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::DownloadFile` variant, `crates/extension/src/capabilities/download_file_capability.rs`)

### Description

Companion allowlist to PERM001 for the `DownloadFile` sandbox capability class — an extension may
only fetch from hosts it declared in its manifest's `capabilities` list. Same allow/deny shape as
`allow_exec`: the requested host is checked against the declared `DownloadFileCapability` entries
before the sandboxed fetch is permitted.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension with a declared `DownloadFile` capability matching the requested host | ✓ | Host matches an allowlisted entry |
| Extension with no matching capability declared | ✗ | Fetch rejected |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM003_ExtensionNpmInstallCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::NpmInstallPackage` variant, `crates/extension/src/capabilities/npm_install_package_capability.rs`)

### Description

Companion allowlist to PERM001/PERM002 for the `NpmInstallPackage` sandbox capability class — an
extension may only install npm packages it declared in its manifest's `capabilities` list.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension with a declared `NpmInstallPackage` capability matching the requested package | ✓ | Package matches an allowlisted entry |
| Extension with no matching capability declared | ✗ | Install rejected |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM004_BufferCapabilityGate

**Type**: data-permission
**Enforced At**: `crates/language/src/buffer.rs:78-89` (`enum Capability`, `Capability::can_edit`/`editable`-style check)

### Description

Every `Buffer` carries a `Capability` enum with three values: `ReadWrite` (normal editable
replica), `Read` (a mutable replica toggled to be read-only for display), and `ReadOnly` (a
replica that structurally cannot accept edits). Edit operations are gated on this value; a
rejected edit is a silent no-op/error at the API level rather than a user-facing dialog. In this
fork, without a live collaboration server, the practical source of a non-`ReadWrite` buffer is
local (view-only toggle) or SSH/remote-hosted content the local client was not given a mutable
handle to — not a remote collaborator's role grant.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Buffer with `Capability::ReadWrite` | ✓ | Edits accepted |
| Buffer with `Capability::Read` or `Capability::ReadOnly` | ✗ | Edit operations rejected |

### Related Modules

- crates/language (buffer.rs)
- crates/multi_buffer

---

## PERM005_WorktreeTrustGate

**Type**: action-permission
**Enforced At**: `crates/project/src/trusted_worktrees.rs` (`TrustedWorktreesStore::can_trust`, `trust`, `restrict`), consumed at `crates/project/src/lsp_store.rs:449` (gates language server spawn) and `crates/project/src/git_store.rs:525,1595` (gates git-integration behavior); UI surfaced via `crates/workspace/src/security_modal.rs` (`SecurityModal`)

### Description

The real local (non-collab) trust boundary in this codebase. Opening a folder or a single file
creates a worktree that is, by default, **untrusted** unless it was previously trusted-and-persisted
or the escape-hatch setting `ProjectSettings.session.trust_all_worktrees` is enabled
(`crates/project/src/project_settings.rs:122`). `TrustedWorktreesStore::can_trust` is checked
before potentially dangerous operations — most importantly, spawning a language server for the
worktree — and returns `false` (restricting the worktree, emitting `TrustedWorktreesEvent::
Restricted`) unless the worktree/path was explicitly trusted. Trust has an explicit hierarchy
(module doc in `trusted_worktrees.rs`): single-file worktree < directory worktree < path override
(trusting a parent directory transitively trusts all its subdirectories/worktrees). Trust state is
tracked per remote host (`RemoteHostLocation` — distinguishes SSH/WSL/Docker/local) and persisted
to a local DB (`DbTrustedPaths`) so re-opening a previously-trusted folder does not re-prompt.
Per the module's own doc comment, Docker and (vestigial) Collab remotes are explicitly exempted —
they "do not employ trust mechanism, as [they] manage that themselves" — so this gate applies to
local, SSH, and WSL projects. The `SecurityModal` UI (`crates/workspace/src/security_modal.rs`)
is the interactive prompt that lets the user grant or decline trust for a restricted path.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Worktree/path explicitly trusted (persisted or session-granted via `SecurityModal`) | ✓ | Language server / git-integration operations proceed |
| Worktree/path covered by a trusted parent-directory override | ✓ | Transitive trust via path hierarchy |
| `trust_all_worktrees` setting enabled | ✓ | All worktrees auto-trusted, bypassing the check entirely |
| Untrusted, not-yet-decided worktree | ✗ | Restricted — language server spawn deferred/blocked until user decides via `SecurityModal` |
| Invisible (internally-created, e.g. tmp dirs for `keymap_editor.rs`) worktree | ✓ | Trust check is skipped entirely — not user-facing |

### Related Modules

- crates/project (trusted_worktrees.rs, lsp_store.rs, git_store.rs, project.rs, worktree_store.rs, project_settings.rs)
- crates/workspace (security_modal.rs)
- crates/title_bar (title_bar.rs — surfaces trust state)
- crates/remote_server (headless_project.rs — remote-host side of the same mechanism)

---

## PERM006_StaffFeatureFlagGate

**Type**: feature-flag
**Enforced At**: `crates/feature_flags/src/feature_flags.rs:114-132` (`trait FeatureFlag`, `enabled_for_staff`/`enabled_for_all`), individual flags in `crates/feature_flags/src/flags.rs`
**source**: `crates/feature_flags/src/flags.rs` (e.g. `NotebookFeatureFlag`, `AcpBetaFeatureFlag`, `AgentSharingFeatureFlag`, `DiffReviewFeatureFlag`, `StreamingEditFileToolFeatureFlag`)

### Description

`FeatureFlagStore` (a `gpui::Global`) resolves each declared `FeatureFlag` type to on/off (or a
richer `FeatureFlagValue` for multi-variant flags) per user session. Every flag defaults
`enabled_for_staff() -> true` unless explicitly overridden to `false` (e.g.
`DiffReviewFeatureFlag`); the `ZED_DISABLE_STAFF` env var forces the current session to be treated
as non-staff for local testing of the disabled state. `enabled_for_all()` (default `false`) is the
override point for a flag that has been fully rolled out client-side without removing call sites.
Flags are watched reactively (`FeatureFlag::watch`, `FeatureFlagAppExt::observe_flag`) so a
server-pushed change to staff status or flag values re-renders dependent views without restart.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Zode staff account, flag has `enabled_for_staff() == true` (the default) | ✓ | Feature visible/active |
| Non-staff account, flag has `enabled_for_all() == true` | ✓ | Feature visible/active for everyone |
| Non-staff account, neither condition holds | ✗ | Feature hidden/inactive |
| Staff account with `ZED_DISABLE_STAFF` env var set | ✗ (treated as non-staff) | Used to locally test the disabled-for-non-staff path |

### Related Modules

- crates/feature_flags (feature_flags.rs, flags.rs, store.rs, settings.rs)

---

## Summary

- **Total Permission Items**: 6
- **By Type**: route-guard: 0 (N/A), screen-permission: 0 (N/A), action-permission: 4, data-permission: 1, role-based: 0 (N/A — see corrected-stale-pass note above), resource-ownership: 0 (N/A), field-permission: 0 (N/A), api-scope: 0 (N/A), feature-flag: 1, experiment: 0 (N/A), env-gate: 0 (N/A — `ZED_DISABLE_STAFF` is a testing override of PERM006, not an independent env-gated behavior branch), locale-gate: 0 (N/A)

---

## Cross-Reference Validation

- [x] All PERM### codes are unique
- [x] All PERM### codes are referenced in FeatureList.md — verified: all 6 PERM001–PERM006 codes are mapped to at least one F### in feature-list.md (Wave 5)
- [x] Related route references — N/A, no RouteList upstream (desktop app, not HTTP-routed)
- [x] Related screen references — N/A, no ScreenList upstream (`screen_source:none`)
- [x] All related module references verified directly against crate paths in source (this pass re-verified every path with `ls`/`grep` against the current tree, not copied from the prior draft)
- [x] No orphaned permission references
- [x] Confirmed `crates/collab` and `crates/call` do not exist in this repository (`ls crates` — both absent); the prior draft's collab-based PERM items are retracted, not carried forward

---

## Client-Side Gate Types

Only `feature-flag` (PERM006) has matches. No `experiment`, `env-gate`, or `locale-gate` call
sites were found in the Wave 0 scout inventory — no A/B variant-assignment code, no
`process.env`/`std::env`-conditioned behavioral branch beyond the `ZED_DISABLE_STAFF` testing
override already captured under PERM006, and no locale-conditioned UI/access branch.
