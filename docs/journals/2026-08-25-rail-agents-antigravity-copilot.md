# Rail agents: Antigravity and Copilot, and the 3000 lines behind them

**Date**: 2026-08-25
**Severity**: Medium — one shipped-red branch found, one security-adjacent guard fixed
**Component**: `project`, `agent_sessions`, `extension`, `extensions_ui`, `migrator`, CI gates
**Status**: Code complete, gates green, awaiting human sign-off on a settings migration

## What Happened

Two agents were added to the sidebar rail — Antigravity (`agy`) and GitHub Copilot
(`copilot`) — joining Claude Code and Codex. That part was small: four entries, two SVGs,
two match arms. Everything else in this session came out of pulling on threads attached to
it.

The plan asked for four things and got three. A Copilot session-history provider works and
reads real sessions. A Copilot usage meter turned out not to exist to be built. The
Antigravity half of both is blocked on a CLI nobody has installed. And a deletion estimated
at ~1140 lines came in at **−3009 across 31 tracked files**.

## The Brutal Truth

### The previous session shipped a red branch and I called it done

Yesterday's session ended with me reporting "code thì xong hết cả 8 phase", pushed. Today,
running the gates properly, `feat.release-v0.1.2` was failing **four** CI jobs — and three
of them were mine from yesterday:

| Job                                | Cause                                                                                                             |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `run_tests_{mac,linux,windows}`    | `test_action_namespaces` — the expected namespace list never gained `auto_update` after that feature was restored |
| `check_style` → `steps::prettier`  | the journal I wrote yesterday was not prettier-clean                                                              |
| `check_style` → `steps::cargo_fmt` | **skipped**, never ran — aborted after prettier failed. `auto_update.rs` and `terminal.rs` were unformatted       |
| `check_licenses`                   | `crates/auto_update{,_ui,_helper}/LICENSE-GPL` restored as regular 17-byte files instead of symlinks              |

The root cause is a single wrong assumption: I ran `./script/clippy`, it exited 0, and I
treated that as _the_ gate. It is not. `cargo fmt --all -- --check`
(`tooling/xtask/src/tasks/workflows/steps.rs:174`), `./script/prettier` and
`./script/check-licenses` are three separate gates that script never calls. I also never
looked at CI after pushing.

Two details worth keeping:

- **`cargo_fmt` showed as `skipped`, not `failure`.** Reading the job conclusions alone,
  `check_style: failure` looks like one prettier problem. The formatting gate had never run
  on this branch at all. Fixing only prettier would have turned fmt red on the next push —
  a second failure surfacing after "done".
- **The license symlinks passed a content check and failed a type check.** Restoring the
  `auto_update` crates from git history, I verified `app-icon.ico` by sha256 and read
  `LICENSE-GPL`'s contents, which were exactly `../../LICENSE-GPL`. Perfect content, wrong
  file type. `ls -la` would have caught in one second what `cat` could not catch at all.

### I wrote a comment that asserted a safety property I had not implemented

`CopilotProvider::contains` gates `paths_to_trash`, which feeds a delete-to-OS-trash flow.
I wrote:

```rust
/// Same guard as the Codex provider's: paths reaching a delete must not be
/// able to point outside the store just because a file on disk said so.
fn contains(&self, path: &Path) -> bool {
    path.starts_with(&self.session_state_dir)
}
```

It is not the same guard. `codex.rs` canonicalises both sides, and its doc comment says
exactly why: _"`Path::starts_with` is a component comparison that never touches the
filesystem: it says yes to `~/.codex/../../etc/passwd`, and yes to a symlink under
`~/.codex` pointing anywhere at all."_ The same crate had already paid to find and fix this,
and had a test covering `..`-climbing and symlink escape.

The weak guard is a defect. The comment is worse: it tells the next reader the property
holds, so they will not check. Fixed by canonicalising, and while writing the replacement
test I found a case the review had not raised — a path pointing at the **store root itself**
passed both the old and the canonicalised check, and would have trashed every session at
once.

### Adding two agents made an existing test spawn real processes

`every_agent_the_new_menu_offers_can_actually_be_opened` opens an `AgentView` per builtin.
With four builtins instead of two, it started aborting with GPUI's
_"Detected activity on thread `PTY reader` … Your test is not deterministic."_

Proof it was caused by the change, not a flake: clean HEAD aborted 0 times in 2 runs, the
changed tree aborted 3 times in 3. Cause: in tests `environment.rs:71` hands back an empty
environment, so `locate_binary` falls back to the **process** `PATH`, finds whichever agent
CLIs the developer happens to have installed, and `create_terminal_task` builds a real pty
whose reader thread the test scheduler does not own.

So the test's behaviour depended on the machine it ran on — different on a laptop with
`claude` installed than in CI without it. `resolve_agent_binary` now treats "no PATH from
the project environment" as not-installed under `cfg(test)`. The `agent_ui` suite went from
1.93s to 0.36s because it stopped launching real programs.

### The deletion was three times bigger than the estimate

The plan measured the ACP-registry removal by grepping struct names: ~1140 lines. The real
cascade, driven by the compiler rather than by grep:

`LocalExtensionArchiveAgent` + `LocalRegistryArchiveAgent` + `LocalRegistryNpxAgent` (466) →
their three registration sites → `AgentRegistryStore` (614) → the archive-download helpers
(86) → `ExtensionAgentEntry`, `sync_extension_agents`, the `extension_agents` state field
(197) → `AgentServerStore::local` losing three parameters and three call sites → the
`Extension`/`Registry` variants of `CustomAgentServerSettings` in **two** crates → the
`agent_servers` extension-manifest field → `ExtensionCategoryFilter::AgentServers` → the ACP
registry upsell → `crates/client/src/zed_urls.rs`, a whole module that existed only for that
one banner → two now-unused workspace dependencies → 556 lines of integration tests.

`agent_server_store.rs` went 2582 → 1246.

Two things the grep-based estimate could not have known:

- **`sync_extension_agents` had zero production callers.** The only two hits were _test
  function names_. `extension_host` never called it, so extension-provided agents had been
  inert long before this session — the feature was dead and tested at the same time.
- **A user-visible dead button.** `extensions_ui.rs` had a "View Registry" button
  dispatching `zed_actions::AcpRegistry`, an action with **no handler anywhere**. Clicking
  it fired telemetry and did nothing. That got its own fix, first, before any cleanup.

### Two things the compiler wanted to delete that must not be deleted

**`ExtensionProvides::AgentServers`** lives in `cloud_api_types` under
`#[serde(rename_all = "kebab-case")]`. It is a **wire enum** deserialised from the extension
registry API, which will keep returning `agent-servers` for extensions that declare one.
Removing the variant would fail the whole listing, not one row. Kept, relabelled
`"Agent Servers (unsupported)"`.

**The settings variants had to go, and that broke user files.** `CustomAgentServerSettings`
is `#[serde(tag = "type")]`, and `with_fallible_options` only makes `Option` _fields_
degrade — it does nothing for an unknown enum tag. One stale entry fails the entire
`agent_servers` map. Worse: `m_2026_02_25::migrate_builtin_agent_servers_to_registry` is a
migration that **wrote `"type": "registry"` into users' settings**. Anyone who simply kept
their settings across upgrades holds exactly the entries this change invalidates. Hence
`m_2026_08_24`, with five tests.

### A probe that ended in "no"

The Copilot usage meter was scoped as a timeboxed live probe allowed to conclude nothing
exists. It concluded nothing exists:

- `copilot --help` has no usage or quota flag or subcommand.
- `/usage` is described by the CLI itself as _"Display session usage metrics and
  statistics"_ — this conversation's tokens, not a subscription's remaining quota. TUI only.
- `copilot --acp` advertises `loadSession`, prompt capabilities and session listing, and
  nothing about an account. `account/rateLimits/read` — the method the Codex meter uses —
  answers `-32601 Method not found`, as do five other plausible names.
- `~/.copilot/logs/*.log` names one host, `api.individual.githubcopilot.com`, with zero
  quota, premium-request, entitlement or rate-limit vocabulary.

A GitHub REST route for premium requests may well exist, but reaching it means this editor
holding a GitHub credential — the exact trade the Codex meter was deliberately built to
avoid. Not pursued; written down in `docs/src/agent-usage.md` with all five probes so nobody
repeats them.

### Where the review helped and where it was wrong

The reviewer caught both real defects (the containment guard and the dead `BuiltinAgent`
fields). It was wrong twice, in opposite directions:

- It called `test_action_namespaces` a _"direct regression from the changes"_. Stashing every
  local change reproduced the failure, and CI had been failing it on three platforms since
  the day before.
- It graded the formatting drift _"cosmetic … not compile errors"_ and declined to run a
  formatter. It was a hard CI gate, and the one finding that would have kept the branch red.

It also asserted `claude.rs` swallows a failed count read where `copilot.rs` propagates.
`claude.rs:164` is `.transpose()?` — it propagates identically. No change made, because the
premise was false.

The tester similarly reported `test_generator_function_outline` as a failure worth triaging.
CI passes it on all three platforms and the final local suite passed it too: a stale
tree-sitter grammar artifact on one machine.

## Technical Details

Final state: **4436 tests run, 4436 passed, 16 skipped**. `./script/clippy`,
`cargo fmt --all -- --check`, `./script/prettier` and `./script/check-licenses` all exit 0.

The evidence gate returns **BLOCKED**, deliberately and correctly:

```
✗ inspection-verdict riskGate.signoffRequired is true but humanSignedOff is not true
  — a sensitive-area change (auth/secrets/deploy/migrations) cannot auto-finalize
    without a human sign-off
```

The change carries a settings migration. Flipping `humanSignedOff` would forge the single
thing that gate exists to check, so it stays false until a person says otherwise.

## What Is Still Not Done

- **Nobody has pressed a rail button in a running application.** Not Antigravity, not
  Copilot, not the install screen. Every claim about them rests on tests and on reading the
  launch path, and phase-01's own success criteria list the live check — it is recorded
  unchecked, not quietly ticked.
- **Antigravity has no session history and no usage meter**, and cannot be designed until
  `agy` is installed and signed in. Nothing is claimed about its on-disk format in either
  direction.
- **`docs/generated/entities.md:650`** still lists the removed `agent_servers` /
  `AgentServerManifestEntry` field. That is a generated artifact; refreshing it wants
  `/tkm:rebuild-spec --artifact entities`.
- **`ExternalAgentSource::Builtin` is now constructed nowhere**, since nothing registers a
  builtin into `external_agents` any more. Left in place: removing it would reduce
  `ExternalAgentSource` to a single variant, which reads worse than one dead one.
- **Agent-server extensions fail silently.** `ExtensionManifest` has no
  `deny_unknown_fields`, so a manifest carrying `[agent_servers.foo]` still installs and the
  stanza is simply ignored — no error, no warning at install time. The alternative, denying
  unknown fields, would break the whole extension over one obsolete stanza. The only signal
  a user gets is the `"Agent Servers (unsupported)"` label, and that is written down in
  `docs/src/extensions.md` rather than left for someone to discover.

### Antigravity keeps nothing on this machine

`agy` was installed and signed in after the first pass, which unblocked the phase that had
been waiting on it. The investigation closed it the other way.

`agy` 1.1.20, authenticated (proven by `agy models` fetching the list from the server), was
run twice — once with `-p`, once with `-c`. It answered both times, `-c` recalled the previous
turn, and it wrote **zero bytes** anywhere under the home directory or the working directory.
`strings` on the binary explains it: `#/v1/conversations/{conversation_id}` and
`backend.cachedConversation`. The conversations persist, server-side, behind a credential the
CLI holds.

That closes both halves of the phase, and not for want of effort:

- `SessionProvider`'s doc comment (`provider.rs:6-9`) says _"Every method is blocking: these
  read files and sqlite."_ There is no file. A provider would have to make an authenticated
  HTTPS call to Google on the session panel's `list()` path — that is changing the trait's
  contract, not implementing it.
- The usage half hits the same wall and adds the credential problem the Codex route was
  deliberately built to avoid.

The plan had named this exact scenario in advance — _"`agy` không ghi session ra đĩa"_ was
listed as a medium risk with the disposition "then there is no provider; write it down and
close". Writing the risk down before it happened is what made it a five-minute decision
instead of an argument.

Two prompts were spent on the user's own account to learn this. That is the whole cost of
the answer, and it is a better answer than any amount of reading a 179 MB Go binary's
strings would have produced.

## Lesson

`./script/clippy` exiting 0 is not "the gates pass". Read the workflow definitions and run
every gate they name — on this repo that is four commands, not one. And after pushing, look
at CI; a branch reported as finished while three jobs are red is worse than a branch
reported as unfinished.

When restoring files from git history, check the file _type_, not just the contents. A
symlink and a 17-byte text file holding the symlink's target are indistinguishable to `cat`
and obvious to `ls -la`.

A comment claiming a safety property is a load-bearing claim. If the code does not implement
it, the comment is the more dangerous half of the defect, because it stops the next reader
from looking.
