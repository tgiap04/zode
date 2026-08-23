# Agent session history — consultation record

**Date:** 2026-08-22 · **Lens:** CTO · **Level:** medium · **Status:** design sealed, not implemented

## The commission

Clicking the agent-history entry in the tool dock's header opens a searchable list of past
agent sessions, grouped by project, with per-row actions (resume, fork, copy, open/reveal log,
delete) and a hover toolbar — for **both** Claude and Codex. The header entry's glyph becomes an
asteroid.

Reference UI supplied as three screenshots: grouped list with counts, per-row title/preview/
agent/msgs/subagents/time/model, `Current worktree` / `Unavailable worktree` badges, branch chip,
hover buttons (play, plus-box, chevron, ellipsis), and a 9-entry ellipsis menu.

## What the material actually is (verified on this machine, 2026-08-22)

| | Claude | Codex |
|---|---|---|
| Source | `~/.claude/projects/<cwd-encoded>/<uuid>.jsonl` — **46 files, 221 MB** (see the correction below) | `~/.codex/state_5.sqlite`, table `threads` |
| Title | last `{"type":"ai-title","aiTitle":…}` line; **fallback** = first user message | column `title` |
| Preview | last user/assistant message | `preview` / `first_user_message` |
| Model | `message.model` (e.g. `claude-opus-5`) | `model` |
| cwd, branch | on every user/assistant line | `cwd`, `git_branch` |
| Subagents | count of `<session-id>/subagents/*.meta.json` | `thread_spawn_edges` |
| Message count | requires a full file scan (`wc -l` on 13 MB = 17 ms) | not stored (`tokens_used` only) |
| Resume | `claude --resume <id>`, `--fork-session` for a fork | `codex resume <SESSION_ID>` |
| Rows present today | 46 session logs, 45 with conversation | **0 at design time — since populated, see below** |

### Two corrections found while building

- **The session count was wrong here: 46, not 292.** `find ~/.claude/projects -name '*.jsonl'`
  descends into each session's `subagents/` directory, so it counted subagent transcripts as
  sessions. The session-level glob is `*/*.jsonl` — 46 files, 221 MB, of which 45 carry any
  conversation (one holds an `ai-title` and nothing else). Every performance number that leaned on
  292/408 MB was therefore pessimistic; the real sweep measures ~355 ms.
- **`has_user_event` cannot be used as a Codex filter.** The first real thread — typed into by hand —
  records `has_user_event = 0`. Filtering on it, as this report originally planned, would have hidden
  every real session. This is exactly what "do not build the provider blind" was for.

Two traps found while reading:

1. **The encoded directory name is not reversible.** `-Users-tgiap-dev-devs-zode` cannot be split
   back into a path: `-` is both separator and a legal directory character (`devs/zode` and
   `devs-zode` encode identically). Group keys must come from the `cwd` field inside the file.
2. **The reference app's own title rule is visible in the screenshot.** The row reading
   `<local-command-caveat>Caveat: The messages…` is a raw first user message, not an AI title —
   confirming `ai-title` with a first-message fallback.

`agent_ui::agent_view::agent_task()` (`agent_view.rs:614`) already builds the terminal spawn with
`args: Vec::new()` and `cwd` from the first visible worktree. Resume is that function plus args and
a different cwd — the launch path needs no redesign.

## Decisions

| # | Question | Chosen | Why |
|---|---|---|---|
| 1 | Where it lives | **Panel docked right** | Real `Panel`, so its icon reaches the dock header through `Panel::icon` and the disabled placeholder in `dock.rs` is deleted rather than grown. Matches the screenshot's column width. |
| 2 | Codex with no data | **User runs `codex` once first** | The only way the provider is written against observed data instead of a guess. |
| 3 | v1 scope | **Full parity with the screenshots** | Including msgs, subagents, worktree badges, branch chip, grouped counts. |
| 4 | Delete | **OS Trash** | Recoverable, and reuses the trash path the project panel already has. Never `rm`. |
| 5 | Expensive metadata | **Lazy, per visible row** | `msgs`/`subagents` are display-only — nothing sorts or searches on them. `uniform_list` renders ~20 rows, so ~20 file scans (~340 ms) instead of 408 MB. No on-disk cache, no invalidation layer, no migration. |
| 6 | "Resume in Worktree" | **Terminal at the session's own cwd** | `claude --resume <id>` / `codex resume <id>` with `cwd` = the path recorded in the log. Path gone → action disabled and the row shows `Unavailable worktree`. Creating a git worktree is a separate feature, not a button. |
| 7 | Opening history | **Takes the column turn-by-turn** | Default dock behaviour (`activate_panel`). Two vertical lists sharing one column's height serves neither. |
| 8 | Row chevron | **Longer preview + full metadata** | Session id, log path, full cwd, created-at. Costs nothing beyond the tail already read. |

## The shape

```
crates/agent_sessions/          (new, headless, testable without a window)
  provider.rs   trait SessionProvider { list, counts, resume_command, delete }
  claude.rs     tail+head reader over ~/.claude/projects
  codex.rs      read-only sqlite over ~/.codex/state_*.sqlite (highest suffix wins)
  summary.rs    SessionSummary { id, agent, title, preview, cwd, branch, model, updated_at }

crates/agent_ui/src/session_history/   (the panel)
  panel.rs      Panel impl: position Right, icon Asteroid, toggle action
  list.rs       uniform_list of groups + rows, fuzzy search over title/preview/branch/cwd
  row.rs        hover toolbar, chevron expansion, ellipsis ContextMenu (9 entries)

assets/icons/asteroid.svg       16×16, stroke="black", stroke-width 1.2 (house style)
```

**Reading strategy.** `list()` reads a bounded tail (256 KB, grown to 1 MB while hunting an
`ai-title`) plus a 16 KB head per session — enough for title, preview, model, cwd, branch, both
timestamps. `counts()` is called only for rows the list actually renders, on the background
executor, memoised in RAM by session id; the row shows `…` until it lands.

**Freshness.** Re-`stat` all 292 files when the panel becomes visible (microseconds) and re-parse
only what changed, plus an explicit refresh button. No fs watcher in v1 — the live session's file
grows on every turn and would produce a redraw storm for a number nobody is watching.

## Build order

Three pieces, and they are not one feature. (b) does not depend on (a) if it is built against a
fixture list.

- **(a) Data layer** — provider trait + Claude reader + Codex reader. Headless tests against real
  fixture files. This is where the format risk lives.
- **(b) Panel + list** — Panel impl, asteroid icon, groups, search, row layout, chevron. Delete the
  placeholder in `dock.rs` here.
- **(c) Actions** — the 9 menu entries and the 3 hover buttons, resume/fork wiring through
  `agent_task()`, trash with confirmation.

## Risks

| Risk | Mitigation |
|---|---|
| Codex provider unverifiable — `threads` is empty | **Blocking prerequisite:** run `codex`, say anything, exit. One row is enough. |
| `state_5.sqlite` carries a schema version in its *filename* — the next Codex release is `state_6` | Glob `state_*.sqlite`, take the highest numeric suffix. Missing table/column → provider reports *unavailable* and the panel shows Claude only, never panics. |
| Claude's log format is undocumented (`ai-title`, `subagents/`, `isSidechain`) and may change | Every field degrades: no `ai-title` → first message; no `subagents/` → hide the count; unparseable line → skip it. A format change must cost a column, not the panel. |
| "Open Log" on a 13 MB JSONL | Open as an ordinary editor item and let the editor's large-file path handle it; if it stalls, demote the entry to "Reveal Log". Decide with a measurement, not in advance. |
| Deleting the session that is running right now | Confirmation dialog naming the path and size. The panel cannot reliably know which file the live CLI holds. |
| Tail window misses `ai-title` in a very long session | Grow the tail to 1 MB, then fall back to the first user message — the same rule the reference app visibly uses. |

## Success criteria

1. Panel opens with 292 Claude sessions listed, grouped by real cwd, sorted most-recent-first, in
   under ~200 ms — measured, not asserted.
2. `msgs` and `subagents` for the visible rows match a hand-count on two chosen sessions.
3. `Resume in Worktree` on a live path opens an agent tab that continues the conversation; on a
   dead path the button is disabled and the badge reads `Unavailable worktree`.
4. Codex sessions appear in the same list with the same actions, verified against at least one real
   thread.
5. Deleting moves the `.jsonl` **and** its sidecar directory to the OS trash, and both are
   recoverable from there.
6. Removing `~/.codex/state_*.sqlite` entirely, or corrupting it, leaves the panel working with
   Claude sessions only.

## Unresolved

- Whether "Open Log" survives a 13 MB file in this editor. Measure during (c).
- Group collapse state: remembered per workspace, or reset each session. Trivial either way; decide
  when the rows exist.
