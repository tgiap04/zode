<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Permissions

**Rewritten 2026-08-04** against the post-fork tree (187 packages / 178 crates). The original
draft's entire "Collaboration roles" section (`ChannelRole`: Admin/Member/Talker/Guest/Banned) is
removed — the collaboration system it described no longer exists in this fork. What remains below
is genuinely current.

**Project**: Zode
**Analysis Scope**: Full monorepo — native GPUI desktop editor

> **Curated, plain-language view.** This document is for PM, BA, and client audiences who
> need to understand access without reading raw codes. The raw PERM### matrix lives at
> [permissions-matrix.md](permissions-matrix.md). Derived from that matrix.

## Honesty Note

Zode is not a web application, so it has no traditional multi-tenant user-role system (no
admin/manager/user hierarchy governing the whole app), and — after this fork removed real-time
collaboration — no collaboration-role system either. "Permissions" in this codebase now means two
distinct, unrelated things:

1. What a third-party **extension** is allowed to do on your machine (sandboxing).
2. Whether an open **file/buffer** can be edited (read-write capability).

## Authorization System Type

**System Type**: `capability-based` (not role-based)

Extension sandboxing is a per-extension declared-capability allowlist mechanism (not role-based
at all). File/buffer editability is a per-resource capability attached to the open file. There is
no resource-ownership (owner_id/created_by) model and no application-wide user roles, and no
role-based system of any kind remains after collaboration was removed.

## Curated View

**1. Extension sandboxing** — when you install a third-party extension, it cannot run arbitrary
commands, download from arbitrary hosts, or install arbitrary npm packages. It can only do the
specific things it declared up front in its manifest (an allowlist of exact commands, hosts, and
packages). Anything not declared is rejected before it runs.

**2. File/buffer editing** — every open file (and every combined multi-file view, like a search
results pane) carries its own read-write status. A file marked read-write can be edited normally.
A file marked read-only (for example, via the `read_only_files` setting) silently rejects edit
attempts — there's no error dialog, the edit simply doesn't apply.

## Access Boundaries

- **Extensions** are sandboxed regardless of who installed them. Extension permission is about
  "what can this piece of installed code do to my machine" — a fixed allowlist evaluated at
  extension-authoring time, not something the user grants or denies at runtime.
- **File editability** is a per-file property, not tied to any concept of user identity — there is
  only one user of a running instance of this editor.

## Special Conditions

- No feature flags, A/B experiments, environment-based gates, or locale-based access rules were
  found anywhere in this codebase — access here is governed entirely by the two mechanisms above,
  nothing environment- or rollout-driven.
- There is no concept of a signed CLA, an account, or any per-user gate remaining in this fork.
