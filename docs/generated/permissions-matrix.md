<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Permissions Matrix

**Rewritten 2026-08-04** against the post-fork tree (187 packages / 178 crates). PERM005 through
PERM008 described the collaboration server's (`crates/collab`) role-based access control
(`ChannelRole`: Admin/Member/Talker/Guest/Banned) and route guards — that crate no longer exists,
and these are removed rather than rewritten. PERM001–004 (extension sandbox capabilities and
buffer read-write gating) are still current and unchanged in substance.

**Project**: Zode
**Analysis Scope**: Full monorepo, `generic-source` profile. No screen-list/screen-flow upstream
(`screen_source:none`) — this is a native GPUI desktop app, not a web app, so this matrix maps to
(a) extension WASM sandbox capability grants and (b) Buffer/MultiBuffer read-write capability.
There is no role-based access system of any kind left in this fork — the only one that existed
(`ChannelRole`, scoped to the now-deleted collaboration server) was removed along with it.

> **Raw PERM### matrix.** Machine-generated inventory of every permission item with full
> per-permission detail. The plain-language curated view lives at
> [permissions.md](permissions.md). This file is written FIRST; the curated view derives from it.

**Code Format**: All codes follow `PERM###_NameSlug` format.

**Permission Types**: `route-guard`, `screen-permission`, `action-permission`, `data-permission`, `role-based`, `resource-ownership`, `field-permission`, `api-scope`, `feature-flag`, `experiment`, `env-gate`, `locale-gate`.

**N/A note**: every type except `action-permission` and `data-permission` has **no matches** in
this codebase now. There is no screen-list upstream, no HTTP API surface with scopes/tokens, no
feature-flag/experiment/env-gate/locale-gate call sites, and — after this fork's removal of the
collaboration server — no `role-based` or `route-guard` permission type left either.

## Permissions Index

| Code | Name | Type | Enforced At |
|------|------|------|-------------|
| PERM001_ExtensionProcessExecCapability | ExtensionProcessExecCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`allow_exec`) |
| PERM002_ExtensionDownloadFileCapability | ExtensionDownloadFileCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::DownloadFile` match arms) |
| PERM003_ExtensionNpmInstallCapability | ExtensionNpmInstallCapability | action-permission | `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::NpmInstallPackage` match arms) |
| PERM004_BufferCapabilityGate | BufferCapabilityGate | data-permission | `crates/language/src/buffer.rs` (`editable()`) |

---

## PERM001_ExtensionProcessExecCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionManifest::allow_exec`)

### Description

Before a sandboxed WASM extension may spawn a host process, `allow_exec(desired_command, desired_args)` checks whether any of the extension's declared `capabilities` entries is an `ExtensionCapability::ProcessExec` capability whose `allows(command, args)` matches the requested command+args. No match → rejected with an error surfaced to the caller. This is a per-extension allowlist declared in `extension.toml`, not a runtime user role.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Extension (declared ProcessExec capability matching command+args) | ✓ | Requested command+args must match an allowlisted entry |
| Extension (no matching capability declared) | ✗ | Rejected at dispatch (`BL002_ExtensionHostWasmDispatch`) before the process is spawned |

### Related Modules

- crates/extension
- crates/extension_host

---

## PERM002_ExtensionDownloadFileCapability

**Type**: action-permission
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::DownloadFile` variant)

### Description

Companion allowlist to PERM001 for the `DownloadFile` sandbox capability class — an extension may only fetch from hosts it declared in its manifest's `capabilities` list.

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
**Enforced At**: `crates/extension/src/extension_manifest.rs` (`ExtensionCapability::NpmInstallPackage` variant)

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
**Enforced At**: `crates/language/src/buffer.rs` (`Capability::editable()`), shared by `MultiBuffer`

### Description

Every `Buffer` (and `MultiBuffer`, which delegates to the same discriminator) carries a `Capability` enum: `ReadWrite`, `Read`, `ReadOnly`. `editable()` gates whether edit operations are accepted — for example, the `read_only_files` setting toggles a buffer to non-editable. Rejected edits are no-ops at the API level, not a user-facing error message.

### Permission Rules

| Role | Allow | Conditions |
|------|-------|------------|
| Buffer with `Capability::ReadWrite` | ✓ | Edits accepted |
| Buffer with `Capability::Read` or `Capability::ReadOnly` | ✗ | Edit operations rejected (`editable()` guard) |

### Related Modules

- crates/language (buffer.rs)
- crates/multi_buffer

---

## Summary

- **Total Permission Items**: 4
- **By Type**: action-permission: 3, data-permission: 1, everything else: 0 (N/A — see above; `role-based` and `route-guard` in particular were removed along with the collaboration server)

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

N/A — no `feature-flag`, `experiment`, `env-gate`, or `locale-gate` call sites exist in this
codebase. Zode is a native desktop app with no web-frontend runtime; the extraction signatures
for these gate types target JS/TS/web patterns not present here.
