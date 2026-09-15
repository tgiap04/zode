# Agent lists survive a checkout switch, and one panel's doors stop reopening

**Date**: 2026-09-13 15:30
**Severity**: Medium — two reported bugs fixed, both in user-facing behavior; one independent fix that changes restore behavior deliberately
**Component**: `git_ui` (`branch_panel/`), `workspace` (`dock.rs`)
**Status**: Resolved (commits on `feat.drag-to-reorder-stacked-dock-panels`, phases 01–05 implemented and tested)

## What Happened

Two user reports landed hours apart. A checkout's open agent list snapped shut every time you switched branches. And the "open this list" control itself vanished while the answer was still loading—a different moment but the same root: two state sources at different scopes stepping on each other when a workspace switched. A third defect, pre-existing and latent, was also closed: a panel the user had deliberately closed reopened on every restart whenever `starts_open: true` applied to it.

## The Brutal Truth

**Defect 1 is genuinely maddening:** You open a worktree's agent list, switch branches, and the list you just opened is gone. It wasn't cleared by a restart or a stale record — it's gone the moment you land. The instinct "the record is keyed wrong" was exactly right, but the diagnosis missed which component of the key was the culprit. A checkout is a path on disk. A workspace is a window's state for opening that path. When you switch, a new workspace is built (because the destination path was not already open), that workspace gets a new `WorkspaceId`, the panel reads a record keyed by `BranchPanel-{workspace_id}`, and finds a different drawer.

The fix does not require a third key or a migration path that readers must detect. The record moves from `BranchPanel-{workspace_id}` to a single global `BranchPanel`, keyed only on path. One process-global entity owns it — following `SessionStore`'s precedent — because two panels live at once in a `MultiWorkspace` and two private copies would clobber each other's writes. Legacy records stay on disk undeleted, readable as a fallback while the shared record has never been found, so a downgraded build still finds its state.

**Defect 2 sting is different, more subtle:** The list comes back, sure, but for a frame or two the control itself _vanishes_. A moment later it reappears. This was not a rebuild cycle or a docked status — it was a blink-out. The reason splits into two parts. Workspace-local tabs (open agents) are only visible to that workspace. After a switch, the new panel cannot see the agent running in the old workspace. The session index (process-global but eventually consistent) takes time to re-read disk when a new panel is created. So there is a real window where the list is empty — and `build_rows:79` had a contract: `expanded: has_agents && expanded(..)`. An empty list forces closed, and `render_tree/agent.rs:31` had another: if the control's list is empty, return `None`. The control deleted itself rather than closing.

The fix holds the last non-empty list in memory while the session index re-reads. A separate pass before `build_rows`, gated on whether a scan is in flight: if yes and the result is empty, use the held list; if no and the result is empty, drop the held entry and believe the emptiness. The held entries live in memory only, not persisted to disk — the key-value store describes what _the reader decided_, not what _the panel last saw_.

**Defect 3 was latent until configuration made it live:** A panel with `starts_open: true` added _after_ a dock restore would force itself visible by setting `entry.visible = ix == panel_ix` for every entry (`dock.rs:1096-1098`). On a stacking dock like the left rail, this collapses the whole column to that one panel. The 2026-09-11 journal said this was dormant because only `project_panel` had `starts_open: true` and it was always added first. That's false now: five panels override it, three read it from a setting, and three of those sit on the stacking left dock. A user who sets `"branch_panel": { "starts_open": true }` in their config gets this today. The fix: gate the force-open on a fresh workspace. The line checks `restore_state`'s return value (which was previously discarded) — if a record was found, honor it instead of `starts_open`. One line, but changes restore behavior for every workspace, so it lands alone.

## Technical Details

### The evidence is in the code

**Defect 1:**

- `state.rs:122-128` — the old key: `format!("{BRANCH_PANEL_KEY}-{id:?}")` over `workspace.database_id()`
- `checkout_state.rs:46-56` (new) — the struct holds `expanded`/`collapsed`/`pinned`/`order` once for the process
- `lifecycle.rs` (modified) — `BranchPanel::load` awaits `CheckoutViewState::load(cx)`, which reads `BRANCH_PANEL_KEY` without an id suffix

**Defect 2:**

- `data.rs:173-184` — open tabs come only from `self.workspace.items_of_type::<ItemView>`
- `session_store.rs:94-115` — the index's `refresh` sets `scanning` synchronously inside the method
- `panel.rs` (modified) — `last_known_agents: HashMap<Arc<Path>, Arc<[AgentEntry]>>` field, initialized in `new`
- `lifecycle.rs:hold_known_agents` (new method) — checks `index_is_settling(cx)` and replaces or drops entries per the rule
- `data.rs:observe_agent_tabs` (modified) — compares pre-removal open agent count against live `AgentView` count; requests `session_store.refresh(cx)` only on a decrease

**Defect 3:**

- `dock.rs:997-1015` — `let restored = self.restore_state(..); if !restored && panel.read(cx).starts_open(..) { activate_panel(..) }`
- `workspace.rs:14668–14700` (test harness) — `restoring_a_dock_does_not_read_the_workspace_it_is_inside` already had the setup for testing this

### Two hypotheses from the plan were disproved by reading source

**Hypothesis 1:** "The `RepositoryId` in the stored key changes on every switch, and nothing matches anymore."

**Reality:** The switch never gets as far as re-adopting or comparing keys at all. A new workspace reads a wholly different record (keyed by its own `workspace_id`), so the runtime `RepositoryId` is downstream noise, not the root. The fault is the workspace id in the key, exactly as suspected — but the original diagnosis of which component was wrong.

**Hypothesis 2:** "`session_store: None` while a refresh is queued means `agents_by_checkout` returns only workspace-local tabs, and the disk index is not yet consulted."

**Reality:** `session_store` is set synchronously by `ensure_session_store` before `refresh_if_stale` ever runs (`lifecycle.rs:428`). It has never been `None` in a production render path. The real mechanism: after a switch, a new panel is built, `ensure_session_store` fires a scan, the scan is in flight while `agents_by_checkout` runs, and the result is empty because the index does not yet know about that session. This is a timing problem, not an initialization problem. Phase 3 (detecting tab closes and requesting a re-scan) revealed a second, switch-free path to the same emptiness: closing the last agent tab marks the tree stale but does not request a scan, so if the index has not yet read it, the list empties and the control vanishes with no switch.

Recorded in `reports/study-corrections.md`, with call sites verified against source.

### Test count

Started at git_ui 202, workspace 291, agent_ui 95. No new file regressions; test suite covers all five phases.

### Verification — each fix was neutralised in place and the matching test watched to fail

- **Defect 1:** Shared record made per-panel again (`checkout_state` reverted to keying on workspace). Two tests failed: `a_second_panel_sees_what_the_first_opened` and `a_pin_set_in_one_panel_is_visible_in_another`.
- **Defect 2:** Hold disabled (replaced with unconditional merge). `a_held_list_is_not_empty_while_the_index_settles` failed with `0 vs 1`. Hold made unconditional instead of gated on scan. `a_checkout_with_no_agents_stays_empty` failed.
- **Defect 3:** `!restored` gate dropped. `a_restored_dock_is_not_collapsed_by_a_panel_that_starts_open` failed with `visible == false`, expected `true`.

Each failure reproduced with the predicted assertion. `./script/clippy` exit 0.

### What was not verified

**No manual in-app verification has been performed.** The five phases of automated testing passed. The journal records only what the tests saw, not what a human in the UI would observe.

## Root Cause Analysis

### Defect 1 — the record was scoped to the ephemeral, not the persistent

A checkout is a path on disk: a fact of the repository. A workspace is a window's state for a moment in time. The record of what the reader decided about a checkout should outlive the workspace it was decided in. The original design keyed it by workspace — reasonable if you thought of the record as "the panel's state", but wrong if you thought of it as "what the reader decided about this checkout". A description that spans multiple workspaces cannot live in any one of them.

This is a scope mismatch, not a missing piece. The fallback to legacy records (seeding from per-workspace blobs) shows the intent was always there — the only live copy just had the wrong owner.

### Defect 2 — conflating "empty" with "done"

An empty list from `agents_by_checkout` can mean three things: the index hasn't read the session yet, the workspace doesn't see that agent tab (wrong scope), or the checkout genuinely has no agents. The third is the only one the control can safely interpret as "keep the entry closed." The first two are timing, not absence. The original design treated all three the same and let the control blink out.

Holding the last known list until the index settles is admitting that emptiness is provisional while a scan is in flight. It is not rethinking the logic — it is straightforwardly applying "believe empty answers from a source you trust, and distrust empty answers from a source that is still warming up."

### Defect 3 — a default that overrides explicit choice

`starts_open` exists for workspaces that have never been customized. Once a workspace has a record, that record is explicit choice — what the user actually did. Honouring `starts_open` on top of an existing record is disrespecting the choice already made. One line gates on "did we find a record?" — "no" means default in, "yes" means default out.

## Lessons Learned

### 1. One set is unstatable to break in ways two sets can break

The original design had two sets: `stored_expanded` (path-keyed, persistent) and `expanded` (repository-id-keyed, ephemeral). `adopt` copied the first into the second on every rebuild. Closing a row is a rebuild, so without consumption the restored entry reopened instantly — consumption was the patch, two sets was the fault.

With one shared path-keyed set, "a restored row can be closed and stays closed" becomes true by construction. The property is not re-implemented: it is unstatable to break without reintroducing a second set. Write the property into the design, not the code.

### 2. Scope mismatch is discovered by asking "who owns this fact?"

The record describes what the reader decided about a checkout. A checkout is not a workspace. A workspace is not even the right owner for per-workspace state once a `MultiWorkspace` keeps two panels alive at once. Asking "what object owns this thing?" earlier would have named the mismatch: the record is about checkouts, so the entity holding it must be scoped to the process, not the window.

### 3. Emptiness has shades, and you cannot believe all of them equally

An empty result can be "nothing exists" (believe it) or "the source is still gathering the answer" (don't). If two sources feed the same view and they have different scopes, you need two different trust levels. Holding the result from the eventual-consistency source while it is still settling is not a hack — it is correctly modeling confidence in the answer.

### 4. A gate that depends on load ordering is a trap waiting for the next feature

Phase 2 R1 notes that `ProjectPanel::load` completing before `BranchPanel::load` inside a `futures::join!` is what stops the restored dock from collapsing. That is an accident that happened to work because of a database read inside one of the futures. It is not in the code, it is not tested, and it is not documented anywhere in the implementation. Phase 5 closes it by not relying on the accident at all — one line of production code to check whether a record was found. The lesson: if the only thing holding an invariant is load ordering, make the ordering irrelevant instead of guarding against it.

### 5. A negative assertion about something negative by default measures nothing

`a_restored_open_row_can_be_closed_and_stays_closed` ended by reading the record back from disk and asserting:

```rust
assert!(!state.is_open(&stored), "a fresh read must still show it closed");
```

It read rigorous. It was not. `is_open` answers `false` for a key that was **never recorded** exactly as readily as for one recorded closed, so the assertion held just as well against a `toggle` that wrote nothing at all. Deleting the single `schedule_write` line left the test green.

The fix was a _positive_ read earlier in the same test — the record must first be seen to come back **open** — because that is the only assertion that distinguishes "persisted correctly" from "persistence absent". The general shape to distrust: any assertion satisfied by the empty, absent, default or zero value of what it is inspecting. It is not testing the mechanism; it is agreeing with the default.

The method that caught it is cheap and should be routine: **delete the mechanism, re-run the test, and require it to fail.** A test never watched to fail is a hypothesis, not evidence.

### 6. Making state global makes test isolation load-bearing where it was not

One panel test failed once and did not reproduce. It was first reported as a pre-existing flake. It was not pre-existing and it was not unrelated: `AppDatabase::global(cx)` falls back to a **process-wide static** when the App has no database global (`crates/db/src/db.rs:80-89`), and this panel's harness set none.

That was harmless for as long as those tests never touched the key-value store. Moving the record to one un-scoped `"BranchPanel"` row made every test in the binary read and write the same row, and the suite became order-dependent overnight. The fix is one line the repo already uses in four other harnesses — `cx.set_global(db::AppDatabase::test_new())`.

Two things to carry forward. First: when state is promoted from per-window to per-process, every test harness that touches it silently acquires a new requirement, and nothing in the compiler will say so. Second, and more general: **"pre-existing flake" is a diagnosis, not a description.** It requires the same evidence as any other root cause, and here the timing of its first appearance — immediately after the state went global — was itself the clue.

## Next Steps

All implementation is complete. Automated testing passed.

- **Manual testing needed:** In-app verification of all three defects with real switches and agent launches
- **Phase 5 release note:** Document the behavior change: "A panel you had closed now stays closed across restarts, where it previously reopened if configured with `starts_open: true`."
- **Possible future:** The 2026-09-11 journal notes a second defect in `activate_panel` — on a fresh workspace with two panels both reporting `starts_open: true`, the last-added still hides the first. This is a separate layer and out of scope here.

## Addendum: Correction to 2026-09-11 entry

The claim in `docs/journals/2026-09-11-dock-state-read-from-the-wrong-owner.md` § Latent #1, line 128 states:

> "Only `project_panel` defaults to `starts_open: true`. It does not bite today because it is always added first."

This is **no longer accurate.** Five panels override `starts_open`, three of them read it from a setting:

| Panel              | Source                           | Default               |
| ------------------ | -------------------------------- | --------------------- |
| `project_panel`    | setting + visible-worktree check | `true`                |
| `git_panel`        | setting                          | `false`               |
| **`branch_panel`** | setting                          | `false`               |
| `outline_panel`    | self.active                      | false at construction |
| `session_history`  | hard `false`                     | —                     |

A user who sets `"branch_panel": { "starts_open": true }` in their settings gets a `starts_open` panel added _after_ the project panel. Because the left dock stacks, it hides the project panel and any restored stack with it. This defect is reachable by user configuration today, not latent.

The original observation — that `starts_open` can collapse a restored stack — is correct. The claim that it is dormant is wrong as of the current build. Phase 05's `!restored` gate (commit hash to be assigned on merge) closes it.

The test counts recorded in 2026-09-11 as "780 tests green — workspace 268, git_ui 174" are stale. Current counts are git_ui 202, workspace 291, agent_ui 95.

## Correction — 2026-09-13 21:40

**This entry recorded defect 1 as resolved. The fix was real but incomplete, and the symptom
survived it.**

**What is true:** everything about the key's _location_. The record did move from
`BranchPanel-{workspace_id}` to one un-scoped `BranchPanel` key owned by a process-global
`CheckoutViewState`, two panels in a `MultiWorkspace` do share one record, and the legacy blob is
still readable. That work stands.

**What is false:** the conclusion that this closed the defect. The key's _contents_ were wrong in a
second, independent way. `StoredKey`'s repository component was `RepoData::path` —
`work_directory_abs_path`, which is the checkout the workspace is open at, not the repository. Both
panels shared one record and then looked up different keys in it, so the reader's open agent list
still closed itself on every switch.

The section "Two hypotheses from the plan were disproved by reading source" is worth re-reading with
that in mind. Hypothesis 1 — "the stored key changes on every switch, and nothing matches any more" —
was dismissed as "downstream noise". It was right about the mechanism and wrong only about which
component of the key carried it.

Root cause and fix: [2026-09-13-the-record-was-keyed-by-where-you-were-standing.md](2026-09-13-the-record-was-keyed-by-where-you-were-standing.md).
