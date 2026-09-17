# Env sync design collided with a decision the code had already made

**Date**: 2026-09-17 21:25
**Severity**: Low — caught during consultation, before a line was written
**Component**: `zode_sync`, `zode_account`, `crates/cli`, `tooling/xtask`
**Status**: Design sealed; blueprint at `plans/260917-2125-env-sync-e2ee/` (blocked)

## What Happened

A request came in to sync `.env` files across machines through the web backend, with a
user-created encryption password whose hash would be stored server-side. Consultation found
that `crates/zode_sync/` already implements the whole E2EE stack — and that it had already
rejected the password model on purpose.

## The three things reading the code disproved

### 1. The password model was already considered and refused

`crates/zode_sync/src/dek.rs:24` states it plainly:

> There is no key derivation step and no server-held wrapping key — both would put something
> crackable in reach of the server that already holds the ciphertext.

The requested design would have added exactly that, plus a server-held verifier — an oracle
for checking brute-force guesses without touching the ciphertext. The feature's stated goal
was "so users can see the web cannot steal this". The requested mechanism worked against it.

Worse, in the specific shape requested: the password was to live in zode's config. But
`settings.json` **is** a synced artifact (`kind=settings`). The secret would have ridden the
sync it was meant to protect.

Settled on: `envDEK` wrapped under the existing recovery key. One key for the user, no
password anywhere, no derivation next to the ciphertext.

### 2. `release.yml` is generated, so a CI change there would have been silently reverted

The blueprint's SLSA provenance step first landed in `.github/workflows/release.yml`.
Line 1 of that file:

```
# Generated from xtask::workflows::release
# Rebuild with `cargo xtask workflows`.
```

The real edit site is `tooling/xtask/src/tasks/workflows/release.rs`. Editing the YAML would
survive until the next regenerate, then vanish — and nothing would turn red. The release
would just stop carrying attestations.

### 3. `crates/cli` has no gpui App, so `zode env inspect` cannot read the keychain

The verification CLI was specified to read `envDEK` from the keychain. `CredentialsProvider`
needs an `AsyncApp`; `crates/cli/src/main.rs` is a launcher shim that spawns the real binary
and has no gpui context at all.

This turned out to improve the design rather than constrain it. A tool whose whole job is
proving what leaves your machine should not touch your keychain. It now takes the recovery
key on stdin, opens no socket, and holds no state — runnable on a clean machine and
diffable against `mitmproxy`.

## Two holes in the shipped code the design had to close

| Hole | Where | Consequence for env |
|---|---|---|
| AAD binds `user_id ∥ kind` only | `zode_sync/src/envelope.rs:aad` | With N entries per user, the server can move a blob between two of that user's own entries and the client decrypts it happily. Closed by adding `entry_id`. |
| `write_atomic` uses `std::fs::write`; `back_up` writes beside the original | `zode_sync/src/artifact.rs` | Mode 0644 on a `.env`, and a `.env_backup` landing **inside the repo**, ungitignored. Neither is reusable; phase-04 writes its own. |

Neither is a defect in settings sync — `settings.json` is 0644 anyway and its backup belongs
next to it. They are defects only under the new load.

## The claim that had to go into the docs rather than the code

`grep -rn 'is_private' crates/agent_ui crates/agent_sessions crates/edit_prediction_types crates/telemetry`
returns nothing. `private_files` defaults to `**/.env*` but gates collab sharing only; zode's
agents are external CLI processes reading the filesystem directly.

Env sync does not make this worse. But a feature named "env sync" will be blamed for it, so
phase-06 writes the boundary down instead of waiting to be asked.

## Lesson

The consultation's value here was not choosing between designs. It was finding that the
codebase had already chosen, had written down why, and that the new request contradicted it.
Four of the eight decisions taken went against the requester's opening description — every
one of them because a comment in the existing source explained a constraint the request had
not accounted for.

Reading `dek.rs` cost ten minutes. Building the password model would have cost weeks and
shipped a weaker promise than the one already in the repo.
