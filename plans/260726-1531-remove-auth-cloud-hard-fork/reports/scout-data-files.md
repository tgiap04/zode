# Scout Report — Non-Rust Data Files Referencing Deleted Crates

Date: 2026-07-26
Scope: `assets/**`, `docs/**`, packaging manifests, menu definitions.
Goal: find every runtime-only (compiler-invisible) reference to the 56 crates slated for deletion.

**Headline:** 20 data files must be edited/deleted. The worst single file by far is
`assets/keymaps/default-macos.json` (125 deleted-crate action strings across 29 binding blocks),
with `default-linux.json` (119) and `default-windows.json` (117) essentially tied behind it.
`assets/settings/default.json` is the worst by *line volume* — ~380 lines across 20 top-level keys.

---

## 1. Keymaps — `assets/keymaps/**/*.json`

### 1.1 Count per file

| File | Deleted-crate action strings | Binding blocks affected | Verdict |
|---|---|---|---|
| `assets/keymaps/default-macos.json` | **125** | 29 | worst file |
| `assets/keymaps/default-linux.json` | **119** | 27 | |
| `assets/keymaps/default-windows.json` | **117** | 27 | |
| `assets/keymaps/macos/cursor.json` | **17** | 5 | |
| `assets/keymaps/linux/cursor.json` | **17** | 5 | |
| `assets/keymaps/vim.json` | **6** | 3 | |
| `assets/keymaps/macos/jetbrains.json` | **1** | 1 | + 1 dead context |
| `assets/keymaps/linux/jetbrains.json` | **1** | 1 | + 1 dead context |
| `assets/keymaps/macos/sublime_text.json` | 0 | — | dead context only |
| `assets/keymaps/linux/sublime_text.json` | 0 | — | dead context only |
| `assets/keymaps/{macos,linux}/atom.json` | 0 | — | clean |
| `assets/keymaps/{macos,linux}/emacs.json` | 0 | — | clean |
| `assets/keymaps/macos/textmate.json` | 0 | — | clean |
| `assets/keymaps/initial.json` | 0 | — | clean |
| `assets/keymaps/storybook.json` | 0 | — | clean |

**Total: 403 deleted-crate action strings across 8 files.**

### 1.2 Count per namespace (all keymaps combined)

| Namespace | Owning deleted crate | Occurrences |
|---|---|---|
| `agent::` | `agent_ui` / `agent` | 315 |
| `agents_sidebar::` | `agent_ui` | 30 |
| `assistant::` | `agent_ui` (inline assist) | 13 |
| `collab_panel::` | `collab_ui` | 21 |
| `zeta::` | `edit_prediction` (zeta) | 8 |
| `rules_library::` | `rules_library` | 7 |
| `channel_modal::` | `collab_ui` / `channel` | 6 |
| `inline_assistant::` | `agent_ui` | 6 |
| `edit_prediction::` | `edit_prediction_ui` | 5 |

### 1.3 Distinct action strings (with total occurrence counts)

| Action | Count | Action | Count |
|---|---|---|---|
| `agents_sidebar::ToggleThreadSwitcher` | 18 | `agent::ToggleFocus` | 17 |
| `agent::ScrollOutputPageUp` | 15 | `agent::ScrollOutputPageDown` | 15 |
| `assistant::InlineAssist` | 13 | `agent::AddSelectionToThread` | 13 |
| `agent::ScrollOutputToTop` | 12 | `agent::ScrollOutputToBottom` | 12 |
| `agent::Keep` | 12 | `agent::RejectAll` | 11 |
| `agent::KeepAll` | 11 | `agent::CycleFavoriteModels` | 11 |
| `agent::ScrollOutputToPreviousMessage` | 9 | `agent::ScrollOutputToNextMessage` | 9 |
| `agent::ScrollOutputLineUp` | 9 | `agent::ScrollOutputLineDown` | 9 |
| `agent::ToggleProfileSelector` | 8 | `agent::NewThread` | 8 |
| `agent::RemoveSelectedThread` | 7 | `agent::Chat` | 7 |
| `channel_modal::ToggleMode` | 6 | `agent::ToggleModelSelector` | 6 |
| `agent::Reject` | 6 | `agent::OpenAgentDiff` | 6 |
| `agent::CycleModeSelector` | 6 | `agent::ArchiveSelectedThread` | 6 |
| `agents_sidebar::NewThreadInGroup` | 5 | `agent::OpenSettings` | 5 |
| `rules_library::NewRule` | 4 | `agents_sidebar::FocusSidebarFilter` | 4 |
| `rules_library::ToggleDefaultRule` | 3 | `inline_assistant::ThumbsUpResult` | 3 |
| `inline_assistant::ThumbsDownResult` | 3 | `edit_prediction::ToggleMenu` | 3 |
| `collab_panel::ToggleSelectedChannelFavorite` | 3 | `collab_panel::ToggleFocus` | 3 |
| `collab_panel::Remove` | 3 | `collab_panel::OpenSelectedChannelNotes` | 3 |
| `collab_panel::MoveChannelUp` | 3 | `collab_panel::MoveChannelDown` | 3 |
| `collab_panel::InsertSpace` | 3 | `agents_sidebar::ToggleThreadHistory` | 3 |
| `agent::UndoLastReject` | 3 | `agent::ToggleThinkingMode` | 3 |
| `agent::ToggleThinkingEffortMenu` | 3 | `agent::ToggleOptionsMenu` | 3 |
| `agent::ToggleNewThreadMenu` | 3 | `agent::ToggleFastMode` | 3 |
| `agent::SendNextQueuedMessage` | 3 | `agent::SendImmediately` | 3 |
| `agent::RemoveFirstQueuedMessage` | 3 | `agent::RejectOnce` | 3 |
| `agent::PasteRaw` | 3 | `agent::OpenRulesLibrary` | 3 |
| `agent::OpenPermissionDropdown` | 3 | `agent::OpenAddContextMenu` | 3 |
| `agent::NewExternalAgentThread` | 3 | `agent::ManageProfiles` | 3 |
| `agent::ExpandMessageEditor` | 3 | `agent::EditFirstQueuedMessage` | 3 |
| `agent::CycleThinkingEffort` | 3 | `agent::CyclePreviousInlineAssist` | 3 |
| `agent::CycleNextInlineAssist` | 3 | `agent::ContinueThread` | 3 |
| `agent::ClearMessageQueue` | 3 | `agent::ChatWithFollow` | 3 |
| `agent::AllowOnce` | 3 | `agent::AllowAlways` | 2 |
| `zeta::ThumbsUpActivePrediction` | 2 | `zeta::ThumbsDownActivePrediction` | 2 |
| `edit_prediction::RatePredictions` | 2 | `zeta::PreviousEdit` | 1 |
| `zeta::PreviewPrediction` | 1 | `zeta::NextEdit` | 1 |
| `zeta::FocusPredictions` | 1 | | |

### 1.4 Binding blocks to delete wholesale (block line ranges)

These `{ "context": ..., "bindings": {...} }` blocks are **entirely** about deleted UI — delete the whole block.

#### `assets/keymaps/default-macos.json` (worst file)

| Lines | Context | Deleted actions in block |
|---|---|---|
| 239–249 | `AgentDiff` | 5 |
| 250–261 | `Editor && editor_agent_diff` | 6 |
| 262–286 | `AgentPanel` | 18 |
| 287–293 | `AgentPanel > Markdown` | (markdown actions; dead ctx) |
| 294–301 | `AgentPanel && acp_thread` | 2 |
| 302–311 | `AgentFeedbackMessageEditor > Editor` | dead ctx |
| 312–317 | `AgentConfiguration` | dead ctx |
| 318–322 | `AcpThread > ModeSelector` | dead ctx |
| 323–344 | `AcpThread` | 16 |
| 345–378 | `AcpThread > Editor` | 27 |
| 379–387 | `AcpThread > Editor && start_of_input` | 3 |
| 388–396 | `AcpThread > Editor && end_of_input` | 3 |
| 397–403 | `AcpThread > Editor && mode == full` | dead ctx |
| 404–410 | `AcpThread > Editor && !use_modifier_to_send` | 1 |
| 411–418 | `AcpThread > Editor && use_modifier_to_send` | 1 |
| 419–424 | `ThreadHistory` | dead ctx |
| 425–430 | `ThreadHistory > Editor` | 1 |
| 431–436 | `ThreadsArchiveView` | 1 |
| 437–445 | `RulesLibrary` | 2 |
| 774–789 | `ThreadsSidebar` | 7 |
| 790–796 | `ThreadsSidebar && not_searching` | dead ctx |
| 797–803 | `ThreadSwitcher` | 2 |
| 883–891 | `Editor && edit_prediction` | dead ctx (uses `editor::*`) |
| 892–~899 | `Editor && edit_prediction && edit_prediction_mode == eager && !showing_completions` | dead ctx |
| 934–947 | `!AcpThread > Editor && mode == full` | 1 (predicate must be rewritten) |
| 948–959 | `InlineAssistant > Editor` | 6 |
| 960–~966 | `Prompt` | verify — `Prompt` ctx may be generic prompt UI, not agent |
| 1152–1159 | `CollabPanel && not_editing` | 1 |
| 1160–1169 | `CollabPanel` | 4 |
| 1170–1176 | `(CollabPanel && editing) > Editor` | 1 |
| 1177–1183 | `ChannelModal` | 1 |
| 1196–1202 | `ChannelModal > Picker > Editor` | 1 |
| 1315–1325 | `RatePredictionsModal` | 5 (`zeta::*`) |
| 1326–1334 | `RatePredictionsModal > Editor` | 3 (`zeta::*`) |
| 1539–1545 | `EditPredictionContext > Editor` | `dev::EditPredictionContext*` |

Plus scattered bindings inside otherwise-keepable blocks:

| Line | Token | Note |
|---|---|---|
| 3–55 (global) | `edit_prediction::RatePredictions`, `edit_prediction::ToggleMenu` | 2 bindings inside global block |
| 162–179 | `agent::AddSelectionToThread` | inside `Editor && mode == full` |
| 191 | ctx `Editor && mode == full && edit_prediction` | dead ctx block |
| 199 | ctx `Editor && !edit_prediction` | predicate must be rewritten (block otherwise keepable) |
| 230 | ctx `Editor && !agent_diff && !AgentPanel` | predicate must be rewritten |
| 680–757 | `workspace::Unfollow` (L744) | `call`-crate follow feature |
| 924–933 | `assistant::InlineAssist` | global-ish block |
| 927 | `workspace::FollowNextCollaborator` | `call` crate |
| 1257–1314 | `Terminal` block: 2 agent/assistant bindings | keep block, drop bindings |
| 1451–1453 | `onboarding::Finish`, `onboarding::SignIn`, `onboarding::OpenAccount` | `SignIn`/`OpenAccount` are auth |
| 1542–1543 | `dev::EditPredictionContextGoBack` / `GoForward` | `edit_prediction_context` crate |

#### `assets/keymaps/default-linux.json`

| Lines | Context | Deleted actions |
|---|---|---|
| 202–212 | `Editor && editor_agent_diff` | 6 |
| 213–222 | `AgentDiff` | 5 |
| 223–249 | `AgentPanel` | 20 |
| 250–257 | `AgentPanel > Markdown` | dead ctx |
| 258–265 | `AgentPanel && acp_thread` | 2 |
| 266–274 | `AgentFeedbackMessageEditor > Editor` | dead ctx |
| 275–279 | `AcpThread > ModeSelector` | dead ctx |
| 280–301 | `AcpThread` | 16 |
| 302–337 | `AcpThread > Editor` | 28 |
| 338–346 | `AcpThread > Editor && start_of_input` | 3 |
| 347–355 | `AcpThread > Editor && end_of_input` | 3 |
| 356–362 | `AcpThread > Editor && mode == full` | dead ctx |
| 363–369 / 370–377 | `AcpThread > Editor && (!)use_modifier_to_send` | 1 + 1 |
| 378–383 | `ThreadHistory` | 1 |
| 384–389 | `ThreadsArchiveView` | 1 |
| 390–398 | `RulesLibrary` | 3 |
| 718–733 | `ThreadsSidebar` | 7 |
| 734–740 | `ThreadsSidebar && not_searching` | dead ctx |
| 741–747 | `ThreadSwitcher` | 2 |
| 830–839 | `Editor && edit_prediction` | dead ctx |
| 840–~847 | `Editor && edit_prediction && ... eager` | dead ctx |
| 883–895 | `!AcpThread > Editor && mode == full` | 1 (rewrite predicate) |
| 896–904 | `InlineAssistant` | 4 |
| 905–~911 | `Prompt` | verify |
| 1104–1110 | `CollabPanel && not_editing` | 1 |
| 1111–1119 | `CollabPanel` | 4 |
| 1120–1125 | `(CollabPanel && editing) > Editor` | 1 |
| 1126–1131 | `ChannelModal` | 1 |
| 1142–1147 | `ChannelModal > Picker > Editor` | 1 |
| 1448–1454 | `EditPredictionContext > Editor` | `dev::EditPredictionContext*` |

Scattered:

| Line | Token |
|---|---|
| 44 | `edit_prediction::RatePredictions` |
| 45 | `edit_prediction::ToggleMenu` |
| 150 | `agent::AddSelectionToThread` |
| 158 | ctx `Editor && mode == full && edit_prediction` (dead block) |
| 165 | ctx `Editor && !edit_prediction` (rewrite) |
| 194 | ctx `Editor && !agent_diff` (rewrite) |
| 692 | `workspace::Unfollow` |
| 872 | `workspace::FollowNextCollaborator` |
| 877–882 | `!Terminal` block — 1 assistant binding |
| 1198–1245 | `Terminal` block — 2 bindings |
| 1362 | `onboarding::SignIn` |
| 1363 | `onboarding::OpenAccount` |
| 1451–1452 | `dev::EditPredictionContextGoBack` / `GoForward` |

#### `assets/keymaps/default-windows.json`

Structurally identical to linux. Blocks: 200–211, 212–222, 223–250, 251–258, 258–265, 266–275, 276–281, 281–302, 303–338, 339–347, 348–356, 357–363, 364–370, 371–378, 379–385, 386–392, 393–401, 719–734, 735–741, 742–748, 822–832, 833–~840, 882–895, 896–905, 906–~912, 1105–1112, 1113–1122, 1123–1129, 1130–1136, 1148–1154, 1464–1470.

Scattered: L153 ctx `edit_prediction`, L161 ctx `!edit_prediction`, L192 ctx `!agent_diff`, L680 `workspace::Unfollow`, L870 `workspace::FollowNextCollaborator`, L1210–1258 Terminal block (2), L1384 `onboarding::SignIn`, L1385 `onboarding::OpenAccount`, L1467–1468 `dev::EditPredictionContext*`.

#### `assets/keymaps/vim.json`

| Line | Token | Note |
|---|---|---|
| 217 | `workspace::FollowNextCollaborator` | `[ f` — `call` crate |
| 218 | `workspace::FollowNextCollaborator` | `] f` |
| 362 | `assistant::InlineAssist` | `ctrl-x ctrl-a` |
| 1073 | ctx `Editor && edit_prediction && edit_prediction_mode == eager && ...` | dead ctx block |
| 1083 | `agent::Chat` | inside `MessageEditor > Editor && VimControl` block (1080–1085) |
| 1141–1162 | ctx `ThreadsSidebar && !Editor` / `ThreadsSidebar > Editor && VimControl` | 4 actions: `agents_sidebar::FocusSidebarFilter` (1150), `agent::RemoveSelectedThread` (1151), `agents_sidebar::NewThreadInGroup` (1152, 1153) |

#### `assets/keymaps/{macos,linux}/cursor.json` (17 each)

| Lines | Context | Actions |
|---|---|---|
| 3–13 | `Workspace` | 5× `agent::ToggleFocus` / `agent::OpenSettings` |
| 14–24 | `Editor && mode == full` | 4× `agent::*` + `assistant::InlineAssist` |
| 26 | ctx `InlineAssistant > Editor` | dead ctx block |
| 36–59 (macos) / 35–58 (linux) | `AgentPanel \|\| (MessageEditor > Editor)` | 4 |
| 60–67 / 59–66 | `Editor && editor_agent_diff` | 2 (`agent::KeepAll`, `agent::RejectAll`) |
| 68 / 68 | ctx `Editor && mode == full && edit_prediction` | dead ctx |
| 76–82 / 75–81 | `Terminal` | 1× `assistant::InlineAssist` |

#### `assets/keymaps/{macos,linux}/jetbrains.json`

| File | Line | Token |
|---|---|---|
| `macos/jetbrains.json` | 80 | `assistant::InlineAssist` (`cmd-\`) |
| `macos/jetbrains.json` | 192 | ctx `Dock \|\| Workspace \|\| OutlinePanel \|\| ProjectPanel \|\| CollabPanel` — rewrite predicate |
| `linux/jetbrains.json` | 81 | `assistant::InlineAssist` (`ctrl-\`) |
| `linux/jetbrains.json` | 188 | same `CollabPanel` predicate — rewrite |

#### `assets/keymaps/{macos,linux}/sublime_text.json`

| File | Line | Token | Note |
|---|---|---|---|
| `macos/sublime_text.json` | 70 | ctx `Editor && !agent_diff` | predicate references removed `agent_diff` key context. Rewrite to `Editor`. |
| `linux/sublime_text.json` | 68 | ctx `Editor && !agent_diff` | same |

> **Important:** unknown key-context *identifiers* (e.g. `AgentPanel`, `agent_diff`) do NOT error at runtime
> the way unknown *actions* do — they simply never match. Unknown **actions** produce keymap load errors
> surfaced to the user. So the 403 action strings are the hard failures; the ~90 dead contexts are silent
> cruft that should still be cleaned.

---

## 2. Settings — `assets/settings/*.json`

### 2.1 `assets/settings/default.json` — 2611 lines, 20 affected top-level keys, ~380 lines

| Line span | Key | Owning deleted crate | Lines | Note |
|---|---|---|---|---|
| 71 | `agent_ui_font_size` | `agent_ui` | 1 | |
| 73 | `agent_buffer_font_size` | `agent_ui` | 1 | |
| 422 | `show_edit_predictions` | `edit_prediction` | 1 | also a *language-level* setting — see 2081 |
| 425 | `edit_predictions_disabled_in` | `edit_prediction` | 1 | |
| 450–455 | `calls` (`mute_on_join`, `share_on_join`) | `call` | 6 | |
| 492–509 | `audio` (`experimental.auto_microphone_volume`, `experimental.output_audio_device`, `experimental.input_audio_device`) | `livekit_client` / `audio` | 18 | **verify**: the `audio` crate itself is not in the delete list; only remove if `audio` settings are livekit-only |
| 904–911 | `collaboration_panel` (`button`, `dock`, `default_width`) | `collab_ui` | 8 | |
| 968–972 | `message_editor` (`auto_replace_emoji_shortcode`) | `collab_ui` chat | 5 | |
| **973–1183** | **`agent`** | `agent_settings` / `agent_ui` | **211** | **largest single block in the repo** |
| 1185 | `show_call_status_icon` | `call` | 1 | |
| 1433 | `disable_ai` | (settings) | 1 | becomes meaningless once AI is gone — remove |
| 1436 | `auto_update` | `auto_update` | 1 | |
| 1600–1657 | `edit_predictions` (`provider`, `disabled_globs`, `mode`, `copilot`, `codestral`, `ollama`, `open_ai_compatible_api`) | `edit_prediction*`, `copilot`, `codestral`, `ollama` | 58 | |
| 2081 | `languages.Diff.show_edit_predictions` | `edit_prediction` | 1 | nested inside `languages` block |
| 2272–2312 | `language_models` (`anthropic`, `bedrock`, `google`, `ollama`, `openai`, `openai_compatible`, `opencode`, `open_router`, `lmstudio`, `deepseek`, `mistral`, `vercel`, `vercel_ai_gateway`, `x_ai`, `zed.dev`) | `language_models` + all provider crates | 41 | |
| 2472 | `server_url: "https://zed.dev"` | `client` / `cloud_api_client` | 1 | **verify** — `client` crate may not be deleted |
| 2561 | `context_server_timeout` | `context_server` | 1 | |
| 2563 | `context_servers` | `context_server` | 1 | |
| 2565 | `agent_servers` | `agent_servers` | 1 | |

Sub-keys inside the 211-line `agent` block (for surgical removal / partial keep decisions):
`inline_assistant_use_streaming_tools`, `enabled`, `button`, `dock`, `flexible`, `sidebar_side`,
`default_width`, `default_height`, `limit_content_width`, `max_content_width`, `default_model`,
`model_parameters`, `tool_permissions`, `single_file_review`, `enable_feedback`, `default_profile`,
`profiles`, `new_thread_location`, `notify_when_agent_waiting`, `play_sound_when_agent_done`,
`expand_edit_card`, `expand_terminal_card`, `thinking_display`, `cancel_generation_on_terminal_stop`,
`use_modifier_to_send`, `message_editor_min_lines`, `show_turn_stats`, `show_merge_conflict_indicator`.

### 2.2 Auth/collab-adjacent keys — verify before removing

| Line | Key | Note |
|---|---|---|
| 479 | `title_bar.show_onboarding_banner` | AI/sign-in onboarding banner |
| 481 | `title_bar.show_user_picture` | requires auth |
| 483 | `title_bar.show_user_menu` | requires auth |
| 485 | `title_bar.show_sign_in` | requires auth |
| 1424–1429 | `telemetry` (`diagnostics`, `metrics`) | `telemetry` crate NOT in delete list — **keep**; but `crashes` is deleted, so `telemetry.diagnostics` may lose its consumer |
| 2313–2327 | `session.trust_all_worktrees` | comment mentions "MCP servers" — comment needs editing, key stays |

### 2.3 Other settings files — all clean

| File | Status |
|---|---|
| `assets/settings/initial_user_settings.json` | clean (only `ui_font_size`, `buffer_font_size`, `theme`) |
| `assets/settings/initial_local_settings.json` | clean (comment only) |
| `assets/settings/initial_server_settings.json` | clean (comment only) |
| `assets/settings/initial_tasks.json` | clean |
| `assets/settings/initial_debug_tasks.json` | clean |
| `assets/settings/initial_local_debug_tasks.json` | clean |
| `assets/settings/default_semantic_token_rules.json` | clean |

---

## 3. Menus

### `crates/zed/src/zed/app_menus.rs` (Rust, but menu *data* — compiler WILL catch these)

| Line | Token | Note |
|---|---|---|
| 1 | `use collab_ui::collab_panel;` | import of deleted crate |
| 45 | `MenuItem::action("Collab Panel", collab_panel::ToggleFocus)` | Zed menu |
| 67 | `MenuItem::action("Check for Updates", auto_update::Check)` | Zed menu |
| 300–301 | `MenuItem::action("View Release Notes Locally", auto_update_ui::ViewReleaseNotesLocally)` | Help menu |
| 307–309 | `zed_actions::feedback::{FileBugReport, RequestFeature, EmailZed}` | `feedback` crate not in delete list — **keep**, but verify it does not depend on `client`/`cloud_api_client` |
| 317 | `feedback::OpenZedRepo` | keep |

Notably **absent** from the menu file: no Agent Panel / Assistant / Edit Prediction menu items. Good.

### macOS plists / entitlements — audio-video permissions become dead once `livekit_client` goes

| File | Keys | Note |
|---|---|---|
| `crates/zed/resources/info/Permissions.plist` | `NSCameraUsageDescription`, `NSMicrophoneUsageDescription`, `NSSpeechRecognitionUsageDescription` | mic/camera are livekit-only. Removing them shrinks the permission prompt surface. Optional but recommended. |
| `crates/zed/resources/zed.entitlements` | `com.apple.security.device.audio-input`, `com.apple.security.device.camera` | same — livekit-only. Optional. |
| `crates/zed/resources/info/DocumentTypes.plist` | — | clean |
| `crates/zed/resources/info/SupportedPlatforms.plist` | — | clean |
| `crates/zed/resources/{flatpak,snap,windows}/*`, `zed.desktop.in` | — | **clean**, no references found |

---

## 4. JSON Schemas

**No static schema files describe agent / edit_prediction / language_model settings.**

| Path | Finding |
|---|---|
| `assets/**` | no `*.schema.json`, no JSON Schema documents at all |
| `crates/json_schema_store/src/schemas/*.json` | third-party SchemaStore mirrors (`tsconfig.json`, `package.json`, eslint/prettier etc.) — **unrelated**, no Zed settings schema |
| Settings/keymap JSON schema | generated at runtime from Rust `#[derive(JsonSchema)]` — the compiler covers this |
| `release-manifest.json`, `renovate.json` | clean |

**Risk here: LOW.** Nothing to edit.

---

## 5. Themes, Snippets, Tasks, Sounds, Prompts, Icons

### 5.1 Themes — `assets/themes/*/*.json`

| File | Key | Lines | Verdict |
|---|---|---|---|
| `assets/themes/one/one.json` | `players` | 149, 561 | **KEEP** |
| `assets/themes/ayu/ayu.json` | `players` | 142, 544, 946 | **KEEP** |
| `assets/themes/gruvbox/gruvbox.json` | `players` | 147, 564, 981, 1398, 1815 | **KEEP** |

**`players` verification (as requested):** `PlayerColors` is consumed well beyond collab. Non-collab callers:

- `crates/editor/src/editor.rs:26414, 29359, 29861` — `players().local()` for the local cursor/selection
- `crates/editor/src/editor.rs:10314` — `players().read_only()`
- `crates/terminal/src/terminal.rs:2509` — `players().local().cursor` (terminal cursor)
- `crates/search/src/search_bar.rs:125`, `crates/debugger_ui/.../console.rs:415,891`, `memory_view.rs:320`,
  `crates/extensions_ui/src/extensions_ui.rs:1259` — all `players().local()`

Collab-only / agent-only consumers that go away with the deletion:
`crates/title_bar/src/collab.rs:163`, `crates/collab_ui/src/collab_panel.rs:2845`,
`crates/rules_library/src/rules_library.rs:1140`,
`crates/workspace/src/workspace.rs:7969-7973` and `pane_group.rs:463-469` (`players().agent()`),
`crates/editor/src/editor.rs:28388-28404` (`players().agent()`, `color_for_participant`, `absent`).

**Conclusion: keep `players` in every theme file. Zero theme edits required.**
No theme file contains any `agent.*`, `assistant.*`, `copilot.*`, `collab.*`, `chat.*`, `notification.*`,
or `prediction.*` color key. Themes are entirely safe.

### 5.2 Prompt templates — `assets/prompts/` (whole directory goes)

| File | Owner |
|---|---|
| `assets/prompts/content_prompt.hbs` | `agent` (inline assistant) |
| `assets/prompts/content_prompt_v2.hbs` | `agent` |
| `assets/prompts/terminal_assistant_prompt.hbs` | `agent` (terminal inline assist) |

### 5.3 Sounds — `assets/sounds/` (7 of 8 files go)

| File | Owner | Verdict |
|---|---|---|
| `agent_done.wav` | `agent_ui` (`play_sound_when_agent_done`) | delete |
| `guest_joined_call.wav` | `call` | delete |
| `joined_call.wav` | `call` | delete |
| `leave_call.wav` | `call` | delete |
| `mute.wav` | `call`/`livekit_client` | delete |
| `unmute.wav` | `call`/`livekit_client` | delete |
| `start_screenshare.wav` | `livekit_client` | delete |
| `stop_screenshare.wav` | `livekit_client` | delete |

(All 8 actually. Verify no non-call caller of the `Sound` enum survives.)

### 5.4 Icons — `assets/icons/` (~48 SVGs become orphaned)

`ai_anthropic`, `ai_bedrock`, `ai_claude`, `ai_deep_seek`, `ai_edit`, `ai_gemini`, `ai_google`,
`ai_lm_studio`, `ai_mistral`, `ai_ollama`, `ai_open_ai_compat`, `ai_open_ai`, `ai_open_code`,
`ai_open_router`, `ai_v_zero`, `ai_vercel`, `ai_x_ai`, `ai_zed`, `copilot`, `copilot_disabled`,
`copilot_error`, `copilot_init`, `zed_agent`, `zed_agent_two`, `zed_assistant`, `zed_predict`,
`zed_predict_disabled`, `zed_predict_down`, `zed_predict_error`, `zed_predict_up`, `new_thread`,
`thread`, `thread_from_summary`, `threads_sidebar_left_closed`, `threads_sidebar_left_open`,
`threads_sidebar_right_closed`, `threads_sidebar_right_open`, `mic`, `mic_mute`, `screen`, `chat`,
`bell`, `bell_dot`, `bell_off`, `bell_ring`, `user_check`, `user_group`, `user_round_pen`.

These are referenced through the `IconName` enum in `crates/ui` — **the compiler catches the enum**,
but the SVG files themselves are dead weight and `IconName` variants must be pruned by hand.
`bell*` and `user*` may have non-collab consumers — verify before deleting.

### 5.5 Snippets / tasks — clean

`assets/settings/initial_tasks.json`, `initial_debug_tasks.json`, `initial_local_debug_tasks.json`: no references.

---

## 6. Documentation — `docs/src/**/*.md`

### 6.1 Whole directories/pages that document deleted features (delete)

```
docs/src/ai/agent-panel.md
docs/src/ai/agent-settings.md
docs/src/ai/ai-improvement.md
docs/src/ai/billing.md
docs/src/ai/configuration.md
docs/src/ai/edit-prediction.md
docs/src/ai/external-agents.md
docs/src/ai/inline-assistant.md
docs/src/ai/llm-providers.md
docs/src/ai/mcp.md
docs/src/ai/models.md
docs/src/ai/overview.md
docs/src/ai/plans-and-usage.md
docs/src/ai/privacy-and-security.md
docs/src/ai/rules.md
docs/src/ai/subscription.md
docs/src/ai/text-threads.md
docs/src/ai/tool-permissions.md
docs/src/ai/tools.md
docs/src/collaboration/channels.md
docs/src/collaboration/contacts-and-private-calls.md
docs/src/collaboration/overview.md
docs/src/authentication.md
docs/src/extensions/agent-servers.md
docs/src/extensions/mcp-extensions.md
docs/src/remote-development.md
docs/src/update.md
```

### 6.2 Pages needing partial edits (contain references, but survive)

```
docs/src/SUMMARY.md                 <-- mandatory; L13-28, L51-53, L165, L184, L186, L188 link to deleted pages
docs/src/all-actions.md             <-- generated reference; regenerate
docs/src/reference/all-settings.md  <-- generated reference; regenerate
docs/src/reference/default-key-bindings.md  <-- generated; regenerate
docs/src/completions.md
docs/src/configuring-zed.md
docs/src/editing-code.md
docs/src/extensions.md
docs/src/extensions/developing-extensions.md
docs/src/extensions/languages.md
docs/src/extensions/slash-commands.md
docs/src/getting-started.md
docs/src/git.md
docs/src/roles.md
docs/src/terminal.md
docs/src/telemetry.md
docs/src/toolchains.md
docs/src/troubleshooting.md
docs/src/vim.md
docs/src/visual-customization.md
docs/src/migrate/intellij.md
docs/src/migrate/pycharm.md
docs/src/migrate/rustrover.md
docs/src/migrate/vs-code.md
docs/src/migrate/webstorm.md
docs/src/development/feature-process.md
docs/src/development/freebsd.md
docs/src/development/linux.md
docs/src/development/windows.md
docs/src/languages/rego.md
docs/src/extensions/mcp-extensions.md
```

`docs/src/SUMMARY.md` will break the mdBook build if not updated — links to deleted pages are a hard error in `mdbook build`.

---

## TOTAL BLAST RADIUS

### Distinct data files that must be edited

| Category | Files to EDIT | Files to DELETE |
|---|---|---|
| Keymaps | 10 | 0 |
| Settings | 1 (`default.json`) | 0 |
| Menus (Rust + plist) | 3 (`app_menus.rs`, `Permissions.plist`, `zed.entitlements`) | 0 |
| JSON schemas | 0 | 0 |
| Themes | 0 | 0 |
| Prompts | 0 | 3 (`.hbs`) |
| Sounds | 0 | 8 (`.wav`) |
| Icons | 0 | ~48 (`.svg`) |
| Docs (`.md`) | ~30 | ~27 |

**Files requiring hand-editing (non-generated, non-deletable): 14**
- 10 keymap JSONs
- 1 settings JSON
- 1 Rust menu file
- 2 macOS plist/entitlements (optional but recommended)

Plus ~30 markdown files, ~59 asset files to delete, and 3 generated docs to regenerate.

### Worst single file

**`assets/keymaps/default-macos.json`** — 125 deleted-crate action strings across 29 binding blocks,
plus ~15 dead key-context predicates. This is the highest-density runtime landmine in the repo:
every one of those 125 action strings produces a *keymap load error toast shown to the user* at
startup once the actions are unregistered. `default-linux.json` (119) and `default-windows.json` (117)
are near-identical siblings and must be edited in lockstep — they diverge only in modifier keys and
block ordering, so a diff-by-diff approach across the three is the safe method.

**Worst by line volume:** `assets/settings/default.json` — the single `"agent"` key at
**L973–1183 (211 lines)** is the largest contiguous deletion in any data file, and the file overall
loses ~380 lines across 20 top-level keys.

### Silent-failure ranking (highest risk first)

1. **Keymap action strings (403)** — hard runtime errors surfaced to the user as keymap parse failures.
2. **`assets/settings/default.json` keys (20 top-level + 1 nested)** — unknown keys in *default* settings
   are a developer-facing assertion in Zed's settings loader; users' *existing* `settings.json` files
   containing `"agent"`, `"language_models"`, `"edit_predictions"`, `"collaboration_panel"` etc. will
   emit "unknown setting" warnings after the deletion. **Migration path needed** for existing user configs.
3. **`docs/src/SUMMARY.md`** — hard `mdbook build` failure on dangling links.
4. **Keymap context predicates (~90)** — silent no-ops, cosmetic only.
5. **Orphaned assets (icons/sounds/prompts)** — dead bytes, zero runtime risk.

### Open questions for the implementer

- `assets/settings/default.json` L492–509 `"audio"` — the `audio` crate is NOT in the delete list.
  Confirm whether these three `experimental.*` keys are livekit-only before removing.
- L2472 `"server_url"` — owned by `client`, which is not in the delete list. Verify.
- L1424–1429 `"telemetry"` — `telemetry` crate survives, but `crashes` (the `diagnostics` consumer) does not.
- `title_bar.show_user_picture` / `show_user_menu` / `show_sign_in` / `show_onboarding_banner` —
  these depend on auth; decide whether they become no-ops or get removed.
- `IconName` variants for `bell*` / `user*` / `chat` — check for non-collab consumers before pruning SVGs.
- The `"Prompt"` key context in the three default keymaps (macOS L960, linux L905, windows L906) —
  verify this is the generic modal-prompt context and not agent-related before touching it.
