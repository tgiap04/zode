---
phase: 7
title: "Fix light survivors"
status: pending
effort: "1d"
---

# Phase 7: Fix light survivors

## Context Links

- [Survivor patch detail](./reports/scout-survivor-patches.md) §9–§13, §16
- [`research/survivor-fix-order.txt`](./research/survivor-fix-order.txt) — from Phase 1

## Overview

**Priority:** P1 · **Build state:** 🔴 RED · **Depends on:** Phases 5, 6

Six crates with small edges into the delete set, plus `file_finder` with a slightly larger one. Everything here is mechanical. Total: roughly 180 lines.

**`project` was added here by the red team** — the plan originally called it a zero-change survivor, which was wrong (see 7e).

Work in the reverse-topological order from Phase 1 and use `cargo check -p <crate>`, never `--workspace`. The point of this ordering is that each crate is fixed exactly once — errors flow downstream only.

## Key Insights

- `activity_indicator` imports `auto_update::DismissMessage`, which is **only an action declaration** inside an `actions!` macro (`auto_update.rs:97`). Declare it locally or in `zed_actions` — 4 lines.
- `diagnostics` uses `AgentSettings::get_global(cx).enabled(cx)` to gate one toolbar button. `AgentSettings::enabled` is literally `self.enabled && !DisableAiSettings::get_global(cx).disable_ai` — and `disable_ai` disappears too, so the branch is simply deleted, not rewired.
- `workspace` touches `agent_settings` at exactly two sites. `SidebarSide`/`SidebarDockPosition` come from the `settings` crate, not `agent_settings` — only the *value* lives on `AgentSettings`. Since `sidebar` is deleted, the whole `sidebar_side_context_menu` block goes rather than being rehomed.
- `file_finder` is the largest here (~110 lines): `Match::Channel` is matched in five places, so expect a compile-driven cleanup rather than a single deletion. Also drop the `FileFinderSettings::include_channels` setting — and remember Phase 9 must remove it from `default.json` in the same breath as its consumer.
- `language_tools` uses `EditPredictionStore::try_global` to add a Copilot branch to the LSP log view — delete the branch. `lsp_button.rs:1309` reads `language::language_settings`, **not** the deleted crate; leave it.

## Requirements

**Functional**
- Each of the seven crates checks clean in isolation.
- No behavioural change beyond removal of deleted-feature surfaces.

**Non-functional**
- One commit per crate so any single fix can be reverted independently.

## Architecture

Fix order (leaf-first; each depends only on Phases 5–6, not on each other):

```
1. activity_indicator   ← auto_update::DismissMessage        ~6 lines
2. diagnostics          ← agent_settings                     ~5 lines
3. language_tools       ← edit_prediction::EditPredictionStore ~15 lines
4. notifications        ← channel        (done in Phase 6c — verify only)
5. project              ← agent_servers settings key         ~30 lines   [red team]
6. workspace            ← agent_settings                     ~10-40 lines
7. file_finder          ← channel::ChannelStore              ~110 lines
```

## Related Code Files

**To modify**
- `crates/activity_indicator/src/activity_indicator.rs` `:1`, `:297`, `:525`, `:554`; `Cargo.toml`
- `crates/diagnostics/src/toolbar_controls.rs` `:2`, `:51`; `Cargo.toml`
- `crates/language_tools/src/lsp_log_view.rs` `:2`, `:346`; `Cargo.toml`
- `crates/workspace/src/welcome.rs` `:7`, `:427`; `crates/workspace/src/multi_workspace.rs` `:22`, `:65-100`; `Cargo.toml`
- `crates/file_finder/src/file_finder.rs` `:7`, `:398`, `:465-466`, `:672`, `:834-843`, `:976-1033`, `:1154-1158`, `:1548-1551`; `Cargo.toml`
- `crates/settings_content/` — remove `FileFinderSettings::include_channels`
- `crates/project/src/agent_server_store.rs` `:1896`; `crates/project/src/project.rs` `:1094` — red team finding 5

## Implementation Steps

### 7a. `activity_indicator`

1. `DismissMessage` is an action, not logic. Move the declaration into `zed_actions` (preferred — it is where cross-crate actions live) or declare it locally in `activity_indicator`.
2. Update `:1`, `:297`, `:525`, `:554`. Drop `auto_update` from `Cargo.toml`.
3. `cargo check -p activity_indicator`.

### 7b. `diagnostics`

4. Delete `use agent_settings::AgentSettings;` `:2` and the `is_agent_enabled` binding `:51`, plus the button branch it gates in `render`.
5. Drop the dep. `cargo check -p diagnostics`.

### 7c. `language_tools`

6. Delete `use edit_prediction::EditPredictionStore;` `:2` and the `EditPredictionStore::try_global` branch at `:346`.
7. **Leave `lsp_button.rs:1309` alone** — it reads `language::language_settings`, a surviving crate.
8. Drop the dep. `cargo check -p language_tools`.

### 7d. `notifications` — verification only

9. Already gutted in Phase 6c. Confirm: `cargo check -p notifications` green and `rg -n "channel::" crates/notifications/` empty.

### 7e. `project` — the "zero-change survivor" that isn't (red team finding 5)

The plan originally listed `project` as needing no changes. That was wrong on two counts:

| Site | Issue |
|---|---|
| `crates/project/src/agent_server_store.rs:1896` | `let agent_settings = content.agent_servers.clone().unwrap();` — an **unwrap on a settings key** whose owning crates (`agent_servers`, `agent_ui`) are deleted |
| `crates/project/src/project.rs:1094` | `DisableAiSettings` / `disable_ai` — the AI kill-switch setting, now moot |

9a. Decide the fate of `agent_server_store.rs`. Its consumers (`agent`, `agent_ui`, `agent_servers`) are all deleted, so like `context_server_store` it becomes an orphaned subsystem. **Recommended: delete the file and its wiring in `project.rs`** rather than keeping an unwrap on a key nobody sets.
9b. If instead the `agent_servers` settings key is retained, the `.unwrap()` at `:1896` **must** be made total — an unwrap on a key removed from `default.json` is a startup panic (the exact trap Phase 9 is built around).
9c. Remove `DisableAiSettings` / `disable_ai` (`project.rs:1094`) and coordinate the `default.json` entry removal with Phase 9.
9d. `cargo check -p project`. Record the `agent_servers` decision — **Phase 9's settings table depends on it.**

### 7f. `workspace`

10. `welcome.rs:427` — delete the `ai_enabled` conditional and whatever it gated.
11. `multi_workspace.rs` — since `sidebar` is deleted, remove `sidebar_side_context_menu` (`:65-100`) and the `AgentSettings::get_global(cx).sidebar_side` read at `:69`. Leave the `WorkspaceSidebar` **trait** in place if removing it ripples; nothing implements it after Phase 4, which is harmless.
12. Drop `agent_settings` from `Cargo.toml`. `cargo check -p workspace`.
    > `workspace` is depended on by nearly everything — get it green before moving to Phase 8.

### 7g. `file_finder`

13. Delete `use channel::ChannelStore;` `:7` and the `channel_store` field `:398`.
14. Delete the `Match::Channel` variant `:465-466`. Then let the compiler find every match site — expect `:672`, `:1154-1158`, `:1548-1551`.
15. Delete the field init and `include_channels` read at `:834-843`, and the channel-matching block `:976-1033`.
16. Remove `FileFinderSettings::include_channels` from `settings_content`. **Record it for Phase 9** — the `default.json` entry must go in the same commit as its consumer, or startup panics on the `.unwrap()`.
17. Drop the `channel` dep. `cargo check -p file_finder`.

### 7h. Gate

18. All seven check clean individually.
19. Re-run `research/final-delete-set.py`; the survivor list should be down to the heavy five: `zed`, `settings_ui`, `title_bar`, `git_ui`, `onboarding`.
20. Seven commits, one per crate.

## Todo List

- [ ] 7a `activity_indicator` — `DismissMessage` rehomed
- [ ] 7b `diagnostics` — toolbar branch removed
- [ ] 7c `language_tools` — Copilot LSP-log branch removed; `lsp_button.rs:1309` untouched
- [ ] 7d `notifications` verified green
- [ ] 7e `project` — `agent_server_store.rs:1896` unwrap resolved; `disable_ai` removed; **decision recorded for Phase 9**
- [ ] 7f `workspace` — sidebar context menu removed; **green before Phase 8**
- [ ] 7g `file_finder` — `Match::Channel` fully unwound
- [ ] 7g `include_channels` removal **recorded for Phase 9**
- [ ] `final-delete-set.py` shows only the heavy five remaining
- [ ] Six standalone commits

## Success Criteria

- `cargo check -p activity_indicator -p diagnostics -p language_tools -p notifications -p project -p workspace -p file_finder` all green.
- No `agent_settings`, `auto_update`, `channel`, or `edit_prediction` reference remains in any of them.
- `include_channels` is on the Phase 9 worklist.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| Fixing out of topological order | Same crate revisited; wasted `cargo check` cycles | Follow `research/survivor-fix-order.txt` |
| Using `cargo check --workspace` in this loop | Drowning in errors from the still-unfixed heavy five | Use `-p` exclusively until Phase 11 |
| `include_channels` removed from `settings_content` but not `default.json` | **Startup panic** via `.unwrap()` | Step 16 explicitly hands it to Phase 9 |
| `workspace` left red | Blocks everything in Phase 8 | Step 12 makes it a gate |
| Deleting `WorkspaceSidebar` trait causes an unexpected ripple | Extra unplanned work mid-red-period | Step 11 says leave the trait; unimplemented traits are harmless |

## Security Considerations

None specific. `file_finder` losing channel search removes a feature that queried the collab server — consistent with the privacy goal.

## Next Steps

Phase 8 — the heavy five: `zed` (23 edges), `settings_ui` (7), `title_bar` (4), `git_ui` (3), and `onboarding` (symbol-level, invisible to the graph).
