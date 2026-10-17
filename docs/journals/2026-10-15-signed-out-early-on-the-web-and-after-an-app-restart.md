# Signed out at 15 minutes on the web, and a restart that kept a spent refresh token

**Date**: 2026-10-15 23:27
**Severity**: High — one spent refresh token signs a user out of every session
**Component**: web `frontend/src/api/session-fetch.ts`; zode `crates/zode_account/src/account.rs`
**Status**: ongoing — web fix deployed; server TTL and app release still open

## What Happened

The user reported being signed out quickly, on the web and in the app.

The site never called `POST /api/auth/refresh`. When the 15-minute access cookie expired, `fetchMe` got a 401 and returned `null`, leaving the 30-day refresh cookie unused.

In the app, `Account::restore` refreshed an expired token at startup but saved only to memory. The keychain kept the spent token, and a quit before the next saved refresh left it for the next start to present. Presenting a spent token makes the backend revoke every session of the user, the browser's included. The production logs were not read, so how often this happened is unknown.

## The Brutal Truth

The galling part: the warning was already in the code. The common refresh path saved at once, under a comment naming this exact hazard. Restore refreshed the same token and skipped the save.

The strict policy stays, and the user declined a grace window. The price: any client re-presenting a spent token signs everyone out. Builds up to 0.1.4 keep the bug until users upgrade.

## Technical Details

- Backend: `Refresh token reuse detected, revoking all sessions for userId=…`, then `revokeAll(userId)`.
- Pre-fix `auth.ts`: `if (response.status === 401) { return null; }`
- Post-fix: `navigator.locks` `zode-auth-refresh`, a `/api/auth/me` probe (10 s timeout), `keepalive` refresh.
- Gates: web 241 frontend tests; backend 336 unit, 120 e2e; zode_account 36.

## What We Tried

- **Probe by re-sending the original request** (the brief's shape): changed to `GET /api/auth/me`, which asks the same question without replaying a write.
- **Saving after `/auth/me`**: the first app version dropped rotated tokens for a legacy entry (no user in its payload) when `/auth/me` was unreachable. The tester caught it. Fixed by keeping the keychain username and writing `user: None`.

## Root Cause Analysis

Rotation makes a spent token fatal. Each client owes two things: never run two refreshes on one cookie (the web lock), and persist the rotated pair before anything else can fail (the app). The web never called the refresh endpoint. The app persisted at one of its two `ensure_fresh` call sites, not both.

A lost refresh response still leaves a spent token in the jar. `keepalive` narrows that window; it does not close it.

## Lessons Learned

- Under strict reuse detection, one path that re-presents a spent token signs out every session. Persist before anything else can fail.
- A warning comment is not a guard. A rotating store must be written at every refresh call site.
- A longer TTL widens revocation: a revoked session works on JWT-only routes for up to 4h, including `device/approve`. The user accepted this.

## Next Steps

- **Operator**: set `JWT_ACCESS_TTL=4h` in the production `.env` and run `docker compose up -d backend`. The live value is unknown (the `.env` is unreadable from here), and the new code default applies only when the variable is unset. Done when `Max-Age=14400` appears per `docs/deployment.md`. No date set.
- **User**: run the browser check in `docs/deployment.md`. Not yet run.
- **User**: release Zode with ac29d42 via PR #43 and count reuse events for a week. The PR also carries 92fd321, an unrelated sidebar fix.
