<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Permissions

**Project**: Zed (zode)
**Generated**: 2026-07-26
**Analysis Scope**: Full monorepo — native GPUI desktop editor

> **Curated, plain-language view.** This document is for PM, BA, and client audiences who
> need to understand access without reading raw codes. The raw PERM### matrix lives at
> [permissions-matrix.md](permissions-matrix.md). Derived from that matrix.

## Honesty Note

Zed is not a web application, so it has no traditional multi-tenant user-role system (no
admin/manager/user hierarchy governing the whole app). "Permissions" in this codebase mean
three distinct, unrelated things, each covered below:

1. What a third-party **extension** is allowed to do on your machine (sandboxing).
2. Whether an open **file/buffer** can be edited (read-write capability).
3. What a **collaborator** in a shared coding session is allowed to see/do (collaboration roles).

## Authorization System Type

**System Type**: `hybrid`

The collaboration system is genuinely role-based (`ChannelRole`: Admin, Member, Talker, Guest,
Banned) combined with a per-resource capability grant (`Capability`: ReadWrite, Read, ReadOnly)
attached to each shared project/buffer. Extension sandboxing is a separate, unrelated allowlist
mechanism (not role-based at all — it's per-extension declared capabilities). There is no
resource-ownership (owner_id/created_by) model and no application-wide user roles.

**Identified Roles** (collaboration only — these do not apply outside a shared session):
- Admin — full read/write plus the ability to change other members' permissions
- Member — full read/write, cannot change others' permissions
- Talker — read-only, can use voice chat and text chat
- Guest — read-only, text chat only, no voice
- Banned — no access at all

## Curated View

**1. Extension sandboxing** — when you install a third-party extension, it cannot run arbitrary
commands, download from arbitrary hosts, or install arbitrary npm packages. It can only do the
specific things it declared up front in its manifest (an allowlist of exact commands, hosts, and
packages). Anything not declared is rejected before it runs.

**2. File/buffer editing** — every open file (and every combined multi-file view, like a search
results pane) carries its own read-write status. A file marked read-write can be edited normally.
A file marked read-only (for example, a buffer someone is only viewing, or a remote collaborator's
file they weren't given write access to) silently rejects edit attempts — there's no error dialog,
the edit simply doesn't apply.

**3. Collaboration roles** — in a shared coding session:
- **Admins and Members** can read and edit the shared project's files. Admins can additionally
  change what role other participants have.
- **Talkers** can view the shared project and use voice + text chat, but cannot edit anything.
- **Guests** can view the shared project and use text chat only (no microphone), and cannot edit.
- **Banned** participants cannot see or access the shared project at all.

Regardless of role, a small set of destructive git operations (removing a worktree, renaming a
worktree, creating/restoring an archive checkpoint) are blocked outright for any non-owning
participant — these are hard-denied, not merely gated by the read/write role check above.

## Access Boundaries

The real dividing line in this codebase is **host vs. collaborator**, not "admin vs. user" in the
web-app sense:

- The person who **shares** a project (the host) always has full control over it locally; their
  own machine enforces nothing extra on themselves.
- Every **collaborator** who joins gets a role (Admin/Member/Talker/Guest) that determines both
  what they can see and whether their edits are even forwarded to the host — a read-only
  collaborator's edit requests are rejected server-side before the host ever receives them, not
  just hidden in the UI.
- **Extensions** live in a completely separate boundary: they are sandboxed regardless of who
  installed them or what collaboration role is active. Extension permission is about "what can
  this piece of installed code do to my machine," unrelated to who you're collaborating with.

## Special Conditions

- Admin and Member roles additionally require a signed contribution license agreement (CLA) for
  certain elevated actions in collaboration rooms — Talker/Guest/Banned roles are exempt (they
  can't perform the actions that would require it anyway).
- A small set of git-worktree-destructive operations are blocked for all non-hosting participants
  regardless of their assigned role — this is a hard denylist layered on top of the role system,
  not a role tier itself.
- No feature flags, A/B experiments, environment-based gates, or locale-based access rules were
  found anywhere in this codebase — access here is governed entirely by the three mechanisms
  above, nothing environment- or rollout-driven.
