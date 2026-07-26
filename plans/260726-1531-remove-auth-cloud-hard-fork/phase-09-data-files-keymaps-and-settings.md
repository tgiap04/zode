---
phase: 9
title: "Data files keymaps and settings"
status: pending
effort: "3-4d"
---

# Phase 9: Data files keymaps and settings

## Context Links

- [Data-file blast radius](./reports/scout-data-files.md) — the 403 action strings, file by file
- [Settings scout](./reports/scout-settings-migration.md) §5 — the panic path
- [`research/baseline-action-inventory.md`](./research/baseline-action-inventory.md) — **captured in Phase 1**: 1,292 actions / 88 namespaces, 142 doomed + 2 partial
- [`research/baseline-action-names.txt`](./research/baseline-action-names.txt) — the sorted name list to diff against

## Overview

**Priority:** P0 — **highest-risk phase in the plan** · **Build state:** 🔴 RED → runtime-correct · **Depends on:** Phase 8

The compiler cannot see any of this. **403 action strings across 8 keymap files and 21 settings keys** reference deleted crates. A single leftover keymap action **panics Zed at startup**:

```
keymap_file.rs:196/:199  anyhow::bail!  →  zed.rs:2066  .unwrap()  →  PANIC
```

This is not cleanup deferred to the end. It is a hard correctness requirement of the deletion.

## Key Insights

- **Estimate raised 2d → 3-4d** (red team finding 9). This is the plan's own P0 phase with an all-or-nothing gate (`cargo run` starts or it does not) — a 2d budget with no margin was inconsistent with that label.
- **Bundled keymaps panic; user keymaps do not.** `load_asset` bails on any unresolvable action and `zed.rs:2066`/`:2070`/`:2075` all `.unwrap()` it. A user's own `keymap.json` gets a soft per-binding skip plus a notification (`keymap_file.rs:237-378`, `zed.rs:1942-1962`).
- **Do not hand-curate the removal list by namespace prefix.** Two independent reasons, both now measured:
  - `crates/zed_actions` survives and *still declares* actions in the `agent`, `assistant`, and `collab` namespaces (`zed_actions/src/lib.rs:116`, `:486`, `:520`, `:540`, `:549`, `:557`, `:576`, `:585`). Some bindings with those prefixes stay valid.
  - Two **surviving** namespaces lose only *some* actions: `client` loses `SignIn`/`SignOut`/`Reconnect` (Phase 5) while keeping the namespace; `onboarding` loses `SignIn`/`OpenAccount` but keeps `Finish`/`ResetHints`. A prefix sweep sees neither.

  Diff the regenerated dump against `research/baseline-action-names.txt` instead.
- **Baseline measured in Phase 1:** 1,292 actions, 88 namespaces. **142** live in namespaces that vanish with their crate; `bedrock` (2) and `zed_predict_onboarding` (1) were **missing from the plan's original namespace list** — the dump caught them.
- **Settings are permissive** — no `deny_unknown_fields` anywhere (`fallible_options.rs:11-44`), plus a regression test asserting unknown-key preservation. Users' existing `settings.json` keeps loading. Migration is Phase 13 polish, not a blocker.
- **But `default.json` is not permissive in practice.** ~12 consumers do `content.<key>.unwrap()` because `default.json` is contractually complete — e.g. `auto_update.rs:217`, `workspace_settings.rs:100`, `call_settings.rs:11`. **Field + `default.json` entry + unwrapping consumer must die in the same commit** or startup panics.
- Unknown *key contexts* (`AgentPanel`, `agent_diff`) silently never match — ~90 of them, cosmetic only. Clean them, but they are not urgent.
- Worst files: `default-macos.json` (125 actions, 29 blocks), `default-linux.json` (119), `default-windows.json` (117). `assets/settings/default.json`'s `"agent"` key alone spans `:973-1183` (211 lines).

## Requirements

**Functional**
- Zed starts without panicking.
- Every bundled keymap loads cleanly on macOS, Linux, and Windows targets.
- `default.json` contains no key whose consumer was removed, and vice versa.

**Non-functional**
- The keymap removal list must be **derived** from a regenerated action dump, not hand-written.

## Architecture

```
Phase 1 baseline-actions.txt  ─┐
                               ├─► diff ─► authoritative removal list ─► 8 keymap files
Phase 8 post-deletion dump    ─┘

settings_content field  ─┐
default.json key        ─┼─► must be removed ATOMICALLY (same commit)
.unwrap() consumer      ─┘
```

## Related Code Files

**To modify**
- `assets/keymaps/default-macos.json` (125), `default-linux.json` (119), `default-windows.json` (117)
- `assets/keymaps/macos/cursor.json` (17), `linux/cursor.json` (17), `vim.json` (6)
- `assets/keymaps/macos/jetbrains.json` (1), `linux/jetbrains.json` (1)
- `assets/keymaps/{macos,linux}/sublime_text.json` — dead contexts only
- `assets/settings/default.json` — 21 keys (see list below)

**To delete**
- `assets/prompts/` (3 `.hbs` files)
- `assets/sounds/` (8 `.wav`: `agent_done`, `guest_joined_call`, `joined_call`, `leave_call`, `mute`, `unmute`, `start_screenshare`, `stop_screenshare`)
- ~48 orphaned icons in `assets/icons/` — `ai_*.svg` (18), `copilot*.svg` (4), `zed_agent*`/`zed_assistant`/`zed_predict*` (8), `thread*`/`threads_sidebar_*` (7), `mic`/`mic_mute`/`screen`/`chat` (4), `bell*` (4), `user_check`/`user_group`/`user_round_pen` (3)

**Do NOT touch**
- `assets/themes/*.json` — verified zero edits needed. **`players` must stay** in all three theme files: it is used by non-collab code (`editor.rs:26414`, `:29359`, `:29861`, `:10314` local cursor + read-only, `terminal.rs:2509`, `search_bar.rs:125`, `debugger_ui/console.rs:415`, `:891`, `memory_view.rs:320`, `extensions_ui.rs:1259`).
- `assets/settings/` other than `default.json` — all six are clean.

## Implementation Steps

### 9a. Keymaps — derive, then delete

1. Regenerate the post-deletion action list:
   ```sh
   cargo run -p zed -- --dump-all-actions > research/post-deletion-actions.json
   ```
   If `crates/zed` will not yet run, use `cargo check -p zed` green as the precondition and build the dump as soon as it links.
2. Compute the disappeared actions. The dump is **JSON** (`{"actions":[{"name":…}], "schema_definitions":…}`), so extract names first — a raw `comm` on the JSON would be meaningless:
   ```sh
   python3 -c "
   import json,sys
   print('\n'.join(sorted(a['name'] for a in json.load(open(sys.argv[1]))['actions'])))
   " research/post-deletion-actions.json > research/post-deletion-action-names.txt

   comm -23 research/baseline-action-names.txt research/post-deletion-action-names.txt \
     > research/removed-actions.txt
   wc -l research/removed-actions.txt    # expect ~142 + the client/onboarding partials
   ```
3. For each keymap file, remove every binding whose action appears in `removed-actions.txt`. Where a whole context block empties out, delete the block. Where a context predicate references a dead context (`&& !agent_diff`, `AgentPanel`), rewrite or drop the predicate.
4. Edit `default-macos.json`, `default-linux.json`, `default-windows.json` **in lockstep** — they mirror each other and diverge only in modifiers.
5. Clean the ~90 dead key contexts (cosmetic but cheap while you are in the files).
6. **Write a regression test** that loads every bundled keymap and asserts all actions resolve. No such test exists today — the panic path *is* the test. This is the single most valuable test to add in this whole plan.

### 9b. Settings — atomic triples

7. For each key below, remove **field + `default.json` entry + `.unwrap()` consumer in one commit**:

   | `default.json` line | Key | Consumer to check |
   |---|---|---|
   | `:71`, `:73` | `agent_ui_font_size`, `agent_buffer_font_size` | theme settings |
   | `:422`, `:425` | `show_edit_predictions`, `edit_predictions_disabled_in` | language settings |
   | `:450-455` | `calls` | `call_settings.rs:11` **unwraps** |
   | `:904-911` | `collaboration_panel` | panel settings |
   | `:968-972` | `message_editor` | already orphaned — no consumer |
   | `:973-1183` | `agent` (211 lines) | `agent_settings` (deleted) |
   | `:1185` | `show_call_status_icon` | `workspace_settings.rs:100` **unwraps** |
   | `:1433` | `disable_ai` | `project.rs:1087` |
   | `:1436` | `auto_update` | `auto_update.rs:217` **unwraps** |
   | `:1600-1657` | `edit_predictions` (+ copilot/codestral/ollama sub-keys) | edit-prediction settings |
   | `:2081` | `languages.Diff.show_edit_predictions` | nested |
   | `:2272-2312` | `language_models` (15 providers) | deleted |
   | `:2561`, `:2563` | `context_server_timeout`, `context_servers` | **Decision from Phase 8f**: if `context_server_store` was removed from `project`, these keys go too |
   | `:2565` | `agent_servers` | **Decision from Phase 7e.** Consumer is `crates/project/src/agent_server_store.rs:1896` — `content.agent_servers.clone().unwrap()`. Removing this key without resolving that unwrap is a **startup panic**. |
   | — | `file_finder.include_channels` | **carried over from Phase 7g** |

8. **Verify before removing:** `:492-509` `audio` (the `audio` crate survives — confirm the three `experimental.*` keys were livekit-only), `:1424-1429` `telemetry` (**keep** — `TelemetrySettings` retained in Phase 6b).

8b. **`server_url` (`:2472`) — keep, but understand what you are keeping** (red team finding 2). It is owned by `client`, which survives. But `HttpClientWithUrl::build_zed_api_url` (`http_client.rs:214-224`) turns it into `api.zed.dev`, and `ExtensionStore::install_extension` (`extension_host.rs:693`, `:792`, `:837`) calls exactly that. So keeping this key means **the rebranded fork keeps talking to Zed Industries' production API for every extension browse and install.**

   That is permitted egress under the commissioner's "vẫn cho phép tải về" — but it directly contradicts Phase 11's "zero outbound to `*.zed.dev`" requirement unless carved out. Keep the key here; **Phase 12 must decide** whether to stand up an independent registry or disclose the dependency. Phase 11 excludes it from the blackhole test.
9. `title_bar.show_sign_in` / `show_user_picture` / `show_user_menu` / `show_onboarding_banner` (`:479-485`) — **decide and record the decision in the Todo List**, same as every other row in this table. A "recommendation" has no place in the phase whose governing rule is that field, key, and consumer must die together. **Remove them**: the UI they gate is deleted in Phase 8b, so keeping the keys leaves settings that control nothing — which `CLAUDE.md` forbids as surely as a stub does.
10. **Watch `replace_subschema`** (`util/src/schemars.rs:20-21`): it asserts the named type exists in the definitions map. If a deletion removes the last reference to a type later passed to `replace_subschema`, it **panics**. Audit `settings_store.rs:1167-1232`.

### 9c. Menus and assets

11. `crates/zed/src/zed/app_menus.rs` — already handled in Phase 8d step 23; verify.
12. Delete `assets/prompts/`, `assets/sounds/`, and the ~48 orphaned icons. **Check `IconName` enum consumers first** for `Bell*`, `User*`, `Chat` — they may have non-collab uses. Prune the `IconName` variants by hand.
13. Optional: `crates/zed/resources/info/Permissions.plist` (`NSCameraUsageDescription`, `NSMicrophoneUsageDescription`, `NSSpeechRecognitionUsageDescription`) and `zed.entitlements` (`device.audio-input`, `device.camera`) — livekit-only, safe to drop.

### 9d. Gate — the one that matters

14. **Build and launch the app.** Not `cargo check` — actually run it:
    ```sh
    cargo run
    ```
    It must start without panicking. This is the definitive test for this phase.
15. Switch base keymap in settings to each of `vim`, `jetbrains`, `sublime_text`, `cursor`, `atom`, `emacs`, `textmate` and confirm no panic and no error toast.
16. Load an old `settings.json` containing `"agent"`, `"language_models"`, `"collaboration_panel"` and confirm the app starts and only shows schema squiggles.

## Todo List

- [ ] 9a Post-deletion action dump generated
- [ ] 9a `removed-actions.txt` derived by diff (**not hand-curated**)
- [ ] 9a All 8 keymap files purged; 3 defaults edited in lockstep
- [ ] 9a Dead key contexts cleaned
- [ ] 9a **Regression test added**: every bundled keymap loads, every action resolves
- [ ] 9b All settings triples removed atomically (field + default.json + consumer)
- [ ] 9b `audio`, `telemetry` keys verified and **kept**
- [ ] 9b `server_url` kept, and the extension-registry consequence recorded for Phase 12
- [ ] 9b `agent_servers` decision honoured from Phase 7e (unwrap at `agent_server_store.rs:1896` resolved)
- [ ] 9b `title_bar.show_*` keys **removed** (decision, not recommendation)
- [ ] 9b `replace_subschema` audit done
- [ ] 9c `assets/prompts/`, `assets/sounds/`, orphaned icons deleted; `IconName` pruned
- [ ] 9d **`cargo run` launches without panic**
- [ ] 9d All 7 base keymaps switch cleanly
- [ ] 9d Legacy `settings.json` loads without error

## Success Criteria

- **`cargo run` starts the editor.** Non-negotiable.
- Every bundled keymap loads on every base-keymap setting.
- The new keymap regression test passes.
- `rg -n "agent::|agents_sidebar::|collab_panel::|assistant::|zeta::|rules_library::|channel_modal::|inline_assistant::|edit_prediction::" assets/keymaps/` returns only actions that still exist in `zed_actions`.
- A legacy `settings.json` loads without error.

## Risk Assessment

| Risk | Impact | Mitigation |
|---|---|---|
| One leftover keymap action | **App will not start** | Derive from the diff, not by hand; add the regression test; `cargo run` is the gate |
| Hand-curating by namespace prefix | Deletes still-valid `zed_actions` bindings | Step 2 makes the list mechanical |
| `default.json` key removed but consumer still unwraps | **Startup panic** | Step 7 enforces atomic triples |
| Consumer removed but `default.json` key left | Dead key, schema squiggle, confusion | Same atomic rule, both directions |
| `players` theme key deleted as "collab" | Local cursor, terminal cursor, search, debugger all lose colors | Explicitly listed under "Do NOT touch" with 10 call sites cited |
| `replace_subschema` panic on a removed type | Runtime panic when the JSON LSP requests the schema | Step 10 audits it |
| `IconName` variant deleted while still used | Compile error (caught) or missing icon (not caught) | Step 12 requires a consumer check first |

## Security Considerations

None directly. Note that removing `NSMicrophoneUsageDescription` / `NSCameraUsageDescription` narrows the app's declared macOS permission surface — a genuine (if small) privacy improvement worth doing.

## Next Steps

Phase 10 — tests and docs, including the `expected_namespaces` canary deliberately deferred from Phase 8.
