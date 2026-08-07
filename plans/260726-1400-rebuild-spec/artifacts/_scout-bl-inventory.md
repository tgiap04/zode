## Background Logic Source Inventory

<!-- Stack = Rust/GPUI. No table row exists in bl-source-patterns.md for GPUI —
     all entries below use the [SIGNAL_INFERRED] protocol. Detection method: grep
     for GPUI-idiomatic background-logic markers (cx.spawn/cx.background_spawn +
     .detach() fire-and-forget pattern, Task<>, actions!/#[derive(Action)] macros,
     LSP/DAP adapter + external-process integration points, file/settings watchers,
     debounce/poll loops). Test files (paths containing /test/, /tests/, _test.rs,
     _tests.rs) and build.rs / examples/ files are excluded as non-runtime-app surface. -->

### Rust/GPUI

- custom-command: crates/activity_indicator/src/activity_indicator.rs [SIGNAL_INFERRED]
  - Intent matched: custom-command — user-triggered command registration
  - No-row reason: stack=Rust/GPUI, no row in bl-source-patterns.md table
  - Observed pattern: top-level `actions!(...)` macro invocation registering GPUI Actions dispatched via keybindings/command palette
- custom-command: crates/csv_preview/src/csv_preview.rs [SIGNAL_INFERRED]
- custom-command: crates/dev_container/src/lib.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/debugger_tools/src/dap_log.rs [SIGNAL_INFERRED]
- custom-command: crates/debugger_ui/src/debugger_ui.rs [SIGNAL_INFERRED]
- custom-command: crates/debugger_ui/src/new_process_modal.rs [SIGNAL_INFERRED]
- custom-command: crates/debugger_ui/src/session/running/breakpoint_list.rs [SIGNAL_INFERRED]
- custom-command: crates/debugger_ui/src/session/running/console.rs [SIGNAL_INFERRED]
- custom-command: crates/debugger_ui/src/session/running/memory_view.rs [SIGNAL_INFERRED]
- custom-command: crates/debugger_ui/src/session/running/variable_list.rs [SIGNAL_INFERRED]
- custom-command: crates/diagnostics/src/buffer_diagnostics.rs [SIGNAL_INFERRED]
- custom-command: crates/diagnostics/src/diagnostics.rs [SIGNAL_INFERRED]
- custom-command: crates/editor/src/actions.rs [SIGNAL_INFERRED]
- custom-command: crates/editor/src/split.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/encoding_selector/src/encoding_selector.rs [SIGNAL_INFERRED]
- custom-command: crates/etw_tracing/etw_tracing.rs [SIGNAL_INFERRED]
- custom-command: crates/extension_host/src/extension_host.rs [SIGNAL_INFERRED]
- custom-command: crates/extensions_ui/src/extensions_ui.rs [SIGNAL_INFERRED]
- custom-command: crates/feedback/src/feedback.rs [SIGNAL_INFERRED]
- custom-command: crates/file_finder/src/file_finder.rs [SIGNAL_INFERRED]
- custom-command: crates/git/src/git.rs [SIGNAL_INFERRED]
- custom-command: crates/git_graph/src/git_graph.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/branch_picker.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/commit_view.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/git_panel.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/git_picker.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/project_diff.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/stash_picker.rs [SIGNAL_INFERRED]
- custom-command: crates/git_ui/src/worktree_picker.rs [SIGNAL_INFERRED]
- custom-command: crates/gpui/src/action.rs [SIGNAL_INFERRED] (macro/derive definition site itself)
- custom-command: crates/image_viewer/src/image_viewer.rs [SIGNAL_INFERRED]
- custom-command: crates/input_latency_ui/src/input_latency_ui.rs [SIGNAL_INFERRED]
- custom-command: crates/install_cli/src/install_cli_binary.rs [SIGNAL_INFERRED]
- custom-command: crates/install_cli/src/register_zed_scheme.rs [SIGNAL_INFERRED]
- custom-command: crates/journal/src/journal.rs [SIGNAL_INFERRED]
- custom-command: crates/keymap_editor/src/keymap_editor.rs [SIGNAL_INFERRED]
- custom-command: crates/keymap_editor/src/ui_components/keystroke_input.rs [SIGNAL_INFERRED]
- custom-command: crates/language_selector/src/language_selector.rs [SIGNAL_INFERRED]
- custom-command: crates/language_tools/src/highlights_tree_view.rs [SIGNAL_INFERRED]
- custom-command: crates/language_tools/src/key_context_view.rs [SIGNAL_INFERRED]
- custom-command: crates/language_tools/src/lsp_button.rs [SIGNAL_INFERRED]
- custom-command: crates/language_tools/src/lsp_log_view.rs [SIGNAL_INFERRED]
- custom-command: crates/language_tools/src/syntax_tree_view.rs [SIGNAL_INFERRED]
- custom-command: crates/line_ending_selector/src/line_ending_selector.rs [SIGNAL_INFERRED]
- custom-command: crates/markdown/src/markdown.rs [SIGNAL_INFERRED]
- custom-command: crates/markdown_preview/src/markdown_preview.rs [SIGNAL_INFERRED]
- custom-command: crates/menu/src/menu.rs [SIGNAL_INFERRED]
- custom-command: crates/onboarding/src/base_keymap_picker.rs [SIGNAL_INFERRED]
- custom-command: crates/onboarding/src/onboarding.rs [SIGNAL_INFERRED]
- custom-command: crates/outline_panel/src/outline_panel.rs [SIGNAL_INFERRED]
- custom-command: crates/panel/src/panel.rs [SIGNAL_INFERRED]
- custom-command: crates/picker/src/picker.rs [SIGNAL_INFERRED]
- custom-command: crates/platform_title_bar/src/system_window_tabs.rs [SIGNAL_INFERRED]
- custom-command: crates/project/src/context_server_store.rs [SIGNAL_INFERRED]
- custom-command: crates/project_panel/src/project_panel.rs [SIGNAL_INFERRED]
- custom-command: crates/recent_projects/src/recent_projects.rs [SIGNAL_INFERRED]
- custom-command: crates/remote/src/remote_client.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/repl/src/repl_sessions_ui.rs [SIGNAL_INFERRED]
- custom-command: crates/search/src/project_search.rs [SIGNAL_INFERRED]
- custom-command: crates/search/src/search.rs [SIGNAL_INFERRED]
- custom-command: crates/settings_ui/src/settings_ui.rs [SIGNAL_INFERRED]
- custom-command: crates/sidebar/src/sidebar.rs [SIGNAL_INFERRED]
- custom-command: crates/snippets_ui/src/snippets_ui.rs [SIGNAL_INFERRED]
- custom-command: crates/svg_preview/src/svg_preview.rs [SIGNAL_INFERRED]
- custom-command: crates/system_specs/src/system_specs.rs [SIGNAL_INFERRED]
- custom-command: crates/tab_switcher/src/tab_switcher.rs [SIGNAL_INFERRED]
- custom-command: crates/terminal/src/terminal.rs [SIGNAL_INFERRED]
- custom-command: crates/terminal_view/src/terminal_panel.rs [SIGNAL_INFERRED]
- custom-command: crates/terminal_view/src/terminal_view.rs [SIGNAL_INFERRED]
- custom-command: crates/theme_selector/src/theme_selector.rs [SIGNAL_INFERRED]
- custom-command: crates/title_bar/src/application_menu.rs [SIGNAL_INFERRED]
- custom-command: crates/title_bar/src/title_bar.rs [SIGNAL_INFERRED]
- custom-command: crates/toolchain_selector/src/toolchain_selector.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/change_list.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/command.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/digraph.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/vim/src/helix.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/helix/paste.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/vim/src/indent.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/insert.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/motion.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/normal.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/normal/increment.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/vim/src/normal/paste.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/vim/src/normal/repeat.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/normal/scroll.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/normal/search.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/normal/substitute.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/object.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/replace.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/rewrap.rs [SIGNAL_INFERRED] (`#[derive(Action)]`)
- custom-command: crates/vim/src/vim.rs [SIGNAL_INFERRED]
- custom-command: crates/vim/src/visual.rs [SIGNAL_INFERRED]
- custom-command: crates/workspace/src/multi_workspace.rs [SIGNAL_INFERRED]
- custom-command: crates/workspace/src/pane.rs [SIGNAL_INFERRED]
- custom-command: crates/workspace/src/theme_preview.rs [SIGNAL_INFERRED]
- custom-command: crates/workspace/src/welcome.rs [SIGNAL_INFERRED]
- custom-command: crates/workspace/src/workspace.rs [SIGNAL_INFERRED]
- custom-command: crates/zed/src/zed.rs [SIGNAL_INFERRED]
- custom-command: crates/zed_actions/src/lib.rs [SIGNAL_INFERRED]

- event-listener: _(none found — GPUI's `cx.subscribe`/`cx.observe` entity-event pattern is
  the universal UI-reactivity primitive used across nearly all ~1356 .rs files; it does not
  isolate a distinct background-logic surface separate from ordinary view wiring, so no
  per-file entries are emitted for it. See queue-worker below for the subset that performs
  genuine background async work.)_

- integration: crates/askpass/src/encrypted_password.rs [SIGNAL_INFERRED]
  - Intent matched: integration — external process/credential-helper invocation
  - No-row reason: stack=Rust/GPUI, no row in bl-source-patterns.md table
  - Observed pattern: spawns external `Command::new` (git askpass / credential helper) processes
- integration: crates/dev_container/src/devcontainer_json.rs [SIGNAL_INFERRED]
- integration: crates/dev_container/src/devcontainer_manifest.rs [SIGNAL_INFERRED]
- integration: crates/dev_container/src/docker.rs [SIGNAL_INFERRED]
  - Observed pattern: shells out to `docker` CLI via `Command::new` for devcontainer lifecycle
- integration: crates/explorer_command_injector/src/explorer_command_injector.rs [SIGNAL_INFERRED]
- integration: crates/git/src/repository.rs [SIGNAL_INFERRED]
  - Observed pattern: invokes external `git` binary via process spawn + `.detach()` background task
- integration: crates/lsp/src/lsp.rs [SIGNAL_INFERRED]
  - Intent matched: integration — external language-server process management
  - Observed pattern: spawns/manages LSP server subprocess, `Task<>`-based request/response, file watch registration
- integration: crates/project/src/lsp_store.rs [SIGNAL_INFERRED]
  - Observed pattern: orchestrates LSP adapter lifecycle across worktrees, `cx.background_spawn` + `.detach()`
- integration: crates/project/src/lsp_store/json_language_server_ext.rs [SIGNAL_INFERRED]
- integration: crates/project/src/lsp_store/rust_analyzer_ext.rs [SIGNAL_INFERRED]
- integration: crates/project/src/lsp_store/vue_language_server_ext.rs [SIGNAL_INFERRED]
- integration: crates/project/src/prettier_store.rs [SIGNAL_INFERRED]
  - Observed pattern: manages external prettier formatter subprocess
- integration: crates/project/src/terminals.rs [SIGNAL_INFERRED]
  - Observed pattern: spawns external shell process via `Command::new` for integrated terminal
- integration: crates/remote_server/src/headless_project.rs [SIGNAL_INFERRED]
- integration: crates/repl/src/kernels/native_kernel.rs [SIGNAL_INFERRED]
  - Observed pattern: spawns external Jupyter kernel process
- integration: crates/system_specs/src/system_specs.rs [SIGNAL_INFERRED]
- integration: crates/util/src/command.rs [SIGNAL_INFERRED]
- integration: crates/util/src/command/darwin.rs [SIGNAL_INFERRED]
- integration: crates/util/src/process.rs [SIGNAL_INFERRED]
- integration: crates/util/src/shell_builder.rs [SIGNAL_INFERRED]
- integration: crates/util/src/shell_env.rs [SIGNAL_INFERRED]
- integration: crates/util/src/util.rs [SIGNAL_INFERRED]
- integration: crates/vim/src/command.rs [SIGNAL_INFERRED]
  - Observed pattern: `:!` shell-command execution (external process) from vim command mode

- mail: _(none found)_

- middleware: _(none found — desktop editor has no HTTP request/response middleware chain;
  closest analog would be gpui input event dispatch, which is core UI plumbing, not BL)_

- notification: _(none found as a distinct BL category — `crates/notifications/` exists but
  implements in-app collaboration notification *data model/UI*, not a background dispatch
  worker; see crates/notifications/src/*.rs in File Inventory, tagged `other`)_

- observer: crates/client/src/user.rs [SIGNAL_INFERRED]
  - Intent matched: observer — reacts to external state changes
  - No-row reason: stack=Rust/GPUI, no row in bl-source-patterns.md table
  - Observed pattern: watches/refreshes user account state
- observer: crates/context_server/src/protocol.rs [SIGNAL_INFERRED]
- observer: crates/editor/src/inlays/inlay_hints.rs [SIGNAL_INFERRED]
- observer: crates/feature_flags/src/feature_flags.rs [SIGNAL_INFERRED]
  - Observed pattern: watches feature-flag state changes and notifies subscribers
- observer: crates/fs/src/fs.rs [SIGNAL_INFERRED]
- observer: crates/fs/src/fs_watcher.rs [SIGNAL_INFERRED]
  - Observed pattern: OS filesystem watcher (`notify`-style) backing worktree change detection
- observer: crates/gpui_web/src/window.rs [SIGNAL_INFERRED]
- observer: crates/language_tools/src/lsp_log_view.rs [SIGNAL_INFERRED]
- observer: crates/project/src/debounced_delay.rs [SIGNAL_INFERRED]
  - Intent matched: scheduled-job (debounce loop) — filed under observer/scheduled dual-intent
  - Observed pattern: debounce timer coalescing rapid filesystem/git-status events before re-render
- observer: crates/project/src/debugger/session.rs [SIGNAL_INFERRED]
- observer: crates/prompt_store/src/prompts.rs [SIGNAL_INFERRED]
- observer: crates/recent_projects/src/remote_servers.rs [SIGNAL_INFERRED]
- observer: crates/settings/src/editorconfig_store.rs [SIGNAL_INFERRED]
- observer: crates/settings/src/settings_file.rs [SIGNAL_INFERRED]
- observer: crates/settings/src/settings_store.rs [SIGNAL_INFERRED]
  - Observed pattern: watches settings file(s) on disk, re-parses and republishes settings on change
- observer: crates/snippet_provider/src/lib.rs [SIGNAL_INFERRED]
- observer: crates/vim/src/state.rs [SIGNAL_INFERRED]
- observer: crates/worktree/src/worktree.rs [SIGNAL_INFERRED]
  - Observed pattern: core fs-event watcher driving worktree entry/git-status refresh (background_spawn + detach loop)
- observer: crates/zed/src/main.rs [SIGNAL_INFERRED]
- observer: crates/zed/src/zed.rs [SIGNAL_INFERRED]

- scheduled-job: crates/project/src/debounced_delay.rs [SIGNAL_INFERRED]
  - Intent matched: scheduled-job — debounce/poll timer (dup-listed under observer; both
    intents apply per bl-source-patterns.md "Stack may appear as both" allowance)
  - No-row reason: stack=Rust/GPUI, no row in bl-source-patterns.md table
  - Observed pattern: `DebouncedDelay` type wraps `cx.background_executor().timer()` to coalesce events
- scheduled-job: crates/project/src/project.rs [SIGNAL_INFERRED]
  - Observed pattern: uses `DebouncedDelay` for periodic re-scan/refresh scheduling

- queue-worker: crates/command_palette/src/command_palette.rs [SIGNAL_INFERRED]
  - Intent matched: queue-worker — async background job via `cx.background_spawn` + `.detach()`
  - No-row reason: stack=Rust/GPUI, no row in bl-source-patterns.md table
  - Observed pattern: fire-and-forget background task per GPUI Task/detach convention in CLAUDE.md
- queue-worker: crates/component_preview/src/component_preview.rs [SIGNAL_INFERRED]
- queue-worker: crates/context_server/src/listener.rs [SIGNAL_INFERRED]
- queue-worker: crates/db/src/db.rs [SIGNAL_INFERRED]
  - Observed pattern: background sqlite migration/write task
- queue-worker: crates/debugger_ui/src/attach_modal.rs [SIGNAL_INFERRED]
- queue-worker: crates/debugger_ui/src/session/running.rs [SIGNAL_INFERRED]
- queue-worker: crates/debugger_ui/src/session/running/stack_frame_list.rs [SIGNAL_INFERRED]
- queue-worker: crates/editor/src/code_context_menus.rs [SIGNAL_INFERRED]
- queue-worker: crates/editor/src/editor.rs [SIGNAL_INFERRED]
- queue-worker: crates/editor/src/items.rs [SIGNAL_INFERRED]
- queue-worker: crates/editor/src/runnables.rs [SIGNAL_INFERRED]
- queue-worker: crates/extension_host/src/extension_host.rs [SIGNAL_INFERRED]
  - Observed pattern: background extension install/compile/load task queue
- queue-worker: crates/extensions_ui/src/extension_suggest.rs [SIGNAL_INFERRED]
- queue-worker: crates/file_finder/src/file_finder.rs [SIGNAL_INFERRED]
- queue-worker: crates/git/src/repository.rs [SIGNAL_INFERRED]
- queue-worker: crates/git_graph/src/git_graph.rs [SIGNAL_INFERRED]
- queue-worker: crates/git_ui/src/branch_picker.rs [SIGNAL_INFERRED]
- queue-worker: crates/git_ui/src/git_panel.rs [SIGNAL_INFERRED]
  - Observed pattern: background git-status refresh task (fire-and-forget, mirrors "git status polling")
- queue-worker: crates/git_ui/src/project_diff.rs [SIGNAL_INFERRED]
- queue-worker: crates/gpui/src/executor.rs [SIGNAL_INFERRED]
  - Intent matched: queue-worker — this is the GPUI executor primitive itself (Task/background_spawn/detach definition site)
- queue-worker: crates/image_viewer/src/image_viewer.rs [SIGNAL_INFERRED]
- queue-worker: crates/install_cli/src/install_cli_binary.rs [SIGNAL_INFERRED]
- queue-worker: crates/journal/src/journal.rs [SIGNAL_INFERRED]
- queue-worker: crates/keymap_editor/src/keymap_editor.rs [SIGNAL_INFERRED]
- queue-worker: crates/lsp/src/lsp.rs [SIGNAL_INFERRED]
- queue-worker: crates/markdown/src/markdown.rs [SIGNAL_INFERRED]
- queue-worker: crates/markdown_preview/src/markdown_preview_view.rs [SIGNAL_INFERRED]
- queue-worker: crates/miniprofiler_ui/src/miniprofiler_ui.rs [SIGNAL_INFERRED]
- queue-worker: crates/onboarding/src/onboarding.rs [SIGNAL_INFERRED]
- queue-worker: crates/outline_panel/src/outline_panel.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/buffer_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/debugger/breakpoint_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/debugger/dap_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/debugger/session.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/git_store.rs [SIGNAL_INFERRED]
  - Observed pattern: background git-status/index polling worker feeding project-wide git state
- queue-worker: crates/project/src/image_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/lsp_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/lsp_store/vue_language_server_ext.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/prettier_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/project.rs [SIGNAL_INFERRED]
- queue-worker: crates/project/src/worktree_store.rs [SIGNAL_INFERRED]
- queue-worker: crates/recent_projects/src/dev_container_suggest.rs [SIGNAL_INFERRED]
- queue-worker: crates/remote_server/src/headless_project.rs [SIGNAL_INFERRED]
- queue-worker: crates/remote_server/src/server.rs [SIGNAL_INFERRED]
- queue-worker: crates/repl/src/kernels/mod.rs [SIGNAL_INFERRED]
- queue-worker: crates/repl/src/notebook/notebook_ui.rs [SIGNAL_INFERRED]
- queue-worker: crates/repl/src/repl_editor.rs [SIGNAL_INFERRED]
- queue-worker: crates/settings_ui/src/settings_ui.rs [SIGNAL_INFERRED]
- queue-worker: crates/tasks_ui/src/tasks_ui.rs [SIGNAL_INFERRED]
- queue-worker: crates/terminal/src/terminal.rs [SIGNAL_INFERRED]
- queue-worker: crates/terminal_view/src/terminal_panel.rs [SIGNAL_INFERRED]
- queue-worker: crates/terminal_view/src/terminal_view.rs [SIGNAL_INFERRED]
- queue-worker: crates/vim/src/command.rs [SIGNAL_INFERRED]
- queue-worker: crates/vim/src/state.rs [SIGNAL_INFERRED]
- queue-worker: crates/workspace/src/item.rs [SIGNAL_INFERRED]
- queue-worker: crates/workspace/src/multi_workspace.rs [SIGNAL_INFERRED]
- queue-worker: crates/workspace/src/tasks.rs [SIGNAL_INFERRED]
- queue-worker: crates/workspace/src/welcome.rs [SIGNAL_INFERRED]
- queue-worker: crates/workspace/src/workspace.rs [SIGNAL_INFERRED]
- queue-worker: crates/worktree/src/worktree.rs [SIGNAL_INFERRED]
- queue-worker: crates/zed/src/main.rs [SIGNAL_INFERRED]
- queue-worker: crates/zed/src/zed/migrate.rs [SIGNAL_INFERRED]

- webhook: _(none found — no inbound/outbound HTTP webhook endpoints; this is a desktop
  editor, not a server. Note: `crates/rpc/` and `crates/client/` implement outbound
  collaboration-server RPC/websocket connections, which is the closest analog but does
  not match webhook intent — tagged `background`/`other` in File Inventory instead.)_

