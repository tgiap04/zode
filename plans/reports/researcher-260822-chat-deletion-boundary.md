# Deletion boundary — removing `AgentViewMode::Chat`

Read-only mapping. No skill from the mandatory preflight list fit this task (pure
codebase dependency archaeology, not technology comparison or external doc lookup)
— falling back to Grep/Read directly, noted per protocol.

## The decisive answer

**Terminal mode never touches ACP. It is purely a PTY.** `agent_view.rs::start()`:

```rust
if mode == AgentViewMode::Chat {
    this.update_in(cx, |this, window, cx| this.start_chat(window, cx)).ok();
    return;
}
let terminal = project
    .update(cx, |project, cx| {
        project.create_terminal_task(agent_task(&agent, binary, project, cx), cx)
    })
    .await;
```

Terminal resolves the CLI binary via `project::AgentServerStore::resolve_agent_binary`
(lives in the `project` crate, no ACP involved) and hands it to
`project.create_terminal_task` — a plain pty spawn. Chat's `start_chat` builds a
`crate::conversation_view::ConversationView` wired through
`crate::agent_connection_store::AgentConnectionStore`, which speaks ACP over an npx
adapter. Two disjoint code paths from the same `start()` branch, confirmed by the
file's own comment at `State::Chat`: *"The CLI itself speaks no ACP, so this mode
never runs the binary the terminal mode runs."*

**This is bigger than agent_ui.** `agent_ui`'s `Cargo.toml` depends unconditionally
on `acp_thread` and `agent-client-protocol`, but that dependency is exercised
**only** by the chat-only files. Tracing it out: the entire `agent_servers` crate,
the entire `acp_thread` crate, and the entire `acp_tools` crate have **zero
consumers anywhere in the workspace except `agent_ui`'s chat modules** (confirmed
by grepping every `Cargo.toml` for these three crate names — only `agent_ui`, and
`agent_servers` itself for the first two, show up). Deleting Chat orphans all three
crates outright, plus the `agent_settings` crate. The original ~13,000-line estimate
covered agent_ui alone; the real number is roughly **3x that**.

## File-by-file verdict (`crates/agent_ui/src/`)

| File | Lines | Verdict |
|---|---:|---|
| `agent_view.rs` | 2147 | **Shared** — dispatcher for both modes, needs surgery not deletion |
| `missing_binary.rs` | 217 | **Shared** — binary-resolution failure screen, used by both modes (binary resolution happens before the mode branch) |
| `agent_ui.rs` | 67 | **Shared** — crate root, trims `mod`/`pub use` list |
| `conversation_view.rs` | 2700 | Chat-only |
| `conversation_view/thread_view.rs` | 8591 | Chat-only |
| `agent_diff.rs` | 2282 | Chat-only (see Trap 1) |
| `completion_provider.rs` | 2628 | Chat-only |
| `message_editor.rs` | 1877 | Chat-only |
| `mention_set.rs` | 1222 | Chat-only |
| `config_options.rs` | 880 | Chat-only |
| `model_selector.rs` | 840 | Chat-only |
| `entry_view_state.rs` | 477 | Chat-only |
| `actions.rs` | 328 | **Mostly chat-only** — `RenameAgent` inside it is shared, must be extracted |
| `diagnostics.rs` | 252 | Chat-only (unrelated to the top-level `crates/diagnostics` crate — name collision only, not a dependency) |
| `agent_connection_store.rs` | 242 | Chat-only |
| `outline.rs` | 221 | Chat-only |
| `mode_selector.rs` | 211 | Chat-only — this is ACP *session* modes (plan/act), unrelated to `AgentViewMode` |
| `mention_image.rs` | 169 | Chat-only |
| `external_source_prompt.rs` | 162 | Chat-only |
| `model_selector_popover.rs` | 101 | Chat-only |
| `markdown_style.rs` | 40 | Chat-only |
| `ui.rs` | 24 | Chat-only (submodule root) |
| `ui/agent_notification.rs` | 198 | Chat-only |
| `ui/mention_crease.rs` | 281 | Chat-only |
| `ui/model_selector_components.rs` | 265 | Chat-only |
| `ui/hold_for_default.rs` | 52 | Chat-only |
| `ui/undo_reject_toast.rs` | 29 | Chat-only |

Every "chat-only" verdict above is backed by an internal `crate::` reference-map: I
grepped which agent_ui file references which module, and every one of these files
is reachable only through `conversation_view.rs`/`thread_view.rs`, which
`agent_view.rs` reaches only through the one `State::Chat(...)` arm. None is
imported from outside `agent_ui` (checked via `grep -rn "agent_ui::" crates/ | grep
-v /agent_ui/` — the only external hits are `AgentView`, `agent_ui::init`, and
`agent_ui::AgentDiffToolbar`, all handled below).

## Outside `agent_ui`

- **`crates/sidebar/src/rail_agents.rs`** — imports `AgentView`, `AgentViewMode`.
  Both call sites hardcode `AgentViewMode::Terminal`; the rail never opens Chat
  directly (Chat is reachable only through the in-tab toggle button rendered by
  `agent_view.rs::render_mode_switch`). Unaffected by removing the `Chat` variant.
- **`crates/zed/src/zed.rs`** — `agent_ui::init(cx)` (keep), and
  `cx.new(agent_ui::AgentDiffToolbar::new)` registered unconditionally into the
  editor toolbar (line ~1154). `AgentDiffToolbar`/`AgentDiffPane` operate on
  `Entity<AcpThread>` (from `acp_thread`), which only `ConversationView` ever
  creates — once Chat is gone nothing ever feeds it a thread, so it is dead weight
  registered every window open. Must be dropped along with the import.
- **`crates/zed/src/main.rs`** — `agent_ui::init(cx)` only. Keep.
- **`crates/project_symbols/src/project_symbols.rs`** — one **comment only**
  ("Note if you make changes to this, also change
  `agent_ui::completion_provider::search_symbols`"). `search_symbols` is
  `pub(crate)`, not reachable from `project_symbols` at all — zero compile
  dependency, just a stale comment to clean up.
- **`crates/zed_actions/src/lib.rs`** — `agent` module's `AgentViewMode`,
  `OpenAgent`, `ToggleAgent`, `NewAgent` all **stay** (Terminal mode's whole open
  path runs through them). Only the `Chat` variant of `AgentViewMode` is removed;
  `mode_name`/`mode_from_name` in `agent_view.rs` need their match arms trimmed
  (default fallback changes from `Chat` to `Terminal`, see Trap 3).
- **Keymaps** (`assets/keymaps/*.json`) — none of `agent_ui`'s own actions
  (`RenameAgent`, `Keep`, `Reject`, `OpenAgentDiff`, `NewThread`, etc.) have **any**
  keybinding at all. The only bound `agent::*` strings present are
  `AddSelectionToThread`, `PasteRaw`, `Chat`, `ToggleFocus` — these are
  **`zed_actions::agent`** actions (a different, separate `actions!` block, not
  `agent_ui`'s), dispatched from `terminal_view.rs` and
  `editor/mouse_context_menu.rs` as senders, with the only *handler*
  (`AddSelectionToThread` in `thread_view.rs`) living in chat-only code. Once Chat
  is gone these become dead clicks (context-menu entries that do nothing) — a UX
  cleanup, not a compile blocker.
- **`assets/settings/default.json`** — the `"agent": { ... }` block (~44 lines,
  `sidebar_side` through `single_file_review`) is read exclusively by
  `AgentSettings::get_global`, which is read exclusively by
  `conversation_view.rs`/`thread_view.rs`/`agent_diff.rs`/`message_editor.rs` — all
  chat-only. Zero reads anywhere else in the repo (confirmed by grepping every
  `AgentSettings::get_global` call site). The sibling `"agent_servers": {}` and
  `"context_servers": {}` blocks are **unrelated** — different settings struct,
  feed `project::AgentServerStore`, used by both modes. Do not touch those.
- **`crates/settings_content/src/agent.rs`** (558 lines, `AgentSettingsContent`) —
  dead once `agent_settings` crate is dead. The `enabled` field it defines has
  **zero readers anywhere in the repo**, including inside `agent_ui` itself
  (pre-existing dead code, unrelated to this change).
- **`crates/agent_settings`** (153 lines) — depended on by **only** `agent_ui`'s
  `Cargo.toml`. Orphaned wholesale.
- **`crates/agent_servers`** (4199 lines: `acp.rs` 3467, `custom.rs` 581,
  `agent_servers.rs` 151) — depended on by **only** `agent_ui`'s `Cargo.toml`.
  `project::agent_server_store` (what Terminal mode actually uses for binary
  resolution) does **not** depend on this crate at all — confirmed, `project`'s
  `Cargo.toml` has no `agent_servers` line, and `agent_server_store.rs` has zero
  `agent_servers::` references. This whole crate is the ACP-connection machinery
  (`connect()`, `AcpConnection`, `CustomAgentServer: AgentServer`) that only
  `agent_connection_store.rs`'s chat-only `Agent::server()` call ever invokes.
  Orphaned wholesale.
- **`crates/acp_thread`** (8009 lines) — depended on by only `agent_servers` and
  `agent_ui`. Once both are gone, orphaned wholesale.
- **`crates/acp_tools`** (902 lines) — depended on by only `agent_servers`
  (`use acp_tools::AcpConnectionRegistry` in `acp.rs`). Not wired into
  `zed.rs`/`main.rs` as a standalone devtool in this fork already
  (`grep "acp_tools::init"` → nothing). Orphaned wholesale.
- **`agent-client-protocol` (external crate, v0.11.1)** — depended on by only
  `agent_servers` and `agent_ui`. Drop from the workspace `Cargo.toml` and both
  crates' `[dependencies]` once step 2 below lands. Not a line-count item (it's an
  external dependency, not repo code).
- **`prompt_store`** dependency of `agent_ui` — `ensure_prompt_store`/
  `PromptStore::global` are called only inside `start_chat` in `agent_view.rs`
  (chat-only). Worth checking for orphaning too but not traced past `agent_ui`'s
  own `Cargo.toml` — out of the sized scope below, flagged as a loose end.

## Traps — answered

**1. `agent_diff` — chat-only, confirmed.** Every struct in it (`AgentDiffPane`,
`AgentDiffToolbar`, `AgentDiff`, `WorkspaceThread`) is built around
`Entity<AcpThread>`/`WeakEntity<AcpThread>`. `AcpThread` is created **only** inside
`ConversationView` (chat). Terminal-mode edits are made by the CLI process
directly against the filesystem — no `AcpThread`, no action-log diff review
surface. Nothing else in the repo creates an `AcpThread` for this to attach to.

**2. `completion_provider` — chat-only, confirmed.** It is the `@`-mention
completion (`PromptCompletionProvider<T: PromptCompletionProviderDelegate>`) wired
into `message_editor.rs`'s message editor. It is never registered as a general
`editor::CompletionProvider` anywhere else — its `impl CompletionProvider` exists
solely to plug into that one message editor instance. `search_symbols` inside it is
`pub(crate)`; the one external mention of it (`project_symbols.rs`) is a comment,
not a call.

**3. `SerializableItem` / the `mode` column — safe, no migration needed.** The
`agent_views.mode` SQL column is a plain `TEXT` column (`agent_view.rs:1992`,
`persistence` module), not a Rust enum constraint — SQLite has no idea `AgentViewMode`
exists. `mode_from_name` already has a catch-all default arm
(`_ => AgentViewMode::Chat`). A saved workspace with a row reading `mode = "chat"`
restored after `Chat` is deleted needs exactly one edit: flip that default arm to
`AgentViewMode::Terminal`. The tab then silently reopens as Terminal — no panic, no
schema migration, no data loss (the row itself is untouched; only the
in-memory interpretation of the string changes). Confirmed no other code path
parses this column.

**4. `pub` items another crate imports — three, all handled above.** `AgentView`,
`agent_icon` (kept, shared). `AgentDiffPane`/`AgentDiffToolbar` (used by
`zed.rs`'s toolbar registration — that call site must be deleted alongside
`agent_diff.rs`). `ModelSelector` (`pub use model_selector::ModelSelector` in
`agent_ui.rs`) has **zero external consumers** despite being `pub` — only
`model_selector_popover.rs` (chat-only) uses it. Safe to delete the `pub use` too.

**5. Does removing `Chat` break a `match` outside `agent_ui`? No.** The only
external consumer of `AgentViewMode`, `rail_agents.rs`, never matches on it — both
call sites hardcode `AgentViewMode::Terminal` as a literal. Every actual `match`/`==`
on the enum (7 sites: `mode_name`, `mode_from_name`, the toggle button's
`selected_index`, the two `set_mode` calls in `render_mode_switch`, the `start()`
branch, and one test's `for mode in [Chat, Terminal]` loop) lives inside
`agent_view.rs` itself and is covered in the deletion sequence below.

## Ordered deletion sequence

This **cannot** go file-by-file inside `agent_ui`: `agent_ui.rs`'s `mod` list and
the internal `crate::x` web (e.g. `conversation_view.rs` importing `agent_diff`,
`message_editor`, `mention_set`, `config_options`, `entry_view_state`,
`mode_selector`, `model_selector_popover` — all in the same breath) mean deleting
any one chat-only file while the others remain breaks compilation immediately. The
23-file chat slice inside `agent_ui`, `agent_view.rs`'s trims, and `actions.rs`'s
trim must land as **one commit**.

The four now-orphaned crates, however, only become dead *after* that commit lands
— so they can follow in a second commit. Two-commit plan:

**Commit 1 — gut `agent_ui`:**
1. Extract `RenameAgent` out of `actions.rs`'s `actions!(agent, [...])` block into
   its own tiny declaration (in `agent_view.rs` or `agent_ui.rs`) — it is the one
   shared action in that file.
2. Delete the 23 chat-only files/dirs: `conversation_view.rs`,
   `conversation_view/` (whole dir), `agent_diff.rs`, `completion_provider.rs`,
   `message_editor.rs`, `mention_set.rs`, `config_options.rs`, `model_selector.rs`,
   `entry_view_state.rs`, `diagnostics.rs`, `agent_connection_store.rs`,
   `outline.rs`, `mode_selector.rs`, `mention_image.rs`,
   `external_source_prompt.rs`, `model_selector_popover.rs`, `markdown_style.rs`,
   `ui.rs`, `ui/` (whole dir). Delete the rest of `actions.rs` (keep only
   `RenameAgent`).
3. Trim `agent_view.rs`: remove `State::Chat(...)` variant, `start_chat`,
   `conversation_view()` accessor, the `if mode == AgentViewMode::Chat` branch in
   `start()`, the "Chat" `ToggleButtonSimple` in `render_mode_switch`, the
   `Chat => 0` match arm, the `self.mode == AgentViewMode::Chat` bool passed into
   `missing_binary::render`, and update `mode_name`/`mode_from_name` (default arm
   → `Terminal`). Update/remove the test asserting both modes round-trip.
4. Trim `agent_ui.rs`'s `mod`/`pub use` list to match what remains
   (`agent_view`, `missing_binary`, `actions` (or wherever `RenameAgent` landed)).
5. Remove `zed_actions::agent::AgentViewMode::Chat` variant itself (in
   `zed_actions/src/lib.rs`) — safe per Trap 5.
6. Drop `acp_thread`, `agent-client-protocol`, `agent_servers`, `agent_settings`,
   and (pending verification) `prompt_store` from `agent_ui/Cargo.toml`.
7. Delete `zed.rs`'s `agent_diff_toolbar` registration + its import.
8. Drop the `"agent": {...}` block from `assets/settings/default.json`.
9. Delete `crates/settings_content/src/agent.rs` and the `SettingsContent.agent`
   field wiring in `settings_content.rs`.

**Commit 2 — delete the orphaned crates** (only safe once Commit 1 compiles clean):
10. Delete `crates/agent_servers/`, `crates/acp_thread/`, `crates/acp_tools/`,
    `crates/agent_settings/` wholesale; remove all four from the workspace
    `Cargo.toml` members list; remove `agent-client-protocol` from the workspace
    `Cargo.toml` dependency table.

## Total line count

| Bucket | Lines |
|---|---:|
| `agent_ui` chat-only files (23 files, Commit 1 step 2) | 24,064 |
| `agent_servers` (whole crate) | 4,199 |
| `acp_thread` (whole crate) | 8,009 |
| `acp_tools` (whole crate) | 902 |
| `agent_settings` (whole crate) | 153 |
| `settings_content/src/agent.rs` | 558 |
| **Total** | **≈ 37,885** |

Plus small in-place edits (not whole-file deletions): `agent_view.rs` (~90 lines
trimmed of ~2147), `agent_ui.rs` (~25 of 67), `zed.rs` (2 lines + import),
`default.json` (~44 lines), `zed_actions/src/lib.rs` (~5 lines, the `Chat` variant
+ its doc comment).

**The ~13,000-line estimate is wrong by roughly 3x.** It appears to have counted
only `agent_ui`'s chat-only files and stopped there. The real boundary reaches
through `agent_ui`'s `Cargo.toml` into three entire crates (`agent_servers`,
`acp_thread`, `acp_tools`) plus `agent_settings`, none of which have any consumer
left in the workspace once the chat UI is gone. **≈37,900 lines**, not ~13,000.

## Loose ends not chased to ground

- `prompt_store` crate dependency of `agent_ui` — likely also orphaned by this
  change (only call site found was inside `start_chat`), but I did not trace
  whether `prompt_store` has other consumers elsewhere in the workspace. Size it
  before commit 1 step 6.
- `settings_content.rs`'s exact mechanics for removing the `agent` field from
  `SettingsContent` (schema generation, merge macros) — read only that one file's
  field declaration, not its full merge/schema derive machinery.
- Did not verify whether the settings UI (auto-generated from JSON schema) has any
  hardcoded reference to the `"agent"` settings key that would need a matching edit
  beyond the struct/JSON removal.

**Status:** DONE
**Summary:** Terminal mode is a pure PTY spawn with zero ACP involvement, confirmed directly in `agent_view.rs::start()` and its own comments — so the entire ACP stack (`agent_ui`'s 23 chat-only files, plus the wholly-orphaned `agent_servers`, `acp_thread`, `acp_tools`, and `agent_settings` crates) can go with Chat. Real deletion size is ≈37,900 lines, about 3x the ~13,000 estimate, because the estimate stopped at `agent_ui` and missed three entire crates that have no consumer left once it's gone.
**Concerns/Blockers:** None block the deletion itself. Two loose ends flagged above (`prompt_store` orphan status, `settings_content.rs` merge-macro mechanics) should be checked before the Cargo.toml edits in Commit 1 step 6, but neither changes the shape of the answer.
