<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Permissions Matrix

**Project**: Zed (zode)
**Generated**: 2026-07-26
**Analysis Scope**: Full monorepo, `generic-source` profile. No screen-list/screen-flow upstream (`screen_source:none`) — Zed is a native GPUI desktop app, not a web app, so this matrix maps to (a) extension WASM sandbox capability grants, (b) Buffer/MultiBuffer read-write capability per collaborator, and (c) collaboration-server RPC role/capability guards. There is no traditional web-style RBAC (no admin/user/manager application roles) — the only role system that exists is the collaboration `ChannelRole` used for shared channels/calls/projects.

> **Raw PERM### matrix.** Machine-generated inventory of every permission item with full
> per-permission detail. The plain-language curated view lives at
> [permissions.md](permissions.md). This file is written FIRST; the curated view derives from it.

**Code Format**: All codes follow `PERM###_NameSlug` format.

**Permission Types**: `route-guard`, `screen-permission`, `action-permission`, `data-permission`, `role-based`, `resource-ownership`, `field-permission`, `api-scope`, `feature-flag`, `experiment`, `env-gate`, `locale-gate`.

**N/A note**: `screen-permission`, `field-permission`, `feature-flag`, `experiment`, `env-gate`, `locale-gate`, `api-scope`, `resource-ownership` have **no matches** in this codebase — there is no screen-list upstream (desktop app, not web UI screens with route/element-level auth), no HTTP API surface with scopes/tokens, no feature-flag/experiment/env-gate/locale-gate call sites found in the Wave 0 scout inventory. These types are marked N/A in the Summary rather than fabricated.

## Permissions Index

| Code | Name | Type | Enforced At |
|------|------|------|-------------|
| PERM001_ExtensionProcessExecCapability | ExtensionProcessExecCapability | action-permission | `crates/extension/src/extension_manifest.rs:168` (`allow_exec`) |
| PERM002_ExtensionDownloadFileCapability | ExtensionDownloadFileCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::DownloadFile` match arms, mirrors `allow_exec` pattern) |
| PERM003_ExtensionNpmInstallCapability | ExtensionNpmInstallCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::NpmInstallPackage` match arms, mirrors `allow_exec` pattern) |
| PERM004_BufferCapabilityGate | BufferCapabilityGate | data-permission | `crates/language/src/buffer.rs:89` (`editable()`) |
| PERM005_CollabMutatingProjectRequestGuard | CollabMutatingProjectRequestGuard | route-guard | `crates/collab/src/db/queries/projects.rs:1154` (`host_for_mutating_project_request`) |
| PERM006_CollabReadOnlyProjectRequestGuard | CollabReadOnlyProjectRequestGuard | route-guard | `crates/collab/src/db/queries/projects.rs:1138` (`host_for_read_only_project_request`) |
| PERM007_ChannelRoleAccessControl | ChannelRoleAccessControl | role-based | `crates/collab/src/db/ids.rs:133-236` (`ChannelRole` + `can_edit_projects`/`can_read_projects`/`can_use_microphone`/`can_see_channel`) |
| PERM008_DisallowGuestRequestGuard | DisallowGuestRequestGuard | route-guard | `crates/collab/src/rpc.rs:2283` (`disallow_guest_request`) |

---

## PERM001_ExtensionProcessExecCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs:168` (`ExtensionManifest::allow_exec`)

### Description

Before a sandboxed WASM extension may spawn a host process, `allow_exec(desired_command, desired_args)` checks whether any of the extension's declared `capabilities` entries is an `ExtensionCapability::ProcessExec` capability whose `allows(command, args)` matches the requested command+args. No match → `bail!` with an error surfaced to the caller ("capability for process:exec ... was not listed in the extension manifest"). This is a per-extension allowlist declared in `extension.toml`, not a runtime user role.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension (declared ProcessExec capability matching command+args) | ✓ | Requested command+args must match an allowlisted entry |
| Extension (no matching capability declared) | ✗ | Rejected at dispatch (`BL004_ExtensionHostWasmDispatch`) before the process is spawned |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM002_ExtensionDownloadFileCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::DownloadFile` variant; DISC-017)

### Description

Companion allowlist to PERM001 for the `DownloadFile` sandbox capability class — an extension may only fetch from hosts it declared in its manifest's `capabilities` list. Same gate shape as `allow_exec`: checked against the declared capability entries before permitting the sandboxed fetch.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension (declared DownloadFile capability matching host) | ✓ | Requested host must match an allowlisted entry |
| Extension (no matching capability declared) | ✗ | Fetch rejected |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM003_ExtensionNpmInstallCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::NpmInstallPackage` variant; DISC-017)

### Description

Companion allowlist to PERM001/PERM002 for the `NpmInstallPackage` sandbox capability class — an extension may only install npm packages it declared in its manifest's `capabilities` list.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension (declared NpmInstallPackage capability matching package) | ✓ | Requested package must match an allowlisted entry |
| Extension (no matching capability declared) | ✗ | Install rejected |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM004_BufferCapabilityGate

**Type**: data-permission
**Enforced At**: `crates/language/src/buffer.rs:89` (`Capability::editable()`), shared by `MultiBuffer` (DISC-005)

### Description

Every `Buffer` (and `MultiBuffer`, which delegates to the same discriminator) carries a `Capability` enum: `ReadWrite`, `Read`, `ReadOnly`. `editable()` gates whether edit operations are accepted. `ReadWrite` = normal editable replica; `Read` = a mutable replica toggled to read-only display (e.g. a follower viewing someone else's cursor); `ReadOnly` = a replica that structurally cannot accept edits (e.g. a remote guest without write access — see PERM005/PERM007). Rejected edits are no-ops/errors at the API level, not a user-facing error message.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Buffer with `Capability::ReadWrite` | ✓ | Edits accepted |
| Buffer with `Capability::Read` or `Capability::ReadOnly` | ✗ | Edit operations rejected (`editable()` guard) |

### Related Modules

- crates/language (buffer.rs)
- crates/multi_buffer

---

## PERM005_CollabMutatingProjectRequestGuard

**Type**: route-guard
**Enforced At**: `crates/collab/src/db/queries/projects.rs:1154` (`host_for_mutating_project_request`), dispatched via `crates/collab/src/rpc.rs:2263` (`forward_mutating_project_request`)

### Description

RPC requests that would mutate a shared project (e.g. edit buffers) are resolved through `host_for_mutating_project_request`, which calls `access_project(project_id, connection_id, Capability::ReadWrite, tx)` — the requesting collaborator's project-level capability must be `ReadWrite` or the request is rejected before it ever reaches the host. This is the collab-server-side counterpart to the client-side PERM004 buffer gate — it prevents a read-only collaborator from issuing a mutating RPC in the first place.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Collaborator granted `Capability::ReadWrite` on the project | ✓ | Mutating request forwarded to host |
| Collaborator granted `Capability::ReadOnly` on the project | ✗ | Rejected by `access_project` before forwarding |

### Related Modules

- crates/collab (db/queries/projects.rs, rpc.rs)

---

## PERM006_CollabReadOnlyProjectRequestGuard

**Type**: route-guard
**Enforced At**: `crates/collab/src/db/queries/projects.rs:1138` (`host_for_read_only_project_request`), dispatched via `crates/collab/src/rpc.rs:2242` (`forward_read_only_project_request`)

### Description

Read-only RPC requests (search, document symbols, code actions read-side, project symbols, etc. — see `BL007_RpcProtoMessageRouting`) are resolved through `host_for_read_only_project_request`, which calls `access_project(project_id, connection_id, Capability::ReadOnly, tx)`. Any collaborator who can at least read the project (including guests) may issue these; it is the counterpart to PERM005 for the non-mutating half of the RPC surface.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Collaborator granted `Capability::ReadOnly` or higher | ✓ | Read-only request forwarded to host |
| Collaborator with no access grant on the project | ✗ | Rejected |

### Related Modules

- crates/collab (db/queries/projects.rs, rpc.rs)

---

## PERM007_ChannelRoleAccessControl

**Type**: role-based
**Enforced At**: `crates/collab/src/db/ids.rs:133-236` (`ChannelRole` enum + methods)

### Description

`ChannelRole` is Zed's only genuine role-based access system, scoped to collab channels/calls/shared-projects (not the whole application). Five roles: `Admin` (read/write, can change permissions), `Member` (read/write, cannot change permissions), `Talker` (read-only, can use mic + chat), `Guest` (read-only, chat only, no mic), `Banned` (no access). The enum exposes per-capability predicates rather than a flat allow-list: `can_see_channel` (gated further by channel visibility for Guest/Talker), `can_see_all_descendants`, `can_only_see_public_descendants`, `can_use_microphone`, `can_edit_projects`, `can_read_projects`, `requires_cla` (Admin/Member only, used at `crates/collab/src/db/queries/rooms.rs:1140` — contribution license-agreement gate for elevated roles).

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Admin | ✓ (read/write/edit-permissions/mic/CLA) | `can_edit_projects`=true, `can_read_projects`=true, `can_use_microphone`=true, `requires_cla`=true |
| Member | ✓ (read/write/mic/CLA), ✗ (edit-permissions) | `can_edit_projects`=true, `can_read_projects`=true, `can_use_microphone`=true, `requires_cla`=true |
| Talker | ✓ (read/mic), ✗ (write) | `can_edit_projects`=false, `can_read_projects`=true, `can_use_microphone`=true, sees only public descendants |
| Guest | ✓ (read/chat), ✗ (write/mic) | `can_edit_projects`=false, `can_read_projects`=true, `can_use_microphone`=false, sees only public descendants |
| Banned | ✗ (all) | `can_see_channel`=false, `can_read_projects`=false |

### Related Modules

- crates/collab (db/ids.rs, db/queries/rooms.rs)

---

## PERM008_DisallowGuestRequestGuard

**Type**: route-guard
**Enforced At**: `crates/collab/src/rpc.rs:2283` (`disallow_guest_request`)

### Description

A subset of RPC handlers (e.g. `GitRemoveWorktree`, `GitRenameWorktree`, `GitCreateArchiveCheckpoint`, `GitRestoreArchiveCheckpoint`) are wired to `disallow_guest_request`, which unconditionally responds with `ErrorCode::Forbidden` ("request is not allowed for guests") regardless of project capability — a hard denylist independent of the `Capability::ReadWrite` check in PERM005, applied to specific destructive git operations even for otherwise-writable collaborators below Member.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Any collaborator routed through `disallow_guest_request` handlers | ✗ | Always `Forbidden`, regardless of ReadWrite grant — applies to specific git-worktree-mutation RPCs only |

### Related Modules

- crates/collab (rpc.rs)

---

## Summary

- **Total Permission Items**: 8
- **By Type**: route-guard: 3, screen-permission: 0 (N/A — no screen-list upstream), action-permission: 3, data-permission: 1, role-based: 1, resource-ownership: 0 (N/A — no owner_id/created_by resource model found), field-permission: 0 (N/A — no per-field visibility gating found), api-scope: 0 (N/A — no OAuth/API-scope surface; RPC is a typed binary protocol, not scoped tokens), feature-flag: 0 (N/A — no runtime feature-flag call sites in scout inventory), experiment: 0 (N/A — no A/B experiment call sites found), env-gate: 0 (N/A — no hardcoded env-var behavioral gates found), locale-gate: 0 (N/A — no locale-conditioned UI branch found)

---

## Cross-Reference Validation

- [x] All PERM### codes are unique
- [ ] All PERM### codes are referenced in FeatureList.md — N/A this pass, no upstream FeatureList generated (`generic-source` profile, no feature-specs pass run yet)
- [x] All related route references — N/A, no RouteList upstream (desktop app, not an HTTP-routed web app)
- [x] All related screen references — N/A, no ScreenList upstream (`screen_source:none`)
- [x] All related module references are valid (verified against crate paths read directly from source)
- [x] No orphaned permission references

---

## Client-Side Gate Types

N/A — no `feature-flag`, `experiment`, `env-gate`, or `locale-gate` call sites were found in the Wave 0 scout inventory for this codebase. Zed is a native desktop app with no web-frontend runtime; the extraction signatures for these gate types (`useFlag`, `useExperiment`, `process.env.*` behavioral branches, `i18n.locale ===`) target JS/TS/web patterns not present here. If such gates exist they were not surfaced by the representative-sample scout pass — see behavior-logic.md's Representative-Not-Exhaustive Notice for the same caveat applied to BL### items.
