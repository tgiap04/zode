<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Business Rules (DRAFT)

**Project**: Zode (Zed fork)
**Generated**: 2026-08-07
**Status**: DRAFT — summarized from behavior-logic.md (207 BL items across custom-command,
integration, observer, scheduled-job, queue-worker categories), data-model.md, and architecture.md.

> **Source note**: This is a `generic-source`, no-screen-list profile pass covering background and
> system logic, not user-facing web-style business rules. No `BR-###`/`PERM###`/`BL###` codes
> appear below — this is the plain-language synthesis for a PM/BA/client audience. Organized by
> theme rather than one entry per BL item, since 207 items would otherwise repeat structure without
> adding insight at this altitude.

## Command / Action System

Nearly every user-facing operation in Zode — cursor movement, git staging, debugger stepping, vim
motions, panel navigation — is modeled as a discrete, named "action" (`ShowErrorMessage`,
`ToggleStaged`, `StepInto`, and roughly 250 others across the 100 custom-command items sampled).
Each action is independently bindable to a keystroke via the keymap file and independently
reachable from the command palette; nothing is hardwired to one specific key. A large, dedicated
namespace of actions exists solely for Vim emulation (motions, text objects, operator-pending
state, ex-commands, Helix-mode variants), reflecting that Vim mode is a first-class, deeply
modeled sub-system rather than a thin remap layer on top of the normal editor commands. A separate
shared-action crate (`zed_actions`) exists purely so leaf UI crates can reference common actions
(open settings, change font size, open a URL) without creating circular crate dependencies — this
is an internal engineering rule, not a user-visible behavior, but it explains why some actions
appear to "belong" to no specific feature crate.

## Extension System

Extensions are WASM-sandboxed and cannot perform three specific dangerous operations — running a
process, downloading a file, or installing an npm package — unless the extension's manifest
explicitly pre-declared exactly that capability (an allowlist of commands/hosts/packages, with
support for wildcard argument matching). This allowlist is fixed when the extension is authored;
there is no user-facing runtime prompt to grant additional capabilities on the fly. Separately,
extensions can be installed from a local directory for development ("dev extensions") and rebuilt
in place without restarting the app; the reload/rebuild flow runs the compile step in the
background and only updates the UI once the compile result (success or failure) is known. A
suggestion banner recommends installing a language extension when it detects a relevant file type
open with no matching extension installed; dismissing it ("No, don't install it" / "Don't Show
Again") persists that dismissal so the same suggestion doesn't reappear for that language.

## Workspace / Worktree Trust

Opening a new folder or file does not automatically permit Zode to spawn tooling (language
servers, certain git operations) against it — the folder starts untrusted, and trust must be
explicitly granted (once) through a security prompt before that tooling runs. Trust is
hierarchical: trusting a single file is the narrowest grant, trusting a directory covers
everything inside it, and trusting a parent directory transitively trusts every subdirectory and
file underneath it, present or future. Once granted, trust persists across restarts so the same
folder isn't re-prompted every session. A global opt-out setting exists to auto-trust everything
and skip the prompt entirely. Folders Zode creates for its own internal purposes (e.g. a scratch
directory backing the keymap editor) are never subject to this check since the user never opened
them as content. See [Permissions](permissions.md) for the full mechanism.

## Git Integration

Git state and mutation (status, diff, commit, branch, stash, blame, log, worktree management) is
sourced through a dual path: read-mostly queries go through an embedded `libgit2` handle, while
operations libgit2 doesn't cover well (status, most diffs, stash, ref updates, running
commit-message/pre-commit hooks) shell out to the system or bundled `git` executable. Mutating git
operations run on a background task so the UI never blocks on a git call. Hunk-level staging,
branch/stash pickers, a dedicated "Git Graph" commit-history view, and a project-wide diff view are
all built on top of this same integration layer. Adding a new git remote, committing staged
changes, and background diff recalculation after a buffer registers are all fire-and-forget
background operations — failures surface as a dialog/log entry rather than blocking the
originating UI action.

## LSP / Language Tooling

A worktree only gets a language server once it both (a) is trusted (see Workspace/Worktree Trust
above) and (b) actually needs one — triggered by opening a buffer in a detected language or
changing that buffer's assigned language. The server process is spawned as a child process
communicating over framed JSON-RPC on stdio, with a fixed 5-second graceful-shutdown timeout before
a harder kill. Several language servers get bespoke, non-standard protocol extensions handled
specially: the JSON language server can request Zode-authored schema content by URI instead of
receiving the full schema up front; rust-analyzer's non-standard health-status notifications are
mapped into a common health enum and logged; the Vue language server's embedded TypeScript
requests are proxied to a sibling, separately-registered TypeScript server and the response routed
back. Formatting via Prettier follows the same "start a server-like process" pattern, preferring a
worktree-local Prettier install and falling back to installing and running a bundled default
version on demand; all such formatter/language-server processes are torn down together when a
project becomes idle (see Hibernation Lifecycle below).

## Background Workers / Fire-and-Forget Operations

A large share of everyday UI interactions in Zode are backed by a detached background task rather
than a synchronous call, specifically so the UI thread is never blocked: workspace-item
serialization (persisting the state of open tabs — Git Graph, Project Diff, Image Viewer, Welcome
Page, Component Preview, terminal tabs — so they can be restored on next launch), remote-server
round-trips (restarting language servers, forwarding a breakpoint toggle, fetching the process list
for "Attach to Process", forwarding a buffer edit to a remote host), fire-and-forget SQLite writes,
and debounced recalculation work (git-diff recalculation on buffer registration, general debounced
delay timers used across several features) all follow this same shape: kick off the work in the
background, update UI state only when/if it completes, and log rather than propagate failures for
operations where blocking the user would be worse than a quietly-logged error.

## Hibernation Lifecycle (fork-specific)

Zode adds a multi-project window/hibernation lifecycle not present in upstream Zed. Each open
project carries an activity label — Active (its workspace is focused, never a hibernation
candidate), Warm (unfocused but still fully live, running on an idle timer), or Hibernated (idle
long enough to have been torn down). Only the visibly-active project's workspace is guaranteed to
be fully live; unfocused projects are hibernated after an idle timeout to save memory, with
per-workspace hibernate timers that get cancelled the moment the workspace regains focus. A project
with an active debug session or an in-flight autosave is not force-hibernated — the transition is
deferred and retried later instead, rather than forcing dirty work to be discarded. When a project
does hibernate, dependent state is deliberately shrunk rather than discarded: a terminal's
scrollback history is trimmed to save memory (and the pre-hibernate size is remembered so it can be
restored), the file-tree sidebar keeps a stale-but-visible diagnostic count for the paths it can no
longer verify (shown dimmed rather than removed), and the background file-scanner is explicitly
paused for any project sitting at Warm or Hibernated so it doesn't do wasted work against a
defocused project. Important caveat carried over from this repository's own internal notes: the
activity label is not a real-time resource-state oracle — a project can report `Hibernated` while
the underlying LSP/scanner/terminal resource teardown is still in flight behind a deferred barrier,
so any consumer of this field should treat it as an intent/label, not a guarantee that resources
have actually stopped.

## Settings Precedence

When the same setting is defined in more than one settings file, project-level settings win over
server-provided settings, which win over user/global settings, which win over built-in defaults.
Any change to any settings file triggers automatic re-parsing and re-application of the full
precedence chain; a parse error in one file is tracked against that file specifically rather than
failing the whole settings load.

## File Conflict Detection

If a file open in the editor changes on disk without a corresponding save from within Zode itself
(e.g. edited by another program, or `git checkout` touching the working tree), the system flags the
buffer as conflicted and surfaces a notice to the user rather than silently overwriting either the
on-disk or in-memory version.

---

## Limits / What This Draft Does Not Cover

- No feature-list or user-facing feature specs exist yet for this repo in this wave
  (`generic-source` profile, no `--feature-specs` pass run) — these rules are system-level, not
  tied to any F### feature.
- `behavior-logic.md`'s 207 BL items are drawn from a large, multi-hundred-crate codebase; several
  categories here (Vim emulation, DAP/debugger session rules, per-vendor LLM/agent integration
  quirks, notebook/Jupyter kernel handling) were only lightly sampled and likely hold additional
  rules not surfaced by this pass.
- The observer/scheduled-job/queue-worker BL items (BL124–BL207) were consulted for the themes
  above but not individually enumerated here — see behavior-logic.md directly for per-item detail
  if a specific background-worker rule is needed.
- This draft should be reviewed against `docs/decisions/ADR-*.md` (human-authored), if/when those
  exist, since ADRs often encode the *why* behind a rule that source alone cannot show — in
  particular, why the fork-specific hibernation lifecycle and worktree-trust model were added on
  top of upstream Zed.
