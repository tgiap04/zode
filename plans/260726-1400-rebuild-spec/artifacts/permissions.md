<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Permissions

**Project**: zode (Zed fork)
**Generated**: 2026-08-07
**Analysis Scope**: Full monorepo — native GPUI desktop editor, no collaboration/multiplayer
backend, local-first with SSH-based remote development.

> **Curated, plain-language view.** This document is for PM/BA/client audiences who need to
> understand access without reading raw codes. The raw PERM### matrix lives at
> [permissions-matrix.md](permissions-matrix.md). Derived from that matrix.

## Honesty Note

zode is not a web application and, unlike upstream Zed, it does not ship a collaboration server
(no `crates/collab`, no `crates/call` — confirmed by direct inspection of the source tree). So
there is no multi-tenant user-role system at all, not even the collaboration `ChannelRole` system
upstream Zed has. What this codebase calls "permissions" is three unrelated, independent gates:

1. What a third-party **extension** is allowed to do on your machine (sandboxing).
2. Whether an open **file/buffer** can be edited (read-write capability).
3. Whether a **project/folder you open** is trusted enough to spawn language servers and other
   tooling against it (local workspace trust).

There is also a smaller, separate mechanism — feature flags — that gates which in-progress
features a given account (staff vs. everyone) sees, unrelated to the three above.

## Authorization System Type

**System Type**: `other`

None of `rbac`, `abac`, `acl`, `ownership`, or `hybrid` describe this system well — there is no
role hierarchy and no owner_id/resource-ownership model anywhere in the codebase. What exists
instead are three independent capability/trust gates plus a feature-flag rollout mechanism, none
of which check "who is this user" against a role table.

**Identified Roles**: None. There is no role system in this application.

## Curated View

**1. Extension sandboxing** — when you install a third-party extension, it cannot run arbitrary
commands, download from arbitrary hosts, or install arbitrary npm packages. It can only do the
specific things it declared up front in its manifest: an exact-match (or wildcard-pattern)
allowlist of commands+arguments, download hosts, and npm packages. Anything not declared is
rejected before it runs — there's no runtime prompt to approve an undeclared action; it's a fixed
allowlist set at the time the extension was authored.

**2. File/buffer editing** — every open file (and any combined multi-file view built from open
files, like a search-results pane) carries its own read-write status. A file marked read-write can
be edited normally. A file marked read-only (for example, a buffer deliberately toggled to
view-only, or content from a remote SSH host the local client wasn't given a mutable handle to)
silently rejects edit attempts — there's no error dialog, the edit simply doesn't apply.

**3. Workspace/worktree trust** — the first time you open a folder (or a single file outside any
folder), it is treated as **untrusted** by default. Zed will not spawn a language server (or
perform certain git-integration operations) against an untrusted folder until you explicitly grant
trust through a security prompt. Trust has levels: trusting a single file is the narrowest grant;
trusting a whole directory covers everything inside it; trusting a parent directory transitively
covers every subdirectory and file underneath, current or future. Once granted, trust is
remembered — reopening the same folder later does not re-prompt. There's also a global escape
hatch setting that, if turned on, auto-trusts everything and skips the prompt entirely. This trust
mechanism tracks state separately per remote host (a local folder, an SSH host, and a WSL
distribution are each tracked independently) — Docker-based projects are explicitly exempted since
Docker manages its own isolation.

**4. Feature flags** — a small number of in-development features (notebooks, an "ACP beta"
program, agent-sharing, diff review, a streaming edit-file tool) are gated behind flags that
default to visible for Zed staff accounts and hidden for everyone else, unless a flag has been
explicitly marked as rolled out to all users. This is a rollout mechanism, not a security boundary.

## Access Boundaries

The real dividing lines in this codebase are not "admin vs. user" in the web-app sense — they are:

- **Trusted vs. untrusted workspace.** This is the boundary that matters most for security: an
  untrusted folder cannot get a language server (or certain git operations) run against it until
  the user consciously grants trust. This exists because spawning a language server means running
  an executable against files in that folder — potentially dangerous for a folder the user hasn't
  vetted (e.g. cloned from somewhere unfamiliar).
- **Extension vs. host machine.** Extensions are sandboxed regardless of who installed them —
  their permission boundary is "what can this installed piece of code do to my machine," entirely
  separate from workspace trust or file editability.
- **Editable vs. read-only content.** A narrower, per-file boundary: some open content (view-only
  toggles, some remote-hosted content) simply cannot accept edits regardless of workspace trust or
  extension state.
- **Staff vs. everyone**, for feature flags only — this is a rollout gate, not an access-control
  boundary; it doesn't protect anything, it just controls which in-progress features are visible.

## Special Conditions

- A global setting (`trust_all_worktrees`) lets a user disable the workspace-trust prompt entirely,
  auto-trusting every folder they open — an explicit opt-out for users who find the prompt
  unnecessary friction.
- Workspace trust is skipped silently for folders Zed creates internally for its own purposes
  (e.g. a temp directory used by the keymap editor) — these were never user-opened content, so
  there's nothing to vet.
- Docker-based and (legacy, non-functional in this fork) collaboration-based projects are
  explicitly exempted from the workspace-trust mechanism, per the module's own documentation,
  because those transport layers manage isolation themselves.
- A `ZED_DISABLE_STAFF` environment variable exists purely for local testing — it forces a staff
  account to be treated as non-staff so a developer can verify what non-staff users see for a
  given feature flag. It is a test aid, not a real access boundary.
- No A/B experiments, environment-based access gates, or locale-based access rules were found
  anywhere in this codebase.
