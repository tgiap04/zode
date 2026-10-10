# Remote control through the zodekit.site relay

**Date**: 2026-10-10
**Severity**: High — whoever controls a Zode has a shell on its machine
**Component**: `remote_control`, `remote_relay_protocol`, `remote_relay_client`, `remote`, `remote_server`, `git`, web relay and `/remote`
**Status**: Committed on `feat.release-v0.1.5` in both repos; not yet checked by hand on two real machines or a phone

## What Happened

Another device can now drive a Zode: a browser at `zodekit.site/remote` mirrors agent tabs and terminals and browses files and the uncommitted diff read-only; another Zode opens a project on it through a relay transport, with headless terminals over RPC. It is off by default. The relay routes opaque frames between devices of one account; Noise_KK_25519_AESGCM_SHA256 runs end to end, with `snow` in Rust and a hand-written Noise KK on WebCrypto in the browser so the browser key can stay non-extractable. First contact is a commit-reveal pairing with six digits confirmed on the host.

## What Review Caught

Every component went through split reviews (one reviewer per area; a single reviewer over the whole diff stalls). Most areas had at least one critical defect that the tests had passed over:

- **Host pairing:** a reveal from a foreign session while a prompt was open wiped the pending exchange without counting a failure. Fixed by refusing anything but a matching session in `AwaitingReveal`.
- **Terminal mirror:** chunks already drawn by a snapshot were replayed after it, because the snapshot's sequence counted chunks still in the tap channel. The queue now drops anything at or below the snapshot sequence.
- **Browser pairing:** the browser pinned whatever key finished the handshake, so a relay could answer as the host. The browser now requires its own "digits match" and checks the peer key against the key the account lists.
- **Headless terminals:** unbounded input buffering. Capped at 4 MiB.
- **File serving:** a FIFO could hang a request forever, the diff ran over the whole repository rather than the open folder, and repository-configured diff and textconv programs would have run. Fixed with a new `GitRepository::diff_head_to_worktree_scoped` (literal pathspecs, `--no-ext-diff --no-textconv`, output cut at 2 MiB with the child killed), a 30 s deadline per request, and refusal of private, excluded, symlinked and non-regular files. The final pass also found that a parent folder swapped for a symlink after the scan was followed; reads now resolve the path and require it to stay under the resolved worktree root.

Most fixes were checked by reverting them and watching the new test fail.

## Unrelated Crash Found on the Way

Tests aborted intermittently in wasmtime's macOS exception-handler thread, which aborts when a signal interrupts its `mach_msg` wait. `SIGCHLD` from exiting children was landing on it. Engines are now created inside `util::process::with_sigchld_blocked`, so the thread inherits a mask with `SIGCHLD` blocked: 0 aborts in 20 runs, against 9 in 20 with the helper made a no-op.

## Lessons

- A subagent's status report is a reading of reports, not of the code. The plan sync marked fixed critical defects as open because it read the review files and not the fixes; checking the code settled it.
- Claims in security docs drift towards the general ("runs no program from the repository"). The narrower true claim is no external diff or textconv; the repository's clean filters still run, as they do for any git status in Zode.

## Gates

`./script/clippy` exit 0. Tests: `remote_control` 180, `remote_relay_client` 120, `remote_relay_protocol` 45, `git` 50, `fs` 20, `workspace` 340, `project` 255 (four tests that hang on this machine skipped). `cargo check -p zode`, `rustfmt --check`, prettier exit 0. Web: frontend lint, 218 tests, build; backend lint, unit tests, build, and 113 e2e tests including the relay suite.

## Still Unverified

Typing latency on 4G, the `/remote` layout on a phone, a second Zode opening a project on a real second machine, and the relay deployed to zodekit.site. The terminal tap depends on a personal alacritty fork (`tgiap04/alacritty`). Accepted risks are listed in `docs/src/remote-control-security.md`.
