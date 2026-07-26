# Scout Report — Settings Schema Blast Radius & User Migration Story

Work context: `/Users/tgiap.dev/devs/zode`
Scope: deleting 56 crates (agent*, language_model*, edit_prediction*, copilot*, codestral, collab_ui, call, channel, context_server, auto_update, web_search*, remote_server, remote_connection, zeta*, supermaven).

---

## VERDICT (read this first)

### (a) Settings migration — **OPTIONAL** (recommended as polish, not required for correctness)

Zed's settings deserialization is **permissive**: there is **no `#[serde(deny_unknown_fields)]` anywhere in `settings_content` or `settings`**. Unknown keys are silently dropped by serde. A user's `settings.json` full of `"agent"`, `"language_models"`, `"edit_predictions"` etc. will load fine and the editor will start normally after those struct fields are deleted.

Evidence:
- `crates/settings_content/src/fallible_options.rs:11-44` — `parse_json` is the single entry point for user settings; it constructs a plain `serde_json_lenient::Deserializer` and calls `T::deserialize`. No `deny_unknown_fields`, no unknown-key collector, no error accumulation for unknown keys. It only accumulates **type** errors (via `fallible_options::deserialize`, line 46-62) so a wrong-typed value degrades to `Default` and reports `ParseStatus::Failed` with a message — unknown keys never reach that path.
- `grep -rn "deny_unknown_fields" crates/` → **zero hits in `settings_content/`, `settings/`, `settings_macros/`, `settings_json/`.** All 40+ hits are in `crates/vim/` (action structs, unrelated).
- `crates/settings_content/src/settings_content.rs:265-300` — `RootUserSettings::parse_json` for `SettingsContent`, `Option<SettingsContent>`, `UserSettingsContent` all delegate to `fallible_options::parse_json`.
- `crates/settings_content/src/settings_content.rs:80-100` — `SettingsContent` uses six `#[serde(flatten)]` sub-structs. `deny_unknown_fields` is **structurally impossible** with `flatten` in serde, so this is by design and cannot regress.
- `crates/settings/src/settings_store.rs:2096` — there is an explicit regression test named **`test_edits_for_update_preserves_unknown_keys`**. Unknown-key tolerance is a tested, intentional property of this codebase.

**The one real user-visible cost:** the JSON schema *is* strict. `crates/util/src/schemars.rs:38-55` defines `DefaultDenyUnknownFields`, a schemars transform that sets `"additionalProperties": false` on every generated object schema. It is applied at `crates/settings/src/settings_store.rs:1235` (`json_schema`) and `:1289-1290` (`project_json_schema`), and that schema is served to the built-in JSON language server over `zed://schemas/settings` (`crates/json_schema_store/src/json_schema_store.rs:310-370`). So after deletion, a user who opens their `settings.json` sees red squiggles — *"Property agent is not allowed"* — on every stale key. Annoying, cosmetic, non-fatal.

→ **Recommendation:** ship one JSON migration (`m_2026_XX_XX::remove_deleted_feature_settings`) that strips the dead keys. It is cheap (~80 lines, the engine already supports removal), it produces the migration banner + automatic backup UX users already know, and it eliminates the squiggle noise. But it does **not** gate the deletion phases — deletion can land first and the migration can follow.

### (b) Keymap migration — **OPTIONAL for user keymaps; MANDATORY asset cleanup (hard startup panic)**

Two very different code paths:

| Path | Behavior on unknown action | Evidence |
|---|---|---|
| **User keymap** (`~/.config/zed/keymap.json`) | Per-binding skip + aggregated error notification. Editor still starts, other bindings still load. | `crates/settings/src/keymap_file.rs:237-378` (`load` accumulates into `errors`, returns `SomeFailedToLoad { key_bindings, error_message }`); `crates/zed/src/zed.rs:1945-1962` handles all three variants and only surfaces a notification. |
| **Bundled asset keymaps** (`assets/keymaps/default-*.json`, `assets/keymaps/{macos,linux}/*.json`, `vim.json`) | **`anyhow::bail!` → `.unwrap()` → PANIC at startup.** | `crates/settings/src/keymap_file.rs:180-202` (`load_asset` bails on `SomeFailedToLoad`); `crates/zed/src/zed.rs:2066`, `:2070`, `:2075` all call `KeymapFile::load_asset(...).unwrap()`. |

Unknown action names produce `ActionBuildError::NotFound` → `Err("didn't find an action named ...")` at `crates/settings/src/keymap_file.rs:540-547`.

→ **Therefore: purging deleted-crate actions from `assets/keymaps/*.json` is a HARD, non-negotiable requirement of the deletion phase.** Zed will not boot otherwise. This is not a "migration" — it is part of the delete.
→ A *user*-keymap migration (rewriting stale bindings out of `~/.config/zed/keymap.json`) is optional and purely to suppress the notification.

### (c) Complete key list — see §4.

---

## 1. `crates/settings_content/` — audit

### 1.1 `language_model_core` dependency — trivially removable (4 references)

`crates/settings_content/Cargo.toml:24` — `language_model_core.workspace = true`

| file:line | reference |
|---|---|
| `crates/settings_content/src/language_model.rs:240` | `pub use language_model_core::ReasoningEffort as OpenAiReasoningEffort;` |
| `crates/settings_content/src/language_model.rs:482` | `pub use language_model_core::ModelMode;` |
| `crates/settings_content/src/agent.rs:428` | `pub speed: Option<language_model_core::Speed>,` (field of `LanguageModelSelection`) |
| `crates/settings_content/src/merge_from.rs:59` | `language_model_core::Speed,` inside the `merge_from_overwrites!` macro list |

Three of the four live in files that are deleted wholesale. Only `merge_from.rs:59` is a surgical one-line edit.

### 1.2 Files to delete wholesale

| file | lines | contents |
|---|---|---|
| `crates/settings_content/src/agent.rs` | 903 | `AgentSettingsContent`, `LanguageModelSelection`, `LanguageModelParameters`, `AgentProfileContent`, `ToolPermissionsContent`, `NotifyWhenAgentWaiting`, `PlaySoundWhenAgentDone`, `ThinkingBlockDisplay`, `NewThreadLocation`, `SidebarDockPosition` |
| `crates/settings_content/src/language_model.rs` | 488 | `AllLanguageModelSettingsContent` + all 15 provider sub-structs (anthropic, bedrock, deepseek, google, lmstudio, mistral, ollama, opencode, open_router, openai, openai_compatible, vercel, vercel_ai_gateway, x_ai, `zed.dev`) |

Wiring to remove: `crates/settings_content/src/settings_content.rs:2` (`mod agent;`), `:7` (`mod language_model;`), `:17` (`pub use agent::*;`), `:22` (`pub use language_model::*;`).

### 1.3 Fields to remove from surviving files

| file:line | field | owner |
|---|---|---|
| `settings_content.rs:90` | `pub agent: Option<AgentSettingsContent>` | agent_settings |
| `settings_content.rs:91` | `pub agent_servers: Option<AllAgentServersSettings>` | agent_servers (verify — ACP crate may survive) |
| `settings_content.rs:99` | `pub auto_update: Option<bool>` | auto_update |
| `settings_content.rs:109` | `pub collaboration_panel: Option<PanelSettingsContent>` | collab_ui |
| `settings_content.rs:142` | `pub language_models: Option<AllLanguageModelSettingsContent>` | language_models |
| `settings_content.rs:149` | `pub message_editor: Option<MessageEditorSettings>` | **already orphaned** — struct at `:694`, no consumer in repo outside `settings_content` |
| `settings_content.rs:157` | `pub server_url: Option<String>` | client/collab (`crates/client/src/client.rs:115`) |
| `settings_content.rs:175` | `pub calls: Option<CallSettingsContent>` | call (`crates/call/src/call_settings.rs:11`) |
| `settings_content.rs:1058-1063` | `RemoteSettingsContent { ssh_connections, wsl_connections, dev_container_connections, read_ssh_config, use_podman }` | remote_connection / remote_server |
| `project.rs:~55` | `context_servers: HashMap<Arc<str>, ContextServerSettingsContent>` | context_server |
| `project.rs:~56` | `context_server_timeout: Option<u64>` | context_server |
| `project.rs:~60` | `disable_ai: Option<SaturatingBool>` | AI kill-switch, now moot |
| `language.rs:38` | `edit_predictions: Option<EditPredictionSettingsContent>` (struct at `language.rs:~470`) | edit_prediction |
| `language.rs:80-130` | `enum EditPredictionProvider` + custom `Deserialize` impl | edit_prediction |
| `language.rs:514` | `show_edit_predictions: Option<bool>` (per-language) | edit_prediction |
| `language.rs:521` | `edit_predictions_disabled_in: Option<Vec<String>>` (per-language) | edit_prediction |
| `workspace.rs:~30` | `show_call_status_icon: Option<bool>` | call (`crates/workspace/src/workspace_settings.rs:100`) |
| `theme.rs` | `agent_ui_font_size`, `agent_buffer_font_size` | agent_ui |
| `editor.rs` | `toolbar.agent_review` | agent_ui |

### 1.4 How the schema is generated, and what breaks

- Every content struct derives `schemars::JsonSchema` (`#[derive(..., JsonSchema, MergeFrom)]`) plus the in-house `#[with_fallible_options]` proc macro from `settings_macros`.
- Root generation: `SettingsStore::json_schema()` at `crates/settings/src/settings_store.rs:1233-1285`, and `project_json_schema()` at `:1287+`. Both build a `schemars::SchemaGenerator` with two transforms: `DefaultDenyUnknownFields` and `AllowTrailingCommas` (`:1235-1236`, `:1289-1290`), then call `UserSettingsContent::json_schema(&mut generator)` (`:1239`).
- Runtime-injected sub-schemas (theme names, font names, language names, LSP adapter names) go through `util::schemars::replace_subschema` / `add_new_subschema` (`crates/util/src/schemars.rs:14-36`).
- **`replace_subschema` asserts** at `crates/util/src/schemars.rs:20-21` that (i) `{name}2` does not exist and (ii) `{name}` *does* exist in the definitions map. If a deletion removes the last reference to a type that is later `replace_subschema`'d, this **panics**. Audit `settings_store.rs:1167-1232` for any `replace_subschema::<T>` where `T` only reaches the schema through an agent/LLM path.
- Nothing breaks at compile time from the derive itself — delete the struct, delete the field, done.

---

## 2. `crates/settings/` — SettingsStore + the migration engine

### 2.1 Where migrations live

`crates/migrator/` is a standalone crate (its own `Cargo.toml`), depended on by `crates/settings` (`crates/settings/Cargo.toml`, `migrator.workspace = true`).

```
crates/migrator/src/
├── migrator.rs            (5372 lines — engine + the ordered migration list)
├── migrations.rs          (shared helpers: migrate_settings, migrate_language_setting)
├── migrations/m_YYYY_MM_DD/{settings.rs,keymap.rs}   (38 dated migrations)
└── patterns/{settings.rs,keymap.rs}                  (tree-sitter query strings)
```

### 2.2 How it works

There is **no version number stored anywhere**. The mechanism is: *run every migration, in list order, every time.* Each migration is idempotent — it looks for the old shape and rewrites it; if the old shape is absent it is a no-op. The design contract is documented at `crates/migrator/src/migrator.rs:1-15`:

> "You *must not* modify previous migrations; always create new ones instead. […] You only need to write replacement logic for x-1 to x."

Two migration flavors (`crates/migrator/src/migrator.rs:154-157`):

```rust
enum MigrationType<'a> {
    TreeSitter(MigrationPatterns, &'a Query),   // text-level, preserves comments/formatting
    Json(fn(&mut serde_json::Value) -> Result<()>),  // value-level, then diffed back to text
}
```

- **TreeSitter**: parses the JSON with `tree_sitter_json`, runs a query (patterns in `crates/migrator/src/patterns/settings.rs`), each match callback returns `Option<(Range<usize>, String)>` byte-range replacements. Edits are sorted, de-overlapped, applied in reverse (`migrator.rs:28-66`).
- **Json**: parses with `parse_json_with_comments` into a `serde_json::Value`, hands the callback `&mut Value`, then diffs old vs new via `settings_json::update_value_in_json_text` to produce **minimal text edits that preserve the user's comments and formatting** (`migrator.rs:79-113`). This is the modern flavor — every migration since 2025-10 uses it.

`run_migrations` (`migrator.rs:69-115`) chains them: the output of migration *n* is the input of *n+1*. Returns `Ok(None)` if the text is unchanged.

Ordered list for settings: `crates/migrator/src/migrator.rs:159-253` (`pub fn migrate_settings`). For keymaps: `:115-152` (`pub fn migrate_keymap`).

### 2.3 The shared helper that makes removal easy

`crates/migrator/src/migrations.rs:7-47` — `migrate_settings(value, &mut migrate_one)` applies your callback to:
1. the root object,
2. every release-channel override (`dev`/`nightly`/`preview`/`stable`, via `ReleaseChannelOverrides::OVERRIDE_KEYS`),
3. every platform override (via `PlatformOverrides::OVERRIDE_KEYS`),
4. every entry under `profiles.*` (and `profiles.*.settings`).

So one callback covers all override scopes automatically.

### 2.4 **YES — the engine can strip removed keys. Proof of prior art.**

`crates/migrator/src/migrations/m_2025_11_25/settings.rs` is a pure key-removal migration:

```rust
pub fn remove_context_server_source(value: &mut Value) -> Result<()> {
    migrate_settings(value, &mut migrate_one)
}
fn migrate_one(obj: &mut serde_json::Map<String, Value>) -> Result<()> {
    if let Some(context_servers) = obj.get_mut("context_servers") { ... server_obj.remove("source"); }
    Ok(())
}
```

Also `m_2025_10_02::remove_formatters_on_save`. A top-level strip is one `object.remove("agent")` per key.

### 2.5 Where migrations fire (two places, different semantics)

**(A) In-memory, every load** — `crates/settings/src/settings_store.rs:779-810` (`parse_and_migrate_zed_settings`):
```rust
let migration_res = migrator::migrate_settings(user_settings_content);
migration_status = match &migration_res { Ok(Some(_)) => MigrationStatus::Succeeded, ... };
let content = match &migration_res { Ok(Some(c)) => c, Ok(None)|Err(_) => user_settings_content };
SettingsContentType::parse_json(content)
```
The user's file on disk is **not** touched. Migration result is recorded in `self.file_errors`.

**(B) On-disk, user-triggered via banner** — `crates/zed/src/zed/migrate.rs`:
- `should_migrate_settings` `:270-276` / `should_migrate_keymap` `:262-268` — dry run.
- `MigrationBanner` `:24-140` — a toolbar item that appears when the user opens `settings.json`/`keymap.json` and a migration is pending: *"Your settings file uses deprecated settings which can be automatically updated. A backup will be saved to `settings_backup.json`"* with a Backup-and-Update button.
- Write path at `:280-320`.
- `SettingsParseResult::requires_user_action()` at `crates/settings/src/settings_store.rs:1487-1493` returns `true` when `migration_status == Succeeded` — this is what drives the banner.

**Practical consequence:** adding a strip migration gives you the banner + automatic backup for free. Users get a one-click clean-up with their old file preserved.

### 2.6 Default settings are NOT permissive about types

`crates/settings/src/settings_store.rs:294-295`:
```rust
let default_settings: SettingsContent =
    SettingsContent::parse_json_with_comments(default_settings).unwrap();
```
`parse_json_with_comments` is the **strict** path (no error-swallowing — see the assertion at `crates/settings_content/src/fallible_options.rs:101`). It still ignores unknown keys, so leaving a stale `"agent": {...}` block in `assets/settings/default.json` will not panic — but it *will* rot.

**The real trap:** dozens of `Settings` impls do `content.<key>.unwrap()` because `default.json` is contractually complete (e.g. `crates/auto_update/src/auto_update.rs:217` `Self(content.auto_update.unwrap())`, `crates/workspace/src/workspace_settings.rs:100` `show_call_status_icon: workspace.show_call_status_icon.unwrap()`, `crates/call/src/call_settings.rs:11` `content.calls.clone().unwrap()`). **Removing a key from `assets/settings/default.json` while any consumer still unwraps it = startup panic.** Field, default.json entry, and consumer must be removed in the same commit.

---

## 3. `crates/settings_ui/` — 7 delete-set edges

### Cargo.toml deps in the delete set
`agent` (:20), `agent_settings` (:21), `codestral` (:25), `copilot` (:26), `copilot_ui` (:27), `edit_prediction` (:29), `edit_prediction_ui` (:30).
Collateral: `audio`/`cpal`/`rodio` exist only for the Collaboration page's device pickers; `zed_credentials_provider` only for the edit-prediction setup page.

### Import sites
- `src/pages/edit_prediction_provider_setup.rs:1` (codestral), `:2-6` (edit_prediction), `:7` (edit_prediction_ui), `:724`/`:730` (copilot_ui), `:727` (copilot)
- `src/pages/tool_permissions_setup.rs:1` (agent), `:2` (agent_settings), `:1431`/`:1452` (agent, tests)
- `src/components/ollama_model_picker.rs:30` (`edit_prediction::ollama::fetch_models`)

### Delete whole files
| file | lines |
|---|---|
| `crates/settings_ui/src/pages/edit_prediction_provider_setup.rs` | 748 |
| `crates/settings_ui/src/pages/tool_permissions_setup.rs` | 1459 |
| `crates/settings_ui/src/components/ollama_model_picker.rs` | 213 |
| *(conditional, with Collaboration page)* `src/pages/audio_test_window.rs` | 304 |
| *(conditional)* `src/pages/audio_input_output_setup.rs` | 153 |

### `page_data.rs` surgery (`crates/settings_ui/src/page_data.rs`)
Registry root `settings_data()` at `:63-86`, page vec `:64-79`.

**Whole pages:**
- `:7211-7310` `collaboration_page()` — "Collaboration"; sections "Calls" (`:7214`, items Mute On Join `:7216`, Share On Join `:7229`) and `audio_settings()` (`:7246-7300`).
- `:7312-7692` `ai_page(cx)` — "AI". Sub-blocks: `general_section()` `:7313-7343` (Disable AI `:7317`, Threads Sidebar Side `:7330`); `agent_configuration_section()` `:7345-7630` (Tool Permissions sub-page link `:7349-7358` + 14 items); `context_servers_section()` `:7632-7651`; `edit_prediction_display_sub_section()` `:7653-7679`.
- Remove `:76 collaboration_page(),` and `:77 ai_page(cx),` from the page vec.

**Sections inside surviving pages:**
| lines | page | section |
|---|---|---|
| `:372-389` (+ call site `:400`) | general | `auto_update_section()` |
| `:998-1040` (+ call site `:1256`) | appearance | `agent_panel_font_section()` (`agent_ui_font_size`, `agent_buffer_font_size`) |
| `:2487-2510` | editor/toolbar | item `Agent Review`; **also fix arity `:2412` `[SettingsPageItem; 6]` → `; 5`** |
| `:5688-5754` (+ call site `:5895`) | panels | `collaboration_panel_section()` |
| `:5756-5885` (+ call site `:5896`) | panels | `agent_panel_section()` |
| `:9364-9417` (+ call sites `:3087`, `:7688`) | shared | `edit_prediction_language_settings_section()` |
| `:7694-7737` | network | `Server URL` item `:7715`; arity `:7695` `; 3` → `; 2` |

**Import prunes:** `page_data.rs:14-17` (`render_edit_prediction_setup_page`, `render_tool_permissions_setup_page`, `open_audio_test_window`), `:25-28` (audio device defaults), `:3-6` (`AudioInputDeviceName`/`AudioOutputDeviceName`).

**Module wiring:** `src/pages.rs:3,5,11,13,15-20`; `src/components.rs:6,15`; `src/settings_ui.rs:51` (import), `:53` (audio, conditional), `:555` (`.add_basic_renderer::<settings::OllamaModelName>`), and likely-orphaned renderer registrations at `:505`, `:528`, `:529`, `:550`.

**Cross-crate breakage:** `crates/zed/src/visual_test_runner.rs:2438` references `settings_ui::pages::render_terminal_tool_config`.

---

## 4. COMPLETE KEY LIST TO STRIP

### Top-level keys
```
agent
agent_servers            (verify: agent_servers/ACP crate may survive the cut)
language_models
edit_predictions
auto_update
collaboration_panel
calls
context_servers
context_server_timeout
disable_ai
server_url
message_editor           (already dead code today — no consumer)
show_call_status_icon
ssh_connections
wsl_connections
dev_container_connections
read_ssh_config
use_podman
agent_ui_font_size
agent_buffer_font_size
```

### Nested keys
```
toolbar.agent_review
edit_predictions.provider
edit_predictions.disabled_globs
edit_predictions.mode
edit_predictions.copilot
edit_predictions.codestral
edit_predictions.ollama
edit_predictions.open_ai_compatible_api
edit_predictions.examples_dir
defaults.show_edit_predictions
defaults.edit_predictions_disabled_in
languages.<Name>.show_edit_predictions
languages.<Name>.edit_predictions_disabled_in
```

### Deep sub-trees removed implicitly with their parent
`agent.*` (37 fields incl. `default_model`, `favorite_models`, `inline_assistant_model`, `commit_message_model`, `thread_summary_model`, `inline_alternatives`, `profiles.*`, `tool_permissions.*`, `model_parameters`), `language_models.{anthropic,bedrock,deepseek,google,lmstudio,mistral,ollama,opencode,open_router,openai,openai_compatible,vercel,vercel_ai_gateway,x_ai,"zed.dev"}`, `calls.{mute_on_join,share_on_join}`, `collaboration_panel.{button,dock,default_width}`.

### Keys the delete set does NOT touch (do not strip)
`telemetry`, `repl`, `debugger`, `dap`, `feature_flags`, `proxy`, `session`, `node`, `extension.*`, `git*`, everything editor/theme/terminal/workspace.

### Already-migrated legacy shapes — no action needed
- `features.edit_prediction_provider` → already rewritten to `edit_predictions.provider` by `m_2026_02_02::move_edit_prediction_provider_to_edit_predictions`. There is no `features` field in `SettingsContent` today.
- `assistant` → already collapsed into `agent` by earlier tree-sitter migrations (`patterns/settings.rs` `SETTINGS_ASSISTANT_PATTERN`, `SETTINGS_DUPLICATED_AGENT_PATTERN`).
- `chat_panel` / `notification_panel` → **do not exist** in `settings_content` in this tree. Nothing to strip.

### `assets/settings/default.json` lines to delete (must be same commit as the struct fields)
`:450 calls`, `:904 collaboration_panel`, `:968 message_editor`, `:973 agent`, `:1185 show_call_status_icon`, `:1433 disable_ai`, `:1436 auto_update`, `:1600 edit_predictions`, `:2272 language_models`, `:2472 server_url`, `:2548 ssh_connections`, `:2550 read_ssh_config`, `:2561 context_server_timeout`, `:2563 context_servers`, `:2565 agent_servers`.

---

## 5. Keymap details

### Behavior matrix
| Scenario | Result |
|---|---|
| User `keymap.json` names a deleted action | `Err("didn't find an action named \"agent::NewThread\"")` at `keymap_file.rs:540-547`, collected into `errors`, returned as `SomeFailedToLoad`. `crates/zed/src/zed.rs:1949-1957` shows a notification; **all other bindings still load; editor starts.** |
| Bundled `assets/keymaps/default-{macos,linux,windows}.json` names a deleted action | `load_asset` → `anyhow::bail!` (`keymap_file.rs:195-197`) → `.unwrap()` at `crates/zed/src/zed.rs:2066` → **PANIC** |
| Bundled base keymap (`assets/keymaps/{macos,linux}/{atom,cursor,emacs,jetbrains,sublime_text,textmate}.json`) | same panic path, `crates/zed/src/zed.rs:2070` |
| Bundled `assets/keymaps/vim.json` | same panic path, `crates/zed/src/zed.rs:2075` |
| `load_asset_allow_partial_failure` (`keymap_file.rs:204-222`) — only bails if **zero** bindings load | used by tests + `crates/agent_ui/src/agent_panel.rs:6380`, `crates/search/src/buffer_search.rs:2018,2061`, `crates/editor/src/editor_tests.rs:26749`, `crates/vim/src/test/vim_test_context.rs:102,113` |

### `deny_unknown_fields` on keymap structs
Not literally applied, but hand-rolled: `KeymapSection` collects leftovers into `unrecognized_fields: IndexMap<String, Value>` (`keymap_file.rs:91`, `:255`) and reports them as a section error (`:277-283`). Comment at `:580` explains the intent. The keymap **schema** does set `"additionalProperties": false` explicitly (`keymap_file.rs:786`, `:806`).

### Scale of the cleanup
`assets/keymaps/default-macos.json` alone contains ~50 `agent::*` actions plus `agents_sidebar::*`, `assistant::InlineAssist`, `channel_modal::*`, `collab_panel::*`, `copilot::*`, `edit_prediction::*`, `zeta::*`. Multiply across 3 default keymaps + 11 base keymaps + `vim.json`.

**Partial reprieve:** `crates/zed_actions` is *not* in the delete set and declares some actions in the `agent`, `assistant`, and `collab` namespaces (`crates/zed_actions/src/lib.rs:116`, `:486`, `:520`, `:540`, `:549`, `:557`, `:576`, `:585`). Those stay registered, so a subset of bindings survives. Build the removal list from `cargo run -p zed -- --dump-all-actions` **after** the crate deletion, and diff against the keymap assets — do not hand-curate by namespace prefix.

---

## 6. JSON schema publication

- **Nothing is generated at build time and nothing is checked in.** No `build.rs` in any of the 17 crates that have one emits a schema. The only checked-in schemas are third-party, vendored from SchemaStore by `script/update-json-schemas`: `crates/json_schema_store/src/schemas/{package.json,tsconfig.json}` (`include_str!` at `json_schema_store.rs:17-18`).
- There is an ad-hoc CLI emitter, `crates/schema_generator/src/main.rs` (`cargo run -p schema_generator {theme|icon_theme|project}`), registered at workspace `Cargo.toml:173`. It prints to stdout and **no script or CI job invokes it** — safe to ignore.
- Zed's file→schema associations are built in `all_schema_file_associations` (`json_schema_store.rs:431+`): `paths::settings_file()` → `zed://schemas/settings`, project-local settings → `zed://schemas/project_settings`, `paths::keymap_file()` → `zed://schemas/keymap`. These are handed to the JSON LSP as its `json.schemas` workspace config at `crates/languages/src/json.rs:276`.
- The settings schema is built **at runtime** from live registry state (installed fonts, themes, languages, LSP adapters, registered actions) in `crates/json_schema_store/src/json_schema_store.rs:310-370`, cached in `DYNAMIC_SCHEMA_CACHE` (`:53-54`), invalidated on extension/DAP changes, and served to the built-in JSON LSP under `zed://schemas/settings` and `zed://schemas/project_settings` via `project::lsp_store::json_language_server_ext::register_schema_handler` (`:57-61`). The keymap schema is `zed://schemas/keymap` (`:413`), per-action schemas `zed://schemas/action/*`.
- **Nothing breaks** structurally — the schema regenerates from whatever types exist. The only effect is that stale user keys become schema violations (see verdict (a)).
- **Watch `replace_subschema`'s assertions** (`crates/util/src/schemars.rs:20-21`) — see §1.4.

### Tests / build steps that WILL break
| location | what breaks |
|---|---|
| `crates/settings/src/settings_store.rs:2871-2872` | `assert!(user_schema_str.contains("\"auto_update\""));` — hard fail once `auto_update` is removed. Test `test_project_json_schema_differs_from_user_schema` at `:2853`. |
| `crates/settings/src/settings_store.rs:2035`, `:2109` | test fixtures set `settings.auto_update = Some(true)` |
| `crates/settings/src/settings_store.rs:1634-1645` | test-only `impl Settings for AutoUpdateSetting { content.auto_update.unwrap() }` |
| `crates/docs_preprocessor/src/main.rs:370-390` | mdbook preprocessor compiles the settings schema and **validates every labeled JSON code block in `docs/`** against it. Any doc snippet containing `"agent"`, `"language_models"`, `"edit_predictions"` etc. becomes a docs-build error. |
| `crates/docs_preprocessor/src/main.rs:254` (`template_and_validate_keybindings`), `:302` (`template_and_validate_actions`), `:193-200` | Any `{#kb agent::NewThread}` / `{#action ...}` reference in `docs/` to a deleted action → `PreprocessorError::ActionNotFound` → `handle_preprocessing` returns `Err` → **the `check_docs` CI job fails** (`.github/workflows/run_tests.yml:636-680`). `docs/src/**` must be swept for these. |
| `crates/docs_preprocessor/actions.json` | **Not committed** — regenerated by `script/generate-action-metadata` locally and in CI. `load_all_actions` (`main.rs:622`) **panics** if missing when `CI` is set. Nothing to update manually, but it means the docs job re-derives the action list from the post-deletion binary. |
| `crates/settings_ui/src/pages/tool_permissions_setup.rs:1431,1452` | tests over `agent::ALL_TOOL_NAMES` — deleted with the file |
| `crates/zed/src/zed.rs:5593` | `test_disable_ai_crash` — tied to the `disable_ai` key |
| `crates/zed/src/zed.rs:5064` | `test_generate_keymap_json_schema_for_registered_actions` — smoke test; also the tripwire for `util::schemars` assertion panics |
| **`crates/zed/src/zed.rs:5077` `test_action_namespaces`** | **HARD BREAK.** Asserts `cx.all_action_names()` namespaces equal a hardcoded list at `:5110-5198` which includes `"copilot"`, `"collab"`, `"zeta"`, `"agent"`, `"assistant"`, `"channel"`, … Deleting any namespace-owning crate fails the `assert_eq!`. Must be edited in the deletion commit. |
| `crates/settings/src/keymap_file.rs:1675` | `keymap_schema_for_unbind_excludes_null_and_unbind_action` — inspects `/$defs/UnbindTargetAction` |
| `crates/settings/src/settings_file.rs:104-110`, `:127-133` | `visual_test_settings` / `test_settings` both `.unwrap()` the parse of `default.json` — a malformed default.json fails effectively every test in the repo |
| `crates/settings/src/settings_store.rs:2743`, `:2798` | `test_lsp_settings_schema_generation` / `test_lsp_project_settings_schema_generation` — structural, unlikely to break but touch the same schema path |

---

## 7. Recommended sequencing

1. **Phase D-keymap (blocking, part of deletion):** after crates are removed, run `cargo run -p zed -- --dump-all-actions` (emitter at `crates/zed/src/main.rs:1882-1925`), diff against every file in `assets/keymaps/`, delete orphaned bindings. Without this Zed panics on boot. Same pass: update the hardcoded namespace list in `crates/zed/src/zed.rs:5110-5198`, and sweep `docs/src/**` for `{#kb ...}` / `{#action ...}` references to deleted actions or the `check_docs` CI job fails.
2. **Phase D-settings (blocking, part of deletion):** remove struct fields, `assets/settings/default.json` entries, and unwrapping consumers **atomically**. Fix `settings_store.rs:2871` and the other tests in §6.
3. **Phase M (optional, post-deletion polish):** add `crates/migrator/src/migrations/m_2026_XX_XX/settings.rs` with a `MigrationType::Json` callback that `object.remove(...)`s the §4 key list, wired into the list at `crates/migrator/src/migrator.rs:~252`. Model it on `m_2025_11_25`. Users get the migration banner + automatic backup. ~80 lines.
4. **Phase M-keymap (optional):** a `MigrationType::TreeSitter` keymap migration that drops bindings to dead actions. Lower value than (3) — the user-keymap failure mode is already a soft notification.

### Confidence
- `deny_unknown_fields` absence / permissive deserialization: **EXTRACTED, 0.97** (grep-exhaustive + `flatten` makes it structurally impossible).
- Bundled-keymap startup panic: **EXTRACTED, 0.95** (`.unwrap()` on a `bail!`ing function, three call sites).
- Migration engine supports key removal: **EXTRACTED, 0.99** (two shipped precedents).
- `agent_servers` in/out of the delete set: **AMBIGUOUS, 0.5** — the ACP `agent_servers` crate was not named in the delete list; confirm before stripping that key.
