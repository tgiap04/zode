<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths — all references here are output targets or internal definitions -->
# Behavior Logic

**Project**: zode (Zed editor fork)
**Generated**: 2026-08-07
**Analysis Scope**: Full codebase — 179 crates under `crates/`, 4 extension crates, 3 tooling crates. Stack: Rust/GPUI native desktop application (no HTTP routes/web surface).

**Code Format**: All codes MUST follow `BL###_NameSlug` format (e.g., BL001_ScheduledReport, BL002_EventListener)

**Behavior Logic Types** (canonical 10 — language-neutral):
- `scheduled-job` — Cron-like scheduled tasks
- `queue-worker` — Background job workers (async queue consumers)
- `event-listener` — Event-driven handlers
- `observer` — Model lifecycle hooks (created/updated/deleted)
- `mail` — Email sending logic
- `notification` — In-app / push notification logic
- `middleware` — Request/response processing chain (non-auth)
- `custom-command` — CLI commands
- `integration` — Third-party integrations (external API clients)
- `webhook` — Incoming/outgoing webhook handlers

**Note**: Auth/permission middleware is NOT included — see Permissions.md

**Note**: Feature and UserStory mapping is managed in FeatureList.md and UserStories.md. This document contains behavior logic items without direct feature/story references.

**Note**: This is a native GPUI desktop application, not a web/server app. There is no HTTP surface, so `mail`, `notification` (as a distinct dispatch worker), `middleware`, and `webhook` are empty categories here — see Scope Notes in the Summary section. `event-listener` is also empty: GPUI's `cx.subscribe`/`cx.observe` entity-event pattern is the universal UI-reactivity primitive used across nearly all source files and does not isolate a distinct background-logic surface separate from ordinary view wiring (see scout-report.md's Background Logic Source Inventory note); genuine background async work is captured instead under `observer` (external state watchers) and `queue-worker` (fire-and-forget background tasks).

---

## Cardinality Contract

Rules enforced by Wave 2b researcher and Wave 7a reviewer. Violations are critical.

- **Rule C1 — 1 BL per inventory entry**: Mode A stacks (folder convention): 1 file = 1 BL. Aggregation is a critical violation.
- **Rule C2 — Source fields mandatory, single-valued**: Every BL item MUST include `**Source File**` (one relative path) and `**Source Symbol**` (one symbol). Both fields must match the scout inventory entry 1-to-1.
- **Rule C3 — Unmatched BL warning**: A BL item whose Source File does not appear in the scout inventory → warning; researcher must provide justification in Description.

---

## Behavior Logic Index

| Code | Name | Type | Trigger |
|------|------|------|---------|
| BL001 | ActivityIndicatorStatusActions | custom-command | Keybinding (keymap.json) / command palette, invoked from the status-bar activity indicator |
| BL002 | CsvPreviewOpenActions | custom-command | Command palette / editor tab context menu on `.csv` files |
| BL003 | InitializeDevContainerAction | custom-command | Command palette |
| BL004 | OpenDebugAdapterLogsAction | custom-command | Command palette (dev tools) |
| BL005 | DebuggerSessionControlActions | custom-command | Keybinding (keymap.json) / command palette / debugger toolbar buttons |
| BL006 | NewProcessModalTabActions | custom-command | Keybinding (keymap.json) inside the New Process modal |
| BL007 | BreakpointListNavigationActions | custom-command | Keybinding while breakpoint list panel is focused |
| BL008 | ConsoleWatchExpressionAction | custom-command | Keybinding / context menu while debugger console is focused |
| BL009 | MemoryViewGoToAddressAction | custom-command | Keybinding / query-bar submit inside the memory view |
| BL010 | VariableListActions | custom-command | Keybinding / context menu while variable list is focused |
| BL011 | DeployCurrentFileDiagnosticsAction | custom-command | Command palette / keybinding |
| BL012 | ProjectDiagnosticsActions | custom-command | Command palette / keybinding |
| BL013 | EditorCoreActions | custom-command | Keybinding (keymap.json) — the editor's primary command surface |
| BL014 | ToggleSplitDiffAction | custom-command | Keybinding / toolbar toggle in a split editor |
| BL015 | EncodingSelectorToggleAction | custom-command | Command palette / status-bar encoding indicator click |
| BL016 | EtwTracingActions | custom-command | Command palette (Windows-only diagnostic tooling) |
| BL017 | ReloadExtensionsAction | custom-command | Command palette |
| BL018 | InstallDevExtensionAction | custom-command | Command palette / "Install Dev Extension" button on the Extensions page |
| BL019 | FeedbackActions | custom-command | Command palette / Help menu |
| BL020 | FileFinderActions | custom-command | Keybinding while the file finder modal is open |
| BL021 | GitHunkStagingActions | custom-command | Keybinding / git panel context menu on a hunk or status entry |
| BL022 | GitGraphActions | custom-command | Keybinding / command palette from the Git Graph tab |
| BL023 | BranchPickerActions | custom-command | Keybinding while the branch picker is open |
| BL024 | CommitViewStashActions | custom-command | Command palette / keybinding |
| BL025 | GitPanelActions | custom-command | Keybinding / git panel toolbar and context menu |
| BL026 | GitPickerTabActions | custom-command | Keybinding while the combined git picker is open |
| BL027 | ProjectDiffActions | custom-command | Keybinding / command palette |
| BL028 | StashPickerActions | custom-command | Keybinding while the stash picker is open |
| BL029 | DeleteWorktreeAction | custom-command | Keybinding while the worktree picker is open |
| BL030 | ActionsMacroDefinition | custom-command | N/A — compile-time declarative macro, not an invokable command itself |
| BL031 | ImageViewerZoomActions | custom-command | Keybinding / toolbar buttons while viewing an image |
| BL032 | DumpInputLatencyHistogramAction | custom-command | Command palette (dev tools) |
| BL033 | InstallCliBinaryAction | custom-command | Command palette |
| BL034 | RegisterZedSchemeAction | custom-command | Command palette |
| BL035 | NewJournalEntryAction | custom-command | Command palette / keybinding |
| BL036 | KeymapEditorActions | custom-command | Keybinding / context menu inside the keymap editor |
| BL037 | KeystrokeRecordingActions | custom-command | Keybinding / click on the keystroke-capture input widget |
| BL038 | LanguageSelectorToggleAction | custom-command | Command palette / status-bar language indicator click |
| BL039 | HighlightsTreeViewActions | custom-command | Command palette (dev tools) |
| BL040 | OpenKeyContextViewAction | custom-command | Command palette (dev tools) |
| BL041 | LspToolMenuToggleAction | custom-command | Click on the LSP status-bar button / keybinding |
| BL042 | OpenLanguageServerLogsAction | custom-command | Command palette (dev tools) |
| BL043 | SyntaxTreeViewActions | custom-command | Command palette (dev tools) |
| BL044 | LineEndingSelectorToggleAction | custom-command | Command palette / status-bar line-ending indicator click |
| BL045 | MarkdownCopyActions | custom-command | Keybinding / context menu on selected markdown text |
| BL046 | MarkdownPreviewScrollActions | custom-command | Keybinding while a markdown preview pane is focused |
| BL047 | MenuNavigationActions | custom-command | Keybinding while any menu/list/picker is focused (shared cross-cutting namespace) |
| BL048 | ToggleBaseKeymapSelectorAction | custom-command | Command palette / onboarding flow |
| BL049 | OnboardingFlowActions | custom-command | Buttons within the first-run onboarding screen |
| BL050 | OutlinePanelActions | custom-command | Keybinding / context menu while the outline panel is focused |
| BL051 | PanelTabNavigationActions | custom-command | Keybinding while a tabbed dock panel is focused |
| BL052 | PickerConfirmActions | custom-command | Keybinding while a picker's input editor is focused |
| BL053 | WindowTabManagementActions | custom-command | Keybinding / Window menu (native window tabs) |
| BL054 | ContextServerRestartAction | custom-command | Command palette / context server status UI |
| BL055 | ProjectPanelActions | custom-command | Keybinding / context menu while the project panel is focused |
| BL056 | RecentProjectsMenuActions | custom-command | Keybinding while the recent-projects picker is open |
| BL057 | OpenWslPathAction | custom-command | Programmatic (WSL remote connection flow), not user-keybound |
| BL058 | ReplSessionActions | custom-command | Keybinding / command palette in a REPL-connected editor |
| BL059 | ProjectSearchActions | custom-command | Keybinding / command palette while project search is open |
| BL060 | BufferSearchActions | custom-command | Keybinding / command palette while the in-buffer search bar is open |
| BL061 | SettingsEditorActions | custom-command | Keybinding while the Settings UI is focused |
| BL062 | FocusSidebarFilterAction | custom-command | Keybinding |
| BL063 | SnippetsConfigActions | custom-command | Command palette |
| BL064 | SvgOpenFollowingPreviewAction | custom-command | Command palette / editor tab context menu on `.svg` files |
| BL065 | CopySystemSpecsAction | custom-command | Command palette / Help menu |
| BL066 | TabSwitcherActions | custom-command | Keybinding (Ctrl/Cmd+Tab-style modal) |
| BL067 | TerminalCoreActions | custom-command | Keybinding while a terminal pane is focused |
| BL068 | TerminalPanelToggleActions | custom-command | Keybinding / command palette |
| BL069 | TerminalViewTextActions | custom-command | Programmatic / task-runner integration (SendText, SendKeystroke) and keybinding (RerunTask, RenameTe |
| BL070 | ThemeSelectorReloadAction | custom-command | Command palette (dev/theme-authoring workflow) |
| BL071 | ApplicationMenuActivationActions | custom-command | Keybinding within the client-side (non-native) application menu bar |
| BL072 | TitleBarProjectMenuActions | custom-command | Click on the title bar's project/branch dropdown |
| BL073 | ToolchainSelectorActions | custom-command | Command palette / status-bar toolchain indicator click |
| BL074 | VimChangeListNavigationActions | custom-command | Keybinding in Vim normal mode (`g;` / `g,`) |
| BL075 | VimExCommandActions | custom-command | Vim `:`-command line (ex commands) |
| BL076 | VimDigraphLiteralAction | custom-command | Vim insert-mode digraph sequence (`Ctrl-K` + two chars) |
| BL077 | VimHelixModeActions | custom-command | Keybinding when Vim's Helix emulation mode is active |
| BL078 | VimHelixPasteAction | custom-command | Keybinding in Helix mode |
| BL079 | VimIndentActions | custom-command | Keybinding in Vim normal/visual mode (`>`, `<`, `=`) |
| BL080 | VimInsertModeActions | custom-command | Keybinding while in Vim insert mode |
| BL081 | VimMotionActions | custom-command | Keybinding — Vim motion commands (`w`, `e`, `b`, `k`/`j`, etc.) |
| BL082 | VimNormalModeInsertActions | custom-command | Keybinding in Vim normal mode (`i`, `a`, `I`, `A`, `o`, `O`) |
| BL083 | VimIncrementDecrementActions | custom-command | Keybinding in Vim normal mode (`Ctrl-A` / `Ctrl-X`) |
| BL084 | VimPasteAction | custom-command | Keybinding in Vim normal mode (`p` / `P`) |
| BL085 | VimRepeatMacroActions | custom-command | Keybinding in Vim normal mode (`.`, `q`, `@@`) |
| BL086 | VimScrollActions | custom-command | Keybinding in Vim normal mode (`Ctrl-E`/`Ctrl-Y`, `zz`, etc.) |
| BL087 | VimSearchActions | custom-command | Keybinding in Vim normal mode (`/`, `?`, `*`, `#`) |
| BL088 | VimSubstituteActions | custom-command | Keybinding in Vim normal mode (`s`, `S`) |
| BL089 | VimTextObjectActions | custom-command | Keybinding as the object half of a Vim operator+object combo (`diw`, `ci(`, etc.) |
| BL090 | VimReplaceModeActions | custom-command | Keybinding in Vim normal mode (`R`) |
| BL091 | VimRewrapAction | custom-command | Keybinding in Vim normal mode (`gq`) |
| BL092 | VimOperatorPendingPushActions | custom-command | Internal — pushed onto Vim's operator-pending stack while a multi-key command is being composed |
| BL093 | VimVisualModeActions | custom-command | Keybinding in Vim visual mode (`v`, `V`, `Ctrl-V`, `d`, `x`, etc.) |
| BL094 | MultiWorkspaceSidebarActions | custom-command | Keybinding / click on the multi-project sidebar |
| BL095 | PaneItemManagementActions | custom-command | Keybinding / tab bar interaction on a pane |
| BL096 | OpenThemePreviewAction | custom-command | Command palette (dev tools) |
| BL097 | WelcomeScreenActions | custom-command | Command palette / click on a recent-project entry on the welcome screen |
| BL098 | WorkspacePaneNavigationActions | custom-command | Keybinding / command palette — core workspace pane/window navigation |
| BL099 | ZedApplicationWindowActions | custom-command | Keybinding / command palette / native application menu |
| BL100 | ZedActionsSharedRegistry | custom-command | Keybinding / command palette — cross-crate shared action namespace re-exported to avoid circular cra |
| BL101 | EncryptedPasswordAtRest | integration | an SSH/git askpass prompt (or similar credential flow) needs to hold a user-entered password in memo |
| BL102 | DevContainerJsonParsing | integration | opening a folder with a `.devcontainer/devcontainer.json` (or a specific dev-container config) trigg |
| BL103 | DevContainerManifestBuildAndRun | integration | user opens/attaches a project configured for a dev container — build image/compose stack, then run/e |
| BL104 | DockerCliIntegration | integration | any dev-container operation needing to talk to the Docker (or Podman) engine — inspect, pull, compos |
| BL105 | WindowsExplorerContextMenuInjector | integration | user right-clicks a file/folder in Windows Explorer and invokes the injected "Open with Zed" shell c |
| BL106 | GitCliRepositoryIntegration | integration | any project operation that needs live git state or mutation — status, diff, commit, branch, stash, b |
| BL107 | LanguageServerProcessLifecycle | integration | a worktree needs a language server for a detected language (buffer opened, language changed) — spawn |
| BL108 | LspStoreStartLanguageServer | integration | `LspStore` decides a worktree/language pair needs a running language server (buffer opened, settings |
| BL109 | JsonLanguageServerSchemaContentExtension | integration | the JSON language server (vscode-json-language-server) sends a `vscode/content` request for a `zed:/ |
| BL110 | RustAnalyzerServerStatusExtension | integration | a running rust-analyzer language server emits an `experimental/serverStatus` notification (health Ok |
| BL111 | VueLanguageServerTsServerBridge | integration | the Vue language server sends a `tsserver/request` notification that must be proxied to the co-insta |
| BL112 | PrettierProcessLifecycle | integration | a buffer needs formatting via Prettier — either a worktree-local Prettier install is detected, or th |
| BL113 | ProjectTerminalShellSpawn | integration | user opens a new terminal panel/dock (or a remote "local terminal" breakout) for a project |
| BL114 | ProjectTaskTerminalSpawn | integration | a configured task (from `tasks.json`/task runner) is executed, spawning its command inside a new or  |
| BL115 | HeadlessProjectRemoteJupyterKernelSpawn | integration | a remote (SSH headless) project session receives a request to start a Jupyter/IPython kernel for the |
| BL116 | NativeJupyterKernelProcessLifecycle | integration | user starts a REPL/notebook session locally against a discovered Jupyter kernelspec |
| BL117 | SystemSpecsGpuProbe | integration | user invokes "Copy System Specs Into Clipboard" (or any code path constructing `SystemSpecs`) on Lin |
| BL118 | CrossPlatformCommandWrapper | integration | any subsystem needs to spawn an external process portably across macOS/Linux/Windows (git, LSP serve |
| BL119 | MacosPosixSpawnCommand | integration | any subprocess spawn on macOS (git, LSP servers, tasks, shells) that goes through `util::command::Co |
| BL120 | ChildProcessGroupLifecycle | integration | any spawn path that needs guaranteed cleanup of a subprocess and all its descendants when the parent |
| BL121 | ShellCommandLineBuilder | integration | any integration that needs to construct a shell-invocation command line (task runner, terminal spawn |
| BL122 | LoginShellEnvironmentCapture | integration | Zed starts up (or a terminal/task needs a fresh environment) and needs to capture the user's real lo |
| BL123 | VimShellExecCommand | integration | user runs a vim `:!command`, `:read !command`, or a range/motion/text-object filter (`!!`, `!ip`, `. |
| BL124 | UserStore Connection Status Observer | observer | `Client` connection status stream emits `Status::SignedOut` or `Status::ConnectionLost` |
| BL125 | Context Server Notification Dispatch | observer | Incoming MCP JSON-RPC notification received on the underlying `Client` connection |
| BL126 | Editor Inlay Hints Refresh on Buffer/Settings/LSP Changes | observer | `InlayHintRefreshReason` variant fires — buffer edited, new lines scrolled into view, inlay hint set |
| BL127 | Feature Flag Store Change Observer | observer | `FeatureFlagStore` global is mutated (server flags updated, staff status changed) |
| BL128 | Filesystem Watch Event Stream | observer | OS-level filesystem change under a watched path (create/modify/remove/rescan) |
| BL129 | Global Filesystem Watcher Event Dispatch | observer | `notify` crate delivers a raw OS filesystem event to the process-wide watcher |
| BL130 | Web Window Appearance/DPR Media Query Observer | observer | Browser `matchMedia` "change" event for color-scheme (dark/light) or device-pixel-ratio, plus `Resiz |
| BL131 | LSP Log View Server Entry Observer | observer | `LogStore` emits `Event::NewServerLogEntry` or changes its set of tracked language servers |
| BL132 | Debounced Delay Trigger | observer | `DebouncedDelay::fire_new` called again before the previous delay elapsed (supersedes it) or the del |
| BL133 | Debug Session Breakpoint Store Observer | observer | `BreakpointStore` emits `BreakpointStoreEvent::BreakpointsUpdated` or `BreakpointStoreEvent::Breakpo |
| BL134 | Prompt Template Override Directory Watcher | observer | Filesystem changes to the prompt-template-overrides directory (or its parent, for create/delete of t |
| BL135 | Remote Server Projects Settings Observer | observer | `SettingsStore` global changes, specifically the `RemoteSettings::read_ssh_config` value |
| BL136 | External Editorconfig File Watcher | observer | Filesystem change to an `.editorconfig` file located outside a worktree (an ancestor directory of th |
| BL137 | Config File Content Watcher | observer | Filesystem change event on a single watched config file (settings.json, global settings, keymap.json |
| BL138 | Settings Store User/Global File Watcher | observer | User settings file or global settings file changes on disk |
| BL139 | Snippet Directory Watcher | observer | Filesystem change under a watched snippets directory (global or per-project) |
| BL140 | Vim Marks Buffer Lifecycle Observer | observer | `BufferStore` emits `BufferStoreEvent::BufferAdded`, or a watched `Buffer` emits `BufferEvent::Edite |
| BL141 | Worktree Background Scanner Filesystem Observer | observer | Filesystem change under the worktree root reported by `Fs::watch`, or an explicit rescan/path-prefix |
| BL142 | Dev-Mode Grammar Source Watcher | observer | Filesystem change (`.scm` query file) under `crates/grammars/src` in a debug build |
| BL143 | Keymap File and Settings-Driven Keymap Observer | observer | Keymap file changes on disk, or the `SettingsStore` global changes in a way that affects the base ke |
| BL144 | Debounced Delay Timer | scheduled-job | `DebouncedDelay::fire_new` invoked while a prior timer for the same debouncer is still pending — can |
| BL145 | Git Diff Debounced Recalculation | scheduled-job | `Project::request_buffer_diff_recalculation` is called when a buffer is registered (`register_buffer |
| BL146 | Log Command Invocation | queue-worker | user confirms a command in the command palette (Enter) |
| BL147 | Save Component Preview Active Page | queue-worker | workspace item serialization pass for an open Component Preview tab |
| BL148 | Mcp Serve Connection | queue-worker | an MCP client connects to the local Unix socket created for the context-server listener |
| BL149 | Db Write And Log | queue-worker | any call site that needs a fire-and-forget SQLite write |
| BL150 | Fetch Remote Process List For Attach | queue-worker | user opens the debugger "Attach to Process" modal for a remote (SSH) project |
| BL151 | Forward Spawned Terminal Pid | queue-worker | a debug session spawns an integrated terminal for a debuggee process |
| BL152 | Persist Stack Frame Filter Preference | queue-worker | user toggles "only user frames" filter in the debugger stack-frame list |
| BL153 | Compute Completion Menu Filter Matches | queue-worker | editor completion menu results need re-filtering against a query |
| BL154 | Serialize Editor Selections | queue-worker | editor selections change and the workspace item is eligible for restore-on-startup persistence |
| BL155 | Serialize Editor Folds | queue-worker | editor fold state changes for a file-backed buffer |
| BL156 | Compute Runnable Ranges | queue-worker | editor needs to recompute runnable (test/run) code-lens ranges for the visible multi-buffer range |
| BL157 | Compile Dev Extension | queue-worker | user triggers rebuild of a locally-installed dev extension |
| BL158 | Dismiss Language Extension Suggestion | queue-worker | user clicks "No, don't install it" on the language-extension suggestion banner |
| BL159 | Check Recent History Path Exists | queue-worker | file finder populates candidates from recent-navigation history for a local project |
| BL160 | Read Global Git Committer Identity | queue-worker | git operations need the configured global committer name/email |
| BL161 | Save Git Graph Serialization | queue-worker | workspace item serialization pass for an open Git Graph tab |
| BL162 | Create Git Remote | queue-worker | user confirms adding a new remote in the branch picker's create-remote flow |
| BL163 | Commit Staged Changes | queue-worker | user triggers commit in the git panel with staged changes present |
| BL164 | Save Project Diff Base | queue-worker | workspace item serialization pass for an open Project Diff tab |
| BL165 | Detach And Log Err Task | queue-worker | any call site holding a fallible `Task<Result<T, E>>` that should run to completion without blocking |
| BL166 | Save Image Viewer Path | queue-worker | workspace item serialization pass for an open Image Viewer tab |
| BL167 | Prompt Cli Install On Linux | queue-worker | user runs "Install CLI" on Linux/FreeBSD where symlinking must be done manually |
| BL168 | Create Journal Entry File | queue-worker | user invokes "New Journal Entry" |
| BL169 | Debounce Keymap Action Search | queue-worker | user types in the keymap editor's action/keystroke search query fields |
| BL170 | Send Lsp Initialize Request | queue-worker | a language server process is spawned and needs the LSP handshake performed |
| BL171 | Parse Markdown Source | queue-worker | markdown source content changes and needs re-parsing |
| BL172 | Search Markdown Preview Source | queue-worker | user runs an in-buffer search (Cmd-F) inside a markdown preview view |
| BL173 | Export Miniprofiler Report | queue-worker | user chooses a save path for exporting a performance profile from the miniprofiler UI |
| BL174 | Save Onboarding Active Page | queue-worker | workspace item serialization pass for an open Onboarding tab |
| BL175 | Serialize Outline Panel State | queue-worker | outline panel's active/visible state changes and needs persisting |
| BL176 | Load Buffer Contents Off Thread | queue-worker | a buffer is requested that is not yet resident in memory |
| BL177 | Forward Breakpoint Toggle To Remote | queue-worker | user toggles a breakpoint while the project is a remote (collab) session |
| BL178 | Prune Stale Js Debug Companion Versions | queue-worker | DapStore is constructed (local mode) |
| BL179 | Install Latest Js Debug Companion | queue-worker | a newer js-debug-companion npm package version is detected than what is installed |
| BL180 | Restore Git Checkpoints | queue-worker | an agent/edit-prediction flow requests restoring the working tree to a previously captured git check |
| BL181 | Wait For Image Load | queue-worker | an image path is requested that is not yet loaded into the image store |
| BL182 | Restart Language Servers For Buffers | queue-worker | user or system requests restarting language servers for a set of buffers (remote project path) |
| BL183 | Forward Vue Ts Request To Typescript Server | queue-worker | the Vue language server issues an embedded TypeScript request that must be proxied to the real tsser |
| BL184 | Clear Prettier Caches On Settings Change | queue-worker | prettier-related project settings change, requiring active prettier instances to reload |
| BL185 | Forward Buffer Update To Remote Server | queue-worker | a remote-hosted buffer receives a local edit operation while acting as a remote server client |
| BL186 | Copy Worktree Entry Off Thread | queue-worker | user copies a file/directory entry within a local worktree |
| BL187 | Dismiss Dev Container Suggestion | queue-worker | user clicks "Don't Show Again" on the dev-container open suggestion banner |
| BL188 | Forward Buffer Update On Headless Project | queue-worker | a locally-originated buffer operation occurs inside a headless (remote_server) project |
| BL189 | Read Remote Server Stdin Loop | queue-worker | the remote server process starts and needs to read incoming protocol frames from stdin |
| BL190 | Route Jupyter Messages To Channels | queue-worker | a Jupyter kernel session starts and needs its shell/control/stdin channels wired |
| BL191 | Refresh Python Kernelspecs | queue-worker | a notebook editor needs to refresh available Python kernelspecs for its worktree |
| BL192 | Install Ipykernel Package | queue-worker | REPL execution detects the target Python environment is missing ipykernel |
| BL193 | Compute Settings Exact Search Matches | queue-worker | user types a query in the Settings window's search box |
| BL194 | Build Task Contexts Off Thread | queue-worker | the tasks UI (spawn/run) needs the current task execution context (cwd, selection, LSP task sources) |
| BL195 | Search Terminal Scrollback | queue-worker | user runs an in-terminal search (Cmd-F) inside an integrated terminal |
| BL196 | Serialize Terminal Panel Layout | queue-worker | the terminal panel's pane layout changes and needs persisting (debounced) |
| BL197 | Save Terminal Working Directory And Title | queue-worker | workspace item serialization pass for an open terminal tab whose cwd/title changed |
| BL198 | Pipe Vim Filter Command Stdin | queue-worker | a Vim `:!` filter command is run against a buffer range with piped stdin input |
| BL199 | Delete Vim Mark From Db | queue-worker | user deletes a Vim mark (buffer-local or global) |
| BL200 | Clone Item On Split Off Thread | queue-worker | user splits a pane, requiring the active item to be cloned into the new pane |
| BL201 | Bind Session To Window On Flush | queue-worker | multi-workspace flushes serialization for all open workspaces (e.g. on window close/quit) |
| BL202 | Await Spawned Task Process Output | queue-worker | a task template is resolved and spawned in an integrated terminal, and the workspace needs to know i |
| BL203 | Update Workspace Activation Timestamp | queue-worker | a workspace window becomes active |
| BL204 | Save Workspace Welcome Page State | queue-worker | workspace item serialization pass for an open Welcome Page tab |
| BL205 | Create Worktree Entry On Disk | queue-worker | user creates a new file or directory entry in the project panel/worktree |
| BL206 | Watch Theme File Changes | queue-worker | Zed starts in an environment where the active theme file may change on disk |
| BL207 | Run Keymap Or Settings Migration | queue-worker | user clicks "Backup and Update" in the settings/keymap migration dialog |
| BL208 | Zed Cli Path Resolution | integration | something needs to invoke the zed/zed-cli executable from a shell context |

---

## BL001_ActivityIndicatorStatusActions: ActivityIndicatorStatusActions

**Type**: custom-command
**Trigger**: Keybinding (keymap.json) / command palette, invoked from the status-bar activity indicator
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/activity_indicator/src/activity_indicator.rs
**Source Symbol**: ShowErrorMessage

### Description

`actions!(activity_indicator, [ShowErrorMessage, DismissMessage])` registers two commands for the status-bar language-server activity indicator: `ShowErrorMessage` surfaces the last LSP error message from the status bar, `DismissMessage` clears it.

### Related Modules

- activity_indicator

### Related Data Models

- LanguageServerName

---

## BL002_CsvPreviewOpenActions: CsvPreviewOpenActions

**Type**: custom-command
**Trigger**: Command palette / editor tab context menu on `.csv` files
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/csv_preview/src/csv_preview.rs
**Source Symbol**: OpenPreview

### Description

`actions!(csv, [OpenPreview, OpenPreviewToTheSide])` opens a tabular-data preview of the active CSV buffer, either replacing the active pane item or splitting to the side. Gated behind the `tabular-data-preview` feature flag.

### Related Modules

- csv_preview

### Related Data Models

- Editor

---

## BL003_InitializeDevContainerAction: InitializeDevContainerAction

**Type**: custom-command
**Trigger**: Command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/dev_container/src/lib.rs
**Source Symbol**: InitializeDevContainer

### Description

`#[derive(Action)] struct InitializeDevContainer` (namespace `projects`) opens the Dev Container modal on the active or a new workspace, letting the user initialize a `.devcontainer` configuration.

### Related Modules

- dev_container

### Related Data Models

- Workspace

---

## BL004_OpenDebugAdapterLogsAction: OpenDebugAdapterLogsAction

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_tools/src/dap_log.rs
**Source Symbol**: OpenDebugAdapterLogs

### Description

`actions!(dev, [OpenDebugAdapterLogs])` opens the Debug Adapter Protocol log viewer, backed by a `LogStore` observing all active debug sessions.

### Related Modules

- debugger_tools

---

## BL005_DebuggerSessionControlActions: DebuggerSessionControlActions

**Type**: custom-command
**Trigger**: Keybinding (keymap.json) / command palette / debugger toolbar buttons
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_ui/src/debugger_ui.rs
**Source Symbol**: Start

### Description

File registers the full debugger session control surface: `actions!(debugger, [Start, Continue, Detach, Pause, Restart, RerunSession, StepInto, StepOver, StepOut, StepBack, Stop, ToggleIgnoreBreakpoints, ClearAllBreakpoints, FocusConsole, FocusVariables, FocusBreakpointList, FocusFrames, FocusModules, FocusLoadedSources, FocusTerminal, ToggleThreadPicker, ToggleSessionPicker, Rerun, ToggleExpandItem, ToggleUserFrames])`, plus the data-carrying `ToggleDataBreakpoint` (`#[derive(Action)]`, namespace `debugger`) and a second `actions!(dev, [CopyDebugAdapterArguments])` block for copying launch args to the clipboard.

### Related Modules

- debugger_ui

### Related Data Models

- DebugSession

---

## BL006_NewProcessModalTabActions: NewProcessModalTabActions

**Type**: custom-command
**Trigger**: Keybinding (keymap.json) inside the New Process modal
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_ui/src/new_process_modal.rs
**Source Symbol**: ActivateTaskTab

### Description

`actions!(new_process_modal, [ActivateTaskTab, ActivateDebugTab, ActivateAttachTab, ActivateLaunchTab])` switches between the Task/Debug/Attach/Launch tabs of the modal used to start new debug or task processes.

### Related Modules

- debugger_ui

---

## BL007_BreakpointListNavigationActions: BreakpointListNavigationActions

**Type**: custom-command
**Trigger**: Keybinding while breakpoint list panel is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_ui/src/session/running/breakpoint_list.rs
**Source Symbol**: PreviousBreakpointProperty

### Description

`actions!(debugger, [PreviousBreakpointProperty, NextBreakpointProperty])` moves focus between editable properties (condition, hit count, log message) of the selected breakpoint entry.

### Related Modules

- debugger_ui

---

## BL008_ConsoleWatchExpressionAction: ConsoleWatchExpressionAction

**Type**: custom-command
**Trigger**: Keybinding / context menu while debugger console is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_ui/src/session/running/console.rs
**Source Symbol**: WatchExpression

### Description

`actions!(console, [WatchExpression])` adds the currently selected/typed expression in the debug console to the watch list.

### Related Modules

- debugger_ui

---

## BL009_MemoryViewGoToAddressAction: MemoryViewGoToAddressAction

**Type**: custom-command
**Trigger**: Keybinding / query-bar submit inside the memory view
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_ui/src/session/running/memory_view.rs
**Source Symbol**: GoToSelectedAddress

### Description

`actions!(debugger, [GoToSelectedAddress])` navigates the memory inspection view to the address entered in its query editor.

### Related Modules

- debugger_ui

---

## BL010_VariableListActions: VariableListActions

**Type**: custom-command
**Trigger**: Keybinding / context menu while variable list is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/debugger_ui/src/session/running/variable_list.rs
**Source Symbol**: ExpandSelectedEntry

### Description

`actions!(variable_list, [ExpandSelectedEntry, CollapseSelectedEntry, CopyVariableName, CopyVariableValue, EditVariable, AddWatch, ...])` drives expand/collapse of the variable tree and copy/edit/watch operations on the selected debug variable.

### Related Modules

- debugger_ui

---

## BL011_DeployCurrentFileDiagnosticsAction: DeployCurrentFileDiagnosticsAction

**Type**: custom-command
**Trigger**: Command palette / keybinding
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/diagnostics/src/buffer_diagnostics.rs
**Source Symbol**: DeployCurrentFile

### Description

`actions!(diagnostics, [DeployCurrentFile])` opens the buffer-scoped diagnostics editor for the currently focused file, showing only excerpts around diagnostics for that buffer.

### Related Modules

- diagnostics

### Related Data Models

- Project

---

## BL012_ProjectDiagnosticsActions: ProjectDiagnosticsActions

**Type**: custom-command
**Trigger**: Command palette / keybinding
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/diagnostics/src/diagnostics.rs
**Source Symbol**: Deploy

### Description

`actions!(diagnostics, [Deploy, ToggleWarnings, ToggleDiagnosticsRefresh])` opens the project-wide diagnostics view and toggles whether warning-level diagnostics and automatic refresh are shown.

### Related Modules

- diagnostics

---

## BL013_EditorCoreActions: EditorCoreActions

**Type**: custom-command
**Trigger**: Keybinding (keymap.json) — the editor's primary command surface
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/editor/src/actions.rs
**Source Symbol**: SelectNext

### Description

Defines the bulk of the editor's data-carrying keymap actions via repeated `#[derive(Action)]` (namespace `editor`): `SelectNext`, `SelectPrevious`, `MoveToBeginningOfLine`, `SelectToBeginningOfLine`, `DeleteToBeginningOfLine`, `MovePageUp`, `MovePageDown` and dozens more cursor/selection/edit commands, each carrying optional bool parameters (e.g. `replace_newest`, `stop_at_soft_wraps`).

### Related Modules

- editor

### Related Data Models

- Editor

---

## BL014_ToggleSplitDiffAction: ToggleSplitDiffAction

**Type**: custom-command
**Trigger**: Keybinding / toolbar toggle in a split editor
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/editor/src/split.rs
**Source Symbol**: ToggleSplitDiff

### Description

`#[derive(Action)] struct ToggleSplitDiff` (namespace `editor`) toggles the diff view style of a `SplittableEditor` between its two panes.

### Related Modules

- editor

### Related Data Models

- Editor

---

## BL015_EncodingSelectorToggleAction: EncodingSelectorToggleAction

**Type**: custom-command
**Trigger**: Command palette / status-bar encoding indicator click
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/encoding_selector/src/encoding_selector.rs
**Source Symbol**: Toggle

### Description

`actions!(encoding_selector, [Toggle])` opens the modal picker for changing the active buffer's text encoding.

### Related Modules

- encoding_selector

---

## BL016_EtwTracingActions: EtwTracingActions

**Type**: custom-command
**Trigger**: Command palette (Windows-only diagnostic tooling)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/etw_tracing/etw_tracing.rs
**Source Symbol**: RecordEtwTrace

### Description

`actions!(zed, [RecordEtwTrace, RecordEtwTraceWithHeapTracing, SaveEtwTrace, CancelEtwTrace])` starts/stops and saves Event Tracing for Windows (ETW) performance traces, optionally with heap tracing.

### Related Modules

- etw_tracing

---

## BL017_ReloadExtensionsAction: ReloadExtensionsAction

**Type**: custom-command
**Trigger**: Command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/extension_host/src/extension_host.rs
**Source Symbol**: ReloadExtensions

### Description

`actions!(zed, [ReloadExtensions])` reloads all installed extensions from disk without restarting the application.

### Related Modules

- extension_host

---

## BL018_InstallDevExtensionAction: InstallDevExtensionAction

**Type**: custom-command
**Trigger**: Command palette / "Install Dev Extension" button on the Extensions page
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/extensions_ui/src/extensions_ui.rs
**Source Symbol**: InstallDevExtension

### Description

`actions!(zed, [InstallDevExtension])` prompts for a local directory and installs it as a development extension.

### Related Modules

- extensions_ui

---

## BL019_FeedbackActions: FeedbackActions

**Type**: custom-command
**Trigger**: Command palette / Help menu
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/feedback/src/feedback.rs
**Source Symbol**: OpenZedRepo

### Description

`actions!(zed, [OpenZedRepo, CopyInstalledExtensionsIntoClipboard])` opens the project's GitHub repository in the browser and copies the installed-extensions list to the clipboard for bug reports.

### Related Modules

- feedback

---

## BL020_FileFinderActions: FileFinderActions

**Type**: custom-command
**Trigger**: Keybinding while the file finder modal is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/file_finder/src/file_finder.rs
**Source Symbol**: SelectPrevious

### Description

`actions!(file_finder, [SelectPrevious, ToggleFilterMenu, ToggleSplitMenu])` navigates file-finder results and toggles its filter/split option menus.

### Related Modules

- file_finder

---

## BL021_GitHunkStagingActions: GitHunkStagingActions

**Type**: custom-command
**Trigger**: Keybinding / git panel context menu on a hunk or status entry
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git/src/git.rs
**Source Symbol**: ToggleStaged

### Description

`actions!(git, [ToggleStaged, StageRange, StageAndNext, UnstageAndNext, Restore, ...])` drives per-hunk staging/unstaging/restoring, plus the data-carrying `RenameBranch` and `RestoreFile` (`#[derive(Action)]`, namespace `git`) for renaming branches and discarding local file changes.

### Related Modules

- git

### Related Data Models

- Oid

---

## BL022_GitGraphActions: GitGraphActions

**Type**: custom-command
**Trigger**: Keybinding / command palette from the Git Graph tab
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_graph/src/git_graph.rs
**Source Symbol**: OpenCommitView

### Description

`actions!(git_graph, [OpenCommitView, FocusSearch])` opens the commit-detail view for the selected graph node and focuses the graph's search field.

### Related Modules

- git_graph

---

## BL023_BranchPickerActions: BranchPickerActions

**Type**: custom-command
**Trigger**: Keybinding while the branch picker is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/branch_picker.rs
**Source Symbol**: DeleteBranch

### Description

`actions!(branch_picker, [DeleteBranch, FilterRemotes])` deletes the selected local/remote branch and toggles filtering of remote branches in the picker list.

### Related Modules

- git_ui

---

## BL024_CommitViewStashActions: CommitViewStashActions

**Type**: custom-command
**Trigger**: Command palette / keybinding
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/commit_view.rs
**Source Symbol**: ApplyCurrentStash

### Description

`actions!(git, [ApplyCurrentStash, PopCurrentStash, DropCurrentStash])` applies, pops, or drops the currently selected stash entry from the commit view.

### Related Modules

- git_ui

---

## BL025_GitPanelActions: GitPanelActions

**Type**: custom-command
**Trigger**: Keybinding / git panel toolbar and context menu
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/git_panel.rs
**Source Symbol**: Close

### Description

`actions!(git_panel, [Close, Toggle, ToggleFocus, OpenMenu, FocusEditor, FocusChanges, ...])` controls the git panel's visibility, focus and menu; the file also declares `actions!(git_graph, [Open])` and the data-carrying `OpenAtCommit { sha: String }` (`gpui::Action`, namespace `git_graph`) for opening the Git Graph tab at a specific commit.

### Related Modules

- git_ui

---

## BL026_GitPickerTabActions: GitPickerTabActions

**Type**: custom-command
**Trigger**: Keybinding while the combined git picker is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/git_picker.rs
**Source Symbol**: ActivateBranchesTab

### Description

`actions!(git_picker, [ActivateBranchesTab, ActivateStashTab])` switches between the Branches and Stash tabs of the combined git picker.

### Related Modules

- git_ui

---

## BL027_ProjectDiffActions: ProjectDiffActions

**Type**: custom-command
**Trigger**: Keybinding / command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/project_diff.rs
**Source Symbol**: Diff

### Description

`actions!(git, [Diff, Add, BranchDiff, LeaderAndFollower])` opens the working-directory diff view, stages files, and diffs against the default branch.

### Related Modules

- git_ui

### Related Data Models

- Project

---

## BL028_StashPickerActions: StashPickerActions

**Type**: custom-command
**Trigger**: Keybinding while the stash picker is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/stash_picker.rs
**Source Symbol**: DropStashItem

### Description

`actions!(stash_picker, [DropStashItem, ShowStashItem])` drops or shows the diff of the selected stash entry.

### Related Modules

- git_ui

---

## BL029_DeleteWorktreeAction: DeleteWorktreeAction

**Type**: custom-command
**Trigger**: Keybinding while the worktree picker is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git_ui/src/worktree_picker.rs
**Source Symbol**: DeleteWorktree

### Description

`actions!(worktree_picker, [DeleteWorktree])` removes the selected git worktree from disk and from the project.

### Related Modules

- git_ui

### Related Data Models

- Project

---

## BL030_ActionsMacroDefinition: ActionsMacroDefinition

**Type**: custom-command
**Trigger**: N/A — compile-time declarative macro, not an invokable command itself
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/gpui/src/action.rs
**Source Symbol**: actions

### Description

`[SIGNAL_INFERRED]` This file defines the `actions!` `macro_rules!` and documents the `Action` derive macro used by every other custom-command entry in this shard; it registers no action of its own. Included per scout inventory because it is the shared mechanism (`#[derive(Action)]`, `#[action(namespace = ...)]`) that all 99 sibling BL items depend on.

### Related Modules

- gpui

---

## BL031_ImageViewerZoomActions: ImageViewerZoomActions

**Type**: custom-command
**Trigger**: Keybinding / toolbar buttons while viewing an image
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/image_viewer/src/image_viewer.rs
**Source Symbol**: ZoomIn

### Description

`actions!(image_viewer, [ZoomIn, ZoomOut, ResetZoom, FitToView, ZoomToActualSize])` controls zoom level of the built-in image preview.

### Related Modules

- image_viewer

---

## BL032_DumpInputLatencyHistogramAction: DumpInputLatencyHistogramAction

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/input_latency_ui/src/input_latency_ui.rs
**Source Symbol**: DumpInputLatencyHistogram

### Description

`actions!(dev, [DumpInputLatencyHistogram])` writes a formatted input-to-frame latency histogram report for the current window, diffed against the previous report if one exists.

### Related Modules

- input_latency_ui

---

## BL033_InstallCliBinaryAction: InstallCliBinaryAction

**Type**: custom-command
**Trigger**: Command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/install_cli/src/install_cli_binary.rs
**Source Symbol**: InstallCliBinary

### Description

`actions!(cli, [InstallCliBinary])` symlinks the Zed CLI auxiliary executable into `/usr/local/bin/zed` (skipping if already up to date).

### Related Modules

- install_cli

---

## BL034_RegisterZedSchemeAction: RegisterZedSchemeAction

**Type**: custom-command
**Trigger**: Command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/install_cli/src/register_zed_scheme.rs
**Source Symbol**: RegisterZedScheme

### Description

`actions!(cli, [RegisterZedScheme])` registers the `zed://` URL scheme handler with the OS.

### Related Modules

- install_cli

---

## BL035_NewJournalEntryAction: NewJournalEntryAction

**Type**: custom-command
**Trigger**: Command palette / keybinding
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/journal/src/journal.rs
**Source Symbol**: NewJournalEntry

### Description

`actions!(journal, [NewJournalEntry])` creates (or appends to) today's journal entry file at the configured journal path.

### Related Modules

- journal

---

## BL036_KeymapEditorActions: KeymapEditorActions

**Type**: custom-command
**Trigger**: Keybinding / context menu inside the keymap editor
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/keymap_editor/src/keymap_editor.rs
**Source Symbol**: EditBinding

### Description

`actions!(keymap_editor, [EditBinding, CreateBinding, OpenCreateKeybindingModal, DeleteBinding, CopyAction, CopyContext, ToggleConflictFilter, ...])` edits, creates, deletes and copies keybindings and their context predicates from the keymap editor UI.

### Related Modules

- keymap_editor

---

## BL037_KeystrokeRecordingActions: KeystrokeRecordingActions

**Type**: custom-command
**Trigger**: Keybinding / click on the keystroke-capture input widget
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/keymap_editor/src/ui_components/keystroke_input.rs
**Source Symbol**: StartRecording

### Description

`actions!(keystroke_input, [StartRecording, StopRecording, ClearKeystrokes])` drives the keystroke-capture widget used when defining a new keybinding.

### Related Modules

- keymap_editor

---

## BL038_LanguageSelectorToggleAction: LanguageSelectorToggleAction

**Type**: custom-command
**Trigger**: Command palette / status-bar language indicator click
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_selector/src/language_selector.rs
**Source Symbol**: Toggle

### Description

`actions!(language_selector, [Toggle])` opens the modal picker for changing the active buffer's assigned language.

### Related Modules

- language_selector

---

## BL039_HighlightsTreeViewActions: HighlightsTreeViewActions

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_tools/src/highlights_tree_view.rs
**Source Symbol**: OpenHighlightsTreeView

### Description

`actions!(dev, [OpenHighlightsTreeView])` opens the highlights tree debugging view; the file additionally declares `actions!(highlights_tree_view, [ToggleTextHighlights, ToggleSemanticTokens, ToggleSyntaxTokens])` to toggle which highlight layers are shown.

### Related Modules

- language_tools

---

## BL040_OpenKeyContextViewAction: OpenKeyContextViewAction

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_tools/src/key_context_view.rs
**Source Symbol**: OpenKeyContextView

### Description

`actions!(dev, [OpenKeyContextView])` opens a debugging view showing the current keybinding context predicate stack.

### Related Modules

- language_tools

---

## BL041_LspToolMenuToggleAction: LspToolMenuToggleAction

**Type**: custom-command
**Trigger**: Click on the LSP status-bar button / keybinding
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_tools/src/lsp_button.rs
**Source Symbol**: ToggleMenu

### Description

`actions!(lsp_tool, [ToggleMenu])` opens/closes the language-server status popover menu.

### Related Modules

- language_tools

---

## BL042_OpenLanguageServerLogsAction: OpenLanguageServerLogsAction

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_tools/src/lsp_log_view.rs
**Source Symbol**: OpenLanguageServerLogs

### Description

`actions!(dev, [OpenLanguageServerLogs])` opens the language server protocol log viewer, backed by a project-scoped `LogStore`.

### Related Modules

- language_tools

---

## BL043_SyntaxTreeViewActions: SyntaxTreeViewActions

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_tools/src/syntax_tree_view.rs
**Source Symbol**: OpenSyntaxTreeView

### Description

`actions!(dev, [OpenSyntaxTreeView])` opens the Tree-sitter syntax tree view for the current file; `actions!(syntax_tree_view, [UseActiveEditor])` refreshes it to track the last-focused editor.

### Related Modules

- language_tools

---

## BL044_LineEndingSelectorToggleAction: LineEndingSelectorToggleAction

**Type**: custom-command
**Trigger**: Command palette / status-bar line-ending indicator click
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/line_ending_selector/src/line_ending_selector.rs
**Source Symbol**: Toggle

### Description

`actions!(line_ending_selector, [Toggle])` opens the modal picker for changing the buffer's line-ending convention (LF/CRLF).

### Related Modules

- line_ending_selector

---

## BL045_MarkdownCopyActions: MarkdownCopyActions

**Type**: custom-command
**Trigger**: Keybinding / context menu on selected markdown text
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/markdown/src/markdown.rs
**Source Symbol**: Copy

### Description

`actions!(markdown, [Copy, CopyAsMarkdown])` copies the current selection as plain text or as raw markdown source.

### Related Modules

- markdown

---

## BL046_MarkdownPreviewScrollActions: MarkdownPreviewScrollActions

**Type**: custom-command
**Trigger**: Keybinding while a markdown preview pane is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/markdown_preview/src/markdown_preview.rs
**Source Symbol**: ScrollPageUp

### Description

`actions!(markdown, [ScrollPageUp, ScrollPageDown, ScrollUp, ScrollDown, ScrollUpByItem, ScrollDownByItem, ...])` scrolls the rendered markdown preview by page, line, or element; `ScrollPageUp`/`ScrollPageDown` carry deprecated aliases from `markdown::MovePageUp`/`MovePageDown`.

### Related Modules

- markdown_preview

---

## BL047_MenuNavigationActions: MenuNavigationActions

**Type**: custom-command
**Trigger**: Keybinding while any menu/list/picker is focused (shared cross-cutting namespace)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/menu/src/menu.rs
**Source Symbol**: Cancel

### Description

`actions!(menu, [Cancel, Confirm, SecondaryConfirm, SelectPrevious, SelectNext, SelectFirst, SelectLast])` defines the shared navigation/confirmation vocabulary reused by every list-like UI (pickers, context menus, completion popovers) across the app.

### Related Modules

- menu

---

## BL048_ToggleBaseKeymapSelectorAction: ToggleBaseKeymapSelectorAction

**Type**: custom-command
**Trigger**: Command palette / onboarding flow
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/onboarding/src/base_keymap_picker.rs
**Source Symbol**: ToggleBaseKeymapSelector

### Description

`actions!(zed, [ToggleBaseKeymapSelector])` opens the modal picker for choosing the base keymap (Default, VS Code, Vim, etc.).

### Related Modules

- onboarding

---

## BL049_OnboardingFlowActions: OnboardingFlowActions

**Type**: custom-command
**Trigger**: Buttons within the first-run onboarding screen
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/onboarding/src/onboarding.rs
**Source Symbol**: Finish

### Description

`actions!(onboarding, [Finish, ResetHints])` completes the onboarding flow and resets multibuffer hint counters; the file also declares the data-carrying `ImportVsCodeSettings { skip_prompt }` and `ImportCursorSettings { skip_prompt }` (`#[derive(Action)]`, namespace `zed`) for one-shot settings migration from other editors.

### Related Modules

- onboarding

---

## BL050_OutlinePanelActions: OutlinePanelActions

**Type**: custom-command
**Trigger**: Keybinding / context menu while the outline panel is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/outline_panel/src/outline_panel.rs
**Source Symbol**: CollapseAllEntries

### Description

`actions!(outline_panel, [CollapseAllEntries, CollapseSelectedEntry, ExpandAllEntries, ExpandSelectedEntry, FoldDirectory, OpenSelectedEntry, ...])` drives tree expand/collapse/navigation and reveal-in-file-manager for the outline/symbol panel.

### Related Modules

- outline_panel

---

## BL051_PanelTabNavigationActions: PanelTabNavigationActions

**Type**: custom-command
**Trigger**: Keybinding while a tabbed dock panel is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/panel/src/panel.rs
**Source Symbol**: NextPanelTab

### Description

`actions!(panel, [NextPanelTab, PreviousPanelTab])` is the shared next/previous-tab navigation used by any panel implementing the `PanelHeader` trait.

### Related Modules

- panel

---

## BL052_PickerConfirmActions: PickerConfirmActions

**Type**: custom-command
**Trigger**: Keybinding while a picker's input editor is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/picker/src/picker.rs
**Source Symbol**: ConfirmCompletion

### Description

`actions!(picker, [ConfirmCompletion])` confirms an in-progress inline completion inside a picker's editor; the file also defines `ConfirmInput { secondary: bool }` (`#[derive(Action)]`, namespace `picker`) which acts on the picker's literal text input rather than the highlighted entry.

### Related Modules

- picker

---

## BL053_WindowTabManagementActions: WindowTabManagementActions

**Type**: custom-command
**Trigger**: Keybinding / Window menu (native window tabs)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/platform_title_bar/src/system_window_tabs.rs
**Source Symbol**: ShowNextWindowTab

### Description

`actions!(window, [ShowNextWindowTab, ShowPreviousWindowTab, MergeAllWindows, MoveTabToNewWindow])` manages OS-native window tabs (macOS-style tabbed windows).

### Related Modules

- platform_title_bar

---

## BL054_ContextServerRestartAction: ContextServerRestartAction

**Type**: custom-command
**Trigger**: Command palette / context server status UI
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/context_server_store.rs
**Source Symbol**: Restart

### Description

`actions!(context_server, [Restart])` restarts a stopped or errored MCP context server.

### Related Modules

- project

### Related Data Models

- ContextServerStatus

---

## BL055_ProjectPanelActions: ProjectPanelActions

**Type**: custom-command
**Trigger**: Keybinding / context menu while the project panel is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project_panel/src/project_panel.rs
**Source Symbol**: ExpandSelectedEntry

### Description

`actions!(project_panel, [ExpandSelectedEntry, CollapseSelectedEntry, CollapseSelectedEntryAndChildren, CollapseAllEntries, NewDirectory, NewFile, Copy, ...])` drives file-tree navigation and creation; the file also declares data-carrying `Delete { skip_prompt }`, `Trash { skip_prompt }`, `SelectNextDiagnostic { severity }`, `SelectPrevDiagnostic { severity }` (`#[derive(Action)]`, namespace `project_panel`).

### Related Modules

- project_panel

### Related Data Models

- Project

---

## BL056_RecentProjectsMenuActions: RecentProjectsMenuActions

**Type**: custom-command
**Trigger**: Keybinding while the recent-projects picker is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/recent_projects/src/recent_projects.rs
**Source Symbol**: ToggleActionsMenu

### Description

`actions!(recent_projects, [ToggleActionsMenu, RemoveSelected, AddToWorkspace])` toggles the per-entry actions menu and removes/adds recent-project entries to the multi-project workspace.

### Related Modules

- recent_projects

### Related Data Models

- WorkspaceId

---

## BL057_OpenWslPathAction: OpenWslPathAction

**Type**: custom-command
**Trigger**: Programmatic (WSL remote connection flow), not user-keybound
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/remote/src/remote_client.rs
**Source Symbol**: OpenWslPath

### Description

`#[derive(gpui::Action)] struct OpenWslPath { distro, paths }` (namespace `workspace`, `no_json`/`no_register`) opens a set of paths inside a WSL distro; excluded from JSON keymap binding and auto-registration since it is dispatched programmatically.

### Related Modules

- remote

---

## BL058_ReplSessionActions: ReplSessionActions

**Type**: custom-command
**Trigger**: Keybinding / command palette in a REPL-connected editor
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/repl/src/repl_sessions_ui.rs
**Source Symbol**: Run

### Description

`actions!(repl, [Run, RunInPlace, ClearOutputs, ClearCurrentOutput, Sessions, Interrupt, ...])` runs/interrupts Jupyter-kernel cells and manages the REPL sessions panel.

### Related Modules

- repl

---

## BL059_ProjectSearchActions: ProjectSearchActions

**Type**: custom-command
**Trigger**: Keybinding / command palette while project search is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/search/src/project_search.rs
**Source Symbol**: SearchInNew

### Description

`actions!(project_search, [SearchInNew, ToggleFocus, NextField, ToggleFilters, ToggleAllSearchResults])` opens a new project-wide search tab and navigates its input fields and filters.

### Related Modules

- search

---

## BL060_BufferSearchActions: BufferSearchActions

**Type**: custom-command
**Trigger**: Keybinding / command palette while the in-buffer search bar is open
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/search/src/search.rs
**Source Symbol**: FocusSearch

### Description

`actions!(search, [FocusSearch, ToggleWholeWord, ToggleCaseSensitive, ToggleRegex, ToggleReplace, ToggleSelection, ...])` toggles buffer-search options (whole word, case sensitivity, regex, replace, selection-only).

### Related Modules

- search

### Related Data Models

- Editor

---

## BL061_SettingsEditorActions: SettingsEditorActions

**Type**: custom-command
**Trigger**: Keybinding while the Settings UI is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/settings_ui/src/settings_ui.rs
**Source Symbol**: Minimize

### Description

`actions!(settings_editor, [Minimize, ToggleFocusNav, ExpandNavEntry, CollapseNavEntry, FocusNextFile, FocusPreviousFile, ...])` drives the settings UI navbar and file navigation; the file also declares `FocusFile(u32)` (`#[derive(Action)]`, namespace `settings_editor`) to jump directly to a specific settings file by index.

### Related Modules

- settings_ui

---

## BL062_FocusSidebarFilterAction: FocusSidebarFilterAction

**Type**: custom-command
**Trigger**: Keybinding
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/sidebar/src/sidebar.rs
**Source Symbol**: FocusSidebarFilter

### Description

`actions!(sidebar, [FocusSidebarFilter])` moves keyboard focus into the multi-project sidebar's search/filter editor.

### Related Modules

- sidebar

### Related Data Models

- MultiWorkspace

---

## BL063_SnippetsConfigActions: SnippetsConfigActions

**Type**: custom-command
**Trigger**: Command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/snippets_ui/src/snippets_ui.rs
**Source Symbol**: ConfigureSnippets

### Description

`actions!(snippets, [ConfigureSnippets, OpenFolder])` opens the user snippets JSON file and the underlying snippets folder in the system file manager.

### Related Modules

- snippets_ui

---

## BL064_SvgOpenFollowingPreviewAction: SvgOpenFollowingPreviewAction

**Type**: custom-command
**Trigger**: Command palette / editor tab context menu on `.svg` files
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/svg_preview/src/svg_preview.rs
**Source Symbol**: OpenFollowingPreview

### Description

`actions!(svg, [OpenFollowingPreview])` opens an SVG preview pane that follows and re-renders the active editor's content.

### Related Modules

- svg_preview

---

## BL065_CopySystemSpecsAction: CopySystemSpecsAction

**Type**: custom-command
**Trigger**: Command palette / Help menu
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/system_specs/src/system_specs.rs
**Source Symbol**: CopySystemSpecsIntoClipboard

### Description

`actions!(zed, [CopySystemSpecsIntoClipboard])` copies OS/architecture/memory/app-version diagnostics to the clipboard for bug reports.

### Related Modules

- system_specs

---

## BL066_TabSwitcherActions: TabSwitcherActions

**Type**: custom-command
**Trigger**: Keybinding (Ctrl/Cmd+Tab-style modal)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/tab_switcher/src/tab_switcher.rs
**Source Symbol**: Toggle

### Description

Declares `Toggle { select_last: bool }` (`#[derive(Action)]`, namespace `tab_switcher`) to open the modal tab switcher, plus `actions!(tab_switcher, [CloseSelectedItem, ToggleAll, OpenInActivePane])` for closing the selected tab and toggling between per-pane and all-tabs view.

### Related Modules

- tab_switcher

---

## BL067_TerminalCoreActions: TerminalCoreActions

**Type**: custom-command
**Trigger**: Keybinding while a terminal pane is focused
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/terminal/src/terminal.rs
**Source Symbol**: Clear

### Description

`actions!(terminal, [Clear, Copy, Paste, ShowCharacterPalette, SearchTest, ScrollLineUp, ...])` provides the core terminal emulator commands: clearing the screen, clipboard copy/paste, and line-based scrolling.

### Related Modules

- terminal

---

## BL068_TerminalPanelToggleActions: TerminalPanelToggleActions

**Type**: custom-command
**Trigger**: Keybinding / command palette
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/terminal_view/src/terminal_panel.rs
**Source Symbol**: Toggle

### Description

`actions!(terminal_panel, [Toggle, ToggleFocus])` shows/hides the terminal dock panel and toggles keyboard focus into it.

### Related Modules

- terminal_view

---

## BL069_TerminalViewTextActions: TerminalViewTextActions

**Type**: custom-command
**Trigger**: Programmatic / task-runner integration (SendText, SendKeystroke) and keybinding (RerunTask, RenameTerminal)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/terminal_view/src/terminal_view.rs
**Source Symbol**: SendText

### Description

Declares `SendText(String)` and `SendKeystroke(String)` (`#[derive(Action)]`, namespace `terminal`) for programmatically feeding text/keystrokes into a terminal, `actions!(terminal, [RerunTask])` to re-run the last task, and `RenameTerminal` for renaming a terminal tab.

### Related Modules

- terminal_view

---

## BL070_ThemeSelectorReloadAction: ThemeSelectorReloadAction

**Type**: custom-command
**Trigger**: Command palette (dev/theme-authoring workflow)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/theme_selector/src/theme_selector.rs
**Source Symbol**: Reload

### Description

`actions!(theme_selector, [Reload])` reloads all themes from disk, used while iterating on theme JSON files.

### Related Modules

- theme_selector

---

## BL071_ApplicationMenuActivationActions: ApplicationMenuActivationActions

**Type**: custom-command
**Trigger**: Keybinding within the client-side (non-native) application menu bar
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/title_bar/src/application_menu.rs
**Source Symbol**: ActivateMenuRight

### Description

`actions!(app_menu, [ActivateMenuRight, ActivateMenuLeft])` moves activation between adjacent top-level menus in the client-rendered application menu (non-macOS); `OpenApplicationMenu(String)` (`#[derive(Action)]`, namespace `app_menu`) opens a named menu directly.

### Related Modules

- title_bar

---

## BL072_TitleBarProjectMenuActions: TitleBarProjectMenuActions

**Type**: custom-command
**Trigger**: Click on the title bar's project/branch dropdown
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/title_bar/src/title_bar.rs
**Source Symbol**: ToggleProjectMenu

### Description

`actions!(collab, [ToggleProjectMenu, SwitchBranch])` opens the title bar's project menu dropdown and initiates a git branch switch from it.

### Related Modules

- title_bar

---

## BL073_ToolchainSelectorActions: ToolchainSelectorActions

**Type**: custom-command
**Trigger**: Command palette / status-bar toolchain indicator click
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/toolchain_selector/src/toolchain_selector.rs
**Source Symbol**: Select

### Description

`actions!(toolchain, [Select, AddToolchain])` opens the modal picker for selecting or adding a language toolchain (e.g. Python venv, Node version) for the current project.

### Related Modules

- toolchain_selector

### Related Data Models

- Project

---

## BL074_VimChangeListNavigationActions: VimChangeListNavigationActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`g;` / `g,`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/change_list.rs
**Source Symbol**: ChangeListOlder

### Description

`actions!(vim, [ChangeListOlder, ChangeListNewer])` navigates backward/forward through the Vim change-list (positions of recent edits).

### Related Modules

- vim

---

## BL075_VimExCommandActions: VimExCommandActions

**Type**: custom-command
**Trigger**: Vim `:`-command line (ex commands)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/command.rs
**Source Symbol**: GoToLine

### Description

Declares the data-carrying ex-command actions `GoToLine { range }`, `YankCommand { range }`, `WithRange { restore_selection, range, action }`, `WithCount { count, action }`, `VimSet { options }`, `VimSave { range, save_intent, filename }`, `VimSplit { vertical }` (all `#[derive(Action)]`, namespace `vim`, `no_json`/`no_register`) — the internal representation Vim's `:` command parser dispatches to.

### Related Modules

- vim

---

## BL076_VimDigraphLiteralAction: VimDigraphLiteralAction

**Type**: custom-command
**Trigger**: Vim insert-mode digraph sequence (`Ctrl-K` + two chars)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/digraph.rs
**Source Symbol**: Literal

### Description

`Literal(String, char)` (`#[derive(Action)]`, namespace `vim`) inserts the Unicode character mapped to a two-character digraph code.

### Related Modules

- vim

---

## BL077_VimHelixModeActions: VimHelixModeActions

**Type**: custom-command
**Trigger**: Keybinding when Vim's Helix emulation mode is active
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/helix.rs
**Source Symbol**: HelixYank

### Description

`actions!(vim, [HelixYank, HelixInsert, HelixAppend, HelixInsertEndOfLine, HelixGotoLastModification, HelixSelectLine, ...])` implements Helix-editor-style selection-first commands within Vim mode.

### Related Modules

- vim

---

## BL078_VimHelixPasteAction: VimHelixPasteAction

**Type**: custom-command
**Trigger**: Keybinding in Helix mode
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/helix/paste.rs
**Source Symbol**: HelixPaste

### Description

`HelixPaste { before: bool }` (`#[derive(Action)]`, namespace `vim`) pastes the register contents before or after the current selection in Helix mode.

### Related Modules

- vim

---

## BL079_VimIndentActions: VimIndentActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal/visual mode (`>`, `<`, `=`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/indent.rs
**Source Symbol**: Indent

### Description

`actions!(vim, [Indent, Outdent, AutoIndent])` increases, decreases, or syntax-auto-adjusts indentation of the selected lines.

### Related Modules

- vim

---

## BL080_VimInsertModeActions: VimInsertModeActions

**Type**: custom-command
**Trigger**: Keybinding while in Vim insert mode
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/insert.rs
**Source Symbol**: NormalBefore

### Description

`actions!(vim, [NormalBefore, TemporaryNormal, InsertFromAbove, InsertFromBelow])` exits to normal mode, does a one-shot normal command, or inserts the character above/below the cursor.

### Related Modules

- vim

---

## BL081_VimMotionActions: VimMotionActions

**Type**: custom-command
**Trigger**: Keybinding — Vim motion commands (`w`, `e`, `b`, `k`/`j`, etc.)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/motion.rs
**Source Symbol**: NextWordStart

### Description

Declares the bulk of Vim's word/subword/line motion vocabulary as data-carrying actions (`#[derive(Action)]`, namespace `vim`): `NextWordStart`, `NextWordEnd`, `PreviousWordStart`, `PreviousWordEnd`, `NextSubwordStart/End`, `PreviousSubwordStart/End`, `Up`, and further vertical/line motions, most parameterized by `ignore_punctuation`/`display_lines`.

### Related Modules

- vim

---

## BL082_VimNormalModeInsertActions: VimNormalModeInsertActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`i`, `a`, `I`, `A`, `o`, `O`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal.rs
**Source Symbol**: InsertAfter

### Description

`actions!(vim, [InsertAfter, InsertBefore, InsertFirstNonWhitespace, InsertEndOfLine, InsertLineAbove, ...])` transitions from Vim normal mode into insert mode at various cursor positions.

### Related Modules

- vim

---

## BL083_VimIncrementDecrementActions: VimIncrementDecrementActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`Ctrl-A` / `Ctrl-X`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal/increment.rs
**Source Symbol**: Increment

### Description

`Increment { step: bool }` and `Decrement { step: bool }` (`#[derive(Action)]`, namespace `vim`) increment/decrement the number (or toggle boolean) under the cursor.

### Related Modules

- vim

---

## BL084_VimPasteAction: VimPasteAction

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`p` / `P`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal/paste.rs
**Source Symbol**: Paste

### Description

`Paste { before: bool, preserve_clipboard: bool }` (`#[derive(Action)]`, namespace `vim`) pastes the active register before or after the cursor/selection.

### Related Modules

- vim

---

## BL085_VimRepeatMacroActions: VimRepeatMacroActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`.`, `q`, `@@`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal/repeat.rs
**Source Symbol**: Repeat

### Description

`actions!(vim, [Repeat, EndRepeat, ToggleRecord, ReplayLastRecording])` repeats the last change and drives macro recording/playback.

### Related Modules

- vim

---

## BL086_VimScrollActions: VimScrollActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`Ctrl-E`/`Ctrl-Y`, `zz`, etc.)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal/scroll.rs
**Source Symbol**: LineUp

### Description

`actions!(vim, [LineUp, LineDown, ColumnRight, ColumnLeft, ScrollUp, ...])` scrolls the viewport by line, column, or half/full page without moving the cursor semantically.

### Related Modules

- vim

---

## BL087_VimSearchActions: VimSearchActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`/`, `?`, `*`, `#`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal/search.rs
**Source Symbol**: MoveToNext

### Description

Declares `MoveToNext`, `MoveToPrevious`, `SearchUnderCursor`, `SearchUnderCursorPrevious`, `Search`, `FindCommand`, `ReplaceCommand` (`#[derive(Action)]`, namespace `vim`), each carrying `case_sensitive`/`partial_word`/`regex` flags, implementing Vim's `/`, `?`, `*`, `#` search motions and `:s` substitute-via-search.

### Related Modules

- vim

---

## BL088_VimSubstituteActions: VimSubstituteActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`s`, `S`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/normal/substitute.rs
**Source Symbol**: Substitute

### Description

`actions!(vim, [Substitute, SubstituteLine])` deletes the selection/line and enters insert mode in its place.

### Related Modules

- vim

---

## BL089_VimTextObjectActions: VimTextObjectActions

**Type**: custom-command
**Trigger**: Keybinding as the object half of a Vim operator+object combo (`diw`, `ci(`, etc.)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/object.rs
**Source Symbol**: Word

### Description

Declares text-object actions `Word { ignore_punctuation }`, `Subword`, `IndentObj { include_below }`, `Parentheses { opening }`, `SquareBrackets { opening }`, `AngleBrackets { opening }`, `CurlyBrackets { opening }` (`#[derive(Action)]`, namespace `vim`), plus `actions!(vim, [Sentence, Paragraph, ...])` for further text objects (quotes, tags, etc.) consumed by Vim operator-pending mode.

### Related Modules

- vim

---

## BL090_VimReplaceModeActions: VimReplaceModeActions

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`R`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/replace.rs
**Source Symbol**: ToggleReplace

### Description

`actions!(vim, [ToggleReplace, UndoReplace])` toggles Vim replace (overtype) mode and undoes the last in-place replacement.

### Related Modules

- vim

---

## BL091_VimRewrapAction: VimRewrapAction

**Type**: custom-command
**Trigger**: Keybinding in Vim normal mode (`gq`)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/rewrap.rs
**Source Symbol**: Rewrap

### Description

`Rewrap { line_length: Option<usize> }` (`#[derive(Action)]`, namespace `vim`) reflows/rewraps the selected text to the given (or configured) line length.

### Related Modules

- vim

---

## BL092_VimOperatorPendingPushActions: VimOperatorPendingPushActions

**Type**: custom-command
**Trigger**: Internal — pushed onto Vim's operator-pending stack while a multi-key command is being composed
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/vim.rs
**Source Symbol**: Number

### Description

Declares the internal operator-pending "push" actions `Number(usize)`, `SelectRegister(String)`, `PushObject { around }`, `PushFindForward { before, multiline }`, `PushFindBackward { after, multiline }`, `PushHelixNext/Previous { around }`, `PushSneak`/`PushSneakBackward { first_char }`, `PushAddSurrounds`, `PushChangeSurrounds { target }`, `PushJump { line }` (all `#[derive(Action)]`, namespace `vim`) that build up Vim's multi-keystroke command state machine.

### Related Modules

- vim

---

## BL093_VimVisualModeActions: VimVisualModeActions

**Type**: custom-command
**Trigger**: Keybinding in Vim visual mode (`v`, `V`, `Ctrl-V`, `d`, `x`, etc.)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/visual.rs
**Source Symbol**: ToggleVisual

### Description

`actions!(vim, [ToggleVisual, ToggleVisualLine, ToggleVisualBlock, VisualDelete, VisualDeleteLine, ...])` enters/exits the three visual-selection sub-modes and deletes the current visual selection.

### Related Modules

- vim

---

## BL094_MultiWorkspaceSidebarActions: MultiWorkspaceSidebarActions

**Type**: custom-command
**Trigger**: Keybinding / click on the multi-project sidebar
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/workspace/src/multi_workspace.rs
**Source Symbol**: ToggleWorkspaceSidebar

### Description

`actions!(multi_workspace, [ToggleWorkspaceSidebar, CloseWorkspaceSidebar, FocusWorkspaceSidebar, NextProject, PreviousProject])` toggles the multi-project sidebar and cycles the active project within a window.

### Related Modules

- workspace

### Related Data Models

- MultiWorkspace

---

## BL095_PaneItemManagementActions: PaneItemManagementActions

**Type**: custom-command
**Trigger**: Keybinding / tab bar interaction on a pane
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/workspace/src/pane.rs
**Source Symbol**: ActivateItem

### Description

Declares `ActivateItem(usize)`, `CloseActiveItem { save_intent, close_pinned }`, `CloseOtherItems`, `CloseMultibufferItems`, `CloseAllItems`, `CloseCleanItems`, `CloseItemsToTheRight/Left { close_pinned }` (all `#[derive(Action)]`, namespace `pane`) governing which tabs/items are activated or closed and whether unsaved-changes prompts are shown.

### Related Modules

- workspace

### Related Data Models

- Pane

---

## BL096_OpenThemePreviewAction: OpenThemePreviewAction

**Type**: custom-command
**Trigger**: Command palette (dev tools)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/workspace/src/theme_preview.rs
**Source Symbol**: OpenThemePreview

### Description

`actions!(dev, [OpenThemePreview])` opens a window rendering a gallery of the current theme's UI colors/tokens.

### Related Modules

- workspace

---

## BL097_WelcomeScreenActions: WelcomeScreenActions

**Type**: custom-command
**Trigger**: Command palette / click on a recent-project entry on the welcome screen
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/workspace/src/welcome.rs
**Source Symbol**: ShowWelcome

### Description

`actions!(zed, [ShowWelcome])` opens the Zed welcome screen; `OpenRecentProject { index }` (`#[derive(Action)]`, namespace `welcome`) opens the recent-project entry at the given index from that screen.

### Related Modules

- workspace

---

## BL098_WorkspacePaneNavigationActions: WorkspacePaneNavigationActions

**Type**: custom-command
**Trigger**: Keybinding / command palette — core workspace pane/window navigation
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/workspace/src/workspace.rs
**Source Symbol**: Open

### Description

Declares `Open { create_new_window: bool }` (`#[derive(Action)]`, namespace `workspace`) for opening paths into a new window or the current multi-project workspace; `actions!(workspace, [ActivateNextPane, ActivatePreviousPane, ActivateLastPane, ActivateNextWindow, ActivatePreviousWindow, ...])` for pane/window cycling; plus data-carrying `ActivatePane(usize)`, `MoveItemToPane { destination, focus, clone }`, `MoveItemToPaneInDirection { direction, focus, clone }`, `NewFileSplit(SplitDirection)`, `SaveAll { save_intent }` and further save/split actions in the same file.

### Related Modules

- workspace

### Related Data Models

- Workspace

---

## BL099_ZedApplicationWindowActions: ZedApplicationWindowActions

**Type**: custom-command
**Trigger**: Keybinding / command palette / native application menu
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/zed/src/zed.rs
**Source Symbol**: DebugElements

### Description

`actions!(zed, [DebugElements, Hide, HideOthers, Minimize, OpenDefaultSettings, ...])` provides top-level application window commands (element inspector, hide/minimize window, open default settings); the file also declares `actions!(dev, [OpenUrlPrompt])` for a URL-entry prompt. (A `actions!(test_only, [ActionA, ActionB])` block deeper in the file at line 4680 is test-only scaffolding for a keymap-loading regression test and is excluded here as test code per the Inclusion/Exclusion Matrix.)

### Related Modules

- zed

---

## BL100_ZedActionsSharedRegistry: ZedActionsSharedRegistry

**Type**: custom-command
**Trigger**: Keybinding / command palette — cross-crate shared action namespace re-exported to avoid circular crate dependencies
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/zed_actions/src/lib.rs
**Source Symbol**: OpenBrowser

### Description

Declares data-carrying `OpenBrowser { url }`, `OpenZedUrl { url }`, `ChangeKeybinding { action }` (`#[derive(Action)]`, namespace `zed`), the unit-action block `actions!(zed, [OpenSettings, OpenSettingsFile, OpenProjectSettings, OpenDefaultKeymapFile, ...])`, and further data-carrying actions `Extensions { category_filter, id }`, `AcpRegistry`, `DecreaseBufferFontSize { persist }`, `IncreaseBufferFontSize { persist }`, `OpenSettingsAt { path }`. This crate exists solely to let leaf UI crates (extensions_ui, onboarding, theme_selector, etc.) reference these shared action types without depending on `zed`/`workspace` directly.

### Related Modules

- zed_actions

---

## BL101_EncryptedPasswordAtRest: EncryptedPasswordAtRest

**Type**: integration
**Trigger**: an SSH/git askpass prompt (or similar credential flow) needs to hold a user-entered password in memory before it is injected into a spawned process's environment or an RPC response
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/askpass/src/encrypted_password.rs
**Source Symbol**: EncryptedPassword

### Description

`EncryptedPassword` wraps a password `Vec<u8>` so plaintext does not sit in memory longer than necessary. On Windows it encrypts the bytes in place with `CryptProtectMemory`/`CryptUnprotectMemory` (same-process scope); on other platforms it stores the raw bytes with no OS-level protection. `Drop` zeroizes the buffer. `decrypt()` requires an `IKnowWhatIAmDoingAndIHaveReadTheDocs` token to convert back to `String`, the only two call sites being env-var injection into a spawned child process and serialization into `proto::AskPassResponse` over RPC — both of which briefly re-expose the plaintext.

### Related Modules

- crates/askpass

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL102_DevContainerJsonParsing: DevContainerJsonParsing

**Type**: integration
**Trigger**: opening a folder with a `.devcontainer/devcontainer.json` (or a specific dev-container config) triggers parsing of the devcontainer manifest before container build/exec integration proceeds
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/dev_container/src/devcontainer_json.rs
**Source Symbol**: deserialize_devcontainer_json

### Description

Deserializes the `devcontainer.json` spec (lenient JSON with comments/trailing commas via `serde_json_lenient`) into the `DevContainer` struct: image/build/compose selection, port forwarding, mounts, features, lifecycle scripts (`onCreateCommand`, `postStartCommand`, etc.), and Zed-specific `customizations.zed.extensions`. `DevContainer::build_type()` picks Image / Dockerfile / DockerCompose / None, and `validate_devcontainer_contents()` enforces that `workspaceMount`/`workspaceFolder` are both set or both absent, and that Compose configs specify a `service`. `LifecycleScript::run()` (via `CommandRunner`) executes the parsed lifecycle commands inside the container's build/exec flow.

### Related Modules

- crates/dev_container

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL103_DevContainerManifestBuildAndRun: DevContainerManifestBuildAndRun

**Type**: integration
**Trigger**: user opens/attaches a project configured for a dev container — build image/compose stack, then run/exec inside the resulting container
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/dev_container/src/devcontainer_manifest.rs
**Source Symbol**: DevContainerManifest::build_and_run

### Description

Orchestrates the full dev-container lifecycle against a `DockerClient`: resolves base/feature images (`build_docker_image`, `build_feature_content_image`), builds or extends docker-compose files (`build_and_extend_compose_files`, `docker_compose_build`), constructs the `docker run`/`docker create_docker_run_command` invocation (mounts, ports, env, UID/GID via `Command::new("id")`), and inspects/starts the resulting container (`docker_client.inspect`, `start_container`). `spawn_dev_container()` is the crate-level entry point that reads the devcontainer configuration and drives this manifest end-to-end.

### Related Modules

- crates/dev_container

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL104_DockerCliIntegration: DockerCliIntegration

**Type**: integration
**Trigger**: any dev-container operation needing to talk to the Docker (or Podman) engine — inspect, pull, compose config/build, exec, ps, start
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/dev_container/src/docker.rs
**Source Symbol**: Docker

### Description

`Docker` (implementing `DockerClient`) shells out to the configured `docker_cli` binary (`docker` or `podman`) via `util::command::Command` for every container operation: `pull_image` (`docker pull`), `inspect` (`docker inspect --format={{json .}}`), `get_docker_compose_config`/`docker_compose_build` (`docker compose ... config|build`, with `DOCKER_BUILDKIT=1` unless podman), `run_docker_exec` (`docker exec -w <dir> -u <user> -e K=V ... sh -c "<cmd>"`), `start_container` (`docker start`), and `find_process_by_filters` (`docker ps -a --filter ...`). `Docker::new` probes `docker buildx version` at construction to detect Buildx support.

### Related Modules

- crates/dev_container

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL105_WindowsExplorerContextMenuInjector: WindowsExplorerContextMenuInjector

**Type**: integration
**Trigger**: user right-clicks a file/folder in Windows Explorer and invokes the injected "Open with Zed" shell context-menu command
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/explorer_command_injector/src/explorer_command_injector.rs
**Source Symbol**: ExplorerCommandInjector_Impl::Invoke

**Note**: Auto-detected `[SIGNAL_INFERRED]` scope — this is a COM `IExplorerCommand` shell-extension DLL, not an in-process integration, but it is the mechanism by which Explorer launches the Zed executable as an external process, so it is included per Rule C1/C3.

### Description

Implements the Windows `IExplorerCommand`/`IClassFactory` COM interfaces for a shell-extension DLL registered under `Software\Classes\Zed*ContextMenu`. `Invoke()` resolves the Zed install folder from the DLL's own module path (`GetModuleFileNameW`), then for every selected `IShellItemArray` entry spawns `Zed.exe <item_path>` via `std::process::Command::new(&zed_exe).arg(&item_path).spawn()`. `GetTitle`/`GetIcon` read the menu label from the registry and the icon from the resolved Zed executable path.

### Related Modules

- crates/explorer_command_injector

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL106_GitCliRepositoryIntegration: GitCliRepositoryIntegration

**Type**: integration
**Trigger**: any project operation that needs live git state or mutation — status, diff, commit, branch, stash, blame, log, checkpoint, hooks — routed through the working directory's git repository
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/git/src/repository.rs
**Source Symbol**: RealGitRepository

### Description

`RealGitRepository` wraps a `libgit2` (`git2::Repository`) handle for read-mostly operations plus a `GitBinary` runner (`util::command::new_command`) that shells out to the system/bundled `git` executable for everything libgit2 doesn't cover well (status, diff, log with `--format=%H%x00%P%x00%D`, stash, worktree, ref edits via `update-ref`, running commit-message/pre-commit hooks). `CommitDataReader` batches `git cat-file` graph-log requests on a background task. `edit_ref` and most mutating calls are dispatched onto the crate's `BackgroundExecutor`. `get_git_committer` resolves the bundled `git` binary path (macOS app bundle) to read `user.name`/`user.email`.

### Related Modules

- crates/git

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL107_LanguageServerProcessLifecycle: LanguageServerProcessLifecycle

**Type**: integration
**Trigger**: a worktree needs a language server for a detected language (buffer opened, language changed) — spawns the server binary, manages its stdio JSON-RPC channel, and shuts it down on cleanup/timeout
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/lsp/src/lsp.rs
**Source Symbol**: LanguageServer::new

### Description

Spawns a language-server binary as a child process (`util::command::new_command`, piped stdin/stdout/stderr, `kill_on_drop(true)`) with the working directory, arguments, and env resolved from `LanguageServerBinary`. Wires the resulting `Child`'s stdin/stdout/stderr into a JSON-RPC 2.0 read/write loop (`Content-Length:` framed messages) via `new_internal`, tracking outbound requests/notifications, response handlers, and a `SERVER_SHUTDOWN_TIMEOUT` (5s) for graceful termination. Exposes `LanguageServerBinaryOptions` (path lookup, binary download, pre-release) governing how adapters resolve the binary before this integration launches it.

### Related Modules

- crates/lsp
- crates/project (lsp_store)

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL108_LspStoreStartLanguageServer: LspStoreStartLanguageServer

**Type**: integration
**Trigger**: `LspStore` decides a worktree/language pair needs a running language server (buffer opened, settings changed, worktree trust granted) and starts (or defers until trust) the adapter's server process
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/lsp_store.rs
**Source Symbol**: LspStore::start_language_server

### Description

Resolves the language-server binary via `get_language_server_binary` (respecting `TrustedWorktrees` gating — untrusted worktrees wait on a `watch` channel before the binary is fetched/spawned), then spawns `lsp::LanguageServer::new` on a background task (or a fake/test server under `test-support`). Tracks pending workspace folders, registers the server under a `LanguageServerId`, and drives the subsequent initialize handshake/workspace-configuration exchange. This is the central integration point coordinating LSP process lifecycle with worktree trust and project settings.

### Related Modules

- crates/project
- crates/lsp

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL109_JsonLanguageServerSchemaContentExtension: JsonLanguageServerSchemaContentExtension

**Type**: integration
**Trigger**: the JSON language server (vscode-json-language-server) sends a `vscode/content` request for a `zed://schemas/action/...` URI it cannot resolve itself, or Zed needs to push a `json/schemaContent` change notification to it
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/lsp_store/json_language_server_ext.rs
**Source Symbol**: register_requests

### Description

Registers a custom LSP request handler (`SchemaContentRequest`, method `vscode/content`) on a running JSON language server so it can ask Zed for the JSON-schema content associated with a `zed://schemas/action/...` URI (avoiding sending the full schema on server startup). Looks up a globally-registered `SchemaHandlingImpl` callback to produce the schema string, and separately `notify_schemas_changed` pushes `json/schemaContent` notifications to all running JSON-schema-adapter servers when schemas are invalidated.

### Related Modules

- crates/project (lsp_store)
- crates/lsp

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL110_RustAnalyzerServerStatusExtension: RustAnalyzerServerStatusExtension

**Type**: integration
**Trigger**: a running rust-analyzer language server emits an `experimental/serverStatus` notification (health Ok/Warning/Error) during indexing or analysis
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/lsp_store/rust_analyzer_ext.rs
**Source Symbol**: register_notifications

### Description

Registers a notification handler for rust-analyzer's non-standard `experimental/serverStatus` LSP extension, mapping its `ServerHealth` (Ok/Warning/Error) plus optional message into `proto::ServerHealth` for propagation to the UI/collab layer, and logging at the corresponding level. Also defines `RUST_ANALYZER_NAME`/`CARGO_DIAGNOSTICS_SOURCE_NAME` constants used elsewhere in the LSP store to special-case this server (e.g. attributing `rustc` diagnostics).

### Related Modules

- crates/project (lsp_store)
- crates/lsp

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL111_VueLanguageServerTsServerBridge: VueLanguageServerTsServerBridge

**Type**: integration
**Trigger**: the Vue language server sends a `tsserver/request` notification that must be proxied to the co-installed TypeScript language server (vtsls/typescript-language-server), and the reply routed back
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/lsp_store/vue_language_server_ext.rs
**Source Symbol**: register_requests

### Description

Bridges the Vue language server's non-standard `tsserver/request`/`tsserver/response` notifications to whichever TypeScript server (`vtsls` or `typescript-language-server`) is also registered for the worktree: on receiving `VueServerRequest` params from `vue-language-server`, it looks up the sibling TS server by name among the `LspStore`'s running `language_server_ids`, forwards the request, and relays the response back as `TypescriptServerResponse`.

### Related Modules

- crates/project (lsp_store)
- crates/lsp

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL112_PrettierProcessLifecycle: PrettierProcessLifecycle

**Type**: integration
**Trigger**: a buffer needs formatting via Prettier — either a worktree-local Prettier install is detected, or the bundled/default Prettier is installed and started on demand
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/prettier_store.rs
**Source Symbol**: PrettierStore::start_prettier

### Description

Starts a Prettier "language server"-style process (`Prettier::start`, given a Node runtime, the resolved `prettier_dir`, and a request timeout from `ProjectSettings`) and registers it under a fresh `LanguageServerId` via `register_new_prettier`. `start_default_prettier` handles the parallel case of installing (`install_default_prettier`, spawning `npm`/node-managed install through `NodeRuntime`) and waiting on the shared installation task before starting the bundled default Prettier when no project-local install exists. `hibernate()` tears down running Prettier instances alongside the rest of the LSP store when a project goes idle.

### Related Modules

- crates/project
- crates/lsp

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL113_ProjectTerminalShellSpawn: ProjectTerminalShellSpawn

**Type**: integration
**Trigger**: user opens a new terminal panel/dock (or a remote "local terminal" breakout) for a project
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/terminals.rs
**Source Symbol**: Project::create_terminal_shell_internal

### Description

Resolves the shell program (from `TerminalSettings`, or the remote client's shell, or `get_default_system_shell`/`get_system_shell`), the working directory (active project directory, or Zed's own directory when force-local in a remote project), and the directory environment (`resolve_directory_environment`, including Python-toolchain venv activation scripts), then spawns the shell process on a background task via `ShellKind`-aware command construction. Distinguishes `create_terminal_shell` (respects remote client) from `create_local_terminal` (always local, "breaks out" of a remote/SSH project).

### Related Modules

- crates/project
- crates/terminal

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL114_ProjectTaskTerminalSpawn: ProjectTaskTerminalSpawn

**Type**: integration
**Trigger**: a configured task (from `tasks.json`/task runner) is executed, spawning its command inside a new or reused terminal
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/terminals.rs
**Source Symbol**: Project::create_terminal_task

### Description

Builds a terminal for a `SpawnInTerminal` task descriptor: resolves the working directory from the task's `cwd` (or falls back to the active/first project directory), determines whether to reveal/hide/reuse an existing terminal per `RevealStrategy`/`HideStrategy`, merges the task's `env` into the resolved shell environment, and spawns the task's `command`/`args` (or a plain shell) as the terminal's child process on a background task.

### Related Modules

- crates/project
- crates/task

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL115_HeadlessProjectRemoteJupyterKernelSpawn: HeadlessProjectRemoteJupyterKernelSpawn

**Type**: integration
**Trigger**: a remote (SSH headless) project session receives a request to start a Jupyter/IPython kernel for the REPL
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/remote_server/src/headless_project.rs
**Source Symbol**: HeadlessProject::handle_create_kernel

### Description

On the headless (remote SSH host) side, writes a Jupyter connection-info JSON (ports, HMAC key, transport) to a temp connection file, then spawns the kernel process via `smol::process::Command::new(binary)` with `{connection_file}` argument substitution — falling back through explicit `command`/`args` from the request, a detected virtualenv Python, then `python3`/`python` on `PATH`. Adjusts `PATH`/`VIRTUAL_ENV` env vars so subprocesses spawned by the kernel resolve the correct interpreter, and tracks the resulting child process lifecycle for the remote session.

### Related Modules

- crates/remote_server
- crates/repl

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL116_NativeJupyterKernelProcessLifecycle: NativeJupyterKernelProcessLifecycle

**Type**: integration
**Trigger**: user starts a REPL/notebook session locally against a discovered Jupyter kernelspec
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/repl/src/kernels/native_kernel.rs
**Source Symbol**: NativeRunningKernel::new

### Description

Peeks 5 free local TCP ports, writes a Jupyter `ConnectionInfo` (HMAC-SHA256, TCP transport) to a runtime-dir connection file via the project `Fs`, builds the kernel's `argv` command (`LocalKernelSpecification::command`, substituting `{connection_file}`) using `util::command::new_std_command`, then spawns it through `util::process::Child::spawn` with piped stdio. Establishes ZeroMQ-style iopub/control/shell/stdin socket connections (`runtimelib::create_client_*_connection`) to the spawned kernel and starts background tasks (`start_kernel_tasks`) to pump Jupyter protocol messages.

### Related Modules

- crates/repl

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL117_SystemSpecsGpuProbe: SystemSpecsGpuProbe

**Type**: integration
**Trigger**: user invokes "Copy System Specs Into Clipboard" (or any code path constructing `SystemSpecs`) on Linux/FreeBSD, needing GPU details beyond what the GPU backend already reports
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/system_specs/src/system_specs.rs
**Source Symbol**: try_determine_available_gpus

### Description

On Linux/FreeBSD, shells out to `vulkaninfo --summary` (plain `std::process::Command`, not the executor-tracked wrapper, since this runs outside an async executor) to capture a human-readable Vulkan device summary for bug-report system specs, wrapping the output in a collapsible `<details>` block or reporting failure if the binary is missing. `SystemSpecs::new`/`new_stateless` otherwise assemble OS/version/memory/architecture/commit-sha/GPU-driver info from in-process sources (`sysinfo`, `release_channel`, `gpui` window GPU specs).

### Related Modules

- crates/system_specs

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL118_CrossPlatformCommandWrapper: CrossPlatformCommandWrapper

**Type**: integration
**Trigger**: any subsystem needs to spawn an external process portably across macOS/Linux/Windows (git, LSP servers, tasks, terminals, kernels)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/util/src/command.rs
**Source Symbol**: Command::new

### Description

Provides the cross-platform `Command`/`Stdio`/`Child` abstraction used throughout the codebase to spawn processes: on macOS it re-exports the custom `darwin` implementation (posix_spawn based, see BL119); elsewhere it wraps `smol::process::Command`, adding `CREATE_NO_WINDOW` (0x08000000) on Windows to suppress console flashing. `new_command`/`new_std_command` are the top-level constructors other crates call instead of touching `std::process::Command`/`smol::process::Command` directly.

### Related Modules

- crates/util

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL119_MacosPosixSpawnCommand: MacosPosixSpawnCommand

**Type**: integration
**Trigger**: any subprocess spawn on macOS (git, LSP servers, tasks, shells) that goes through `util::command::Command`
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/util/src/command/darwin.rs
**Source Symbol**: spawn_posix_spawn

### Description

Implements process spawning on macOS via the raw `posix_spawnp` syscall (with `posix_spawnattr_setexceptionports_np` to suppress Rust panic-handler crash-reporter interception, `posix_spawn_file_actions_addchdir_np` for cwd, and fd dup2/inherit actions for stdin/stdout/stderr redirection) instead of going through `std::process::Command`/`smol::process::Command`, working around Foundation/libdispatch fork-safety issues on Apple platforms. Exposes the same `Command`/`Child`/`Stdio` surface as the generic implementation so callers are platform-agnostic.

### Related Modules

- crates/util

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL120_ChildProcessGroupLifecycle: ChildProcessGroupLifecycle

**Type**: integration
**Trigger**: any spawn path that needs guaranteed cleanup of a subprocess and all its descendants when the parent Zed process is killed (Jupyter kernels, task/terminal children)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/util/src/process.rs
**Source Symbol**: Child::spawn

### Description

Wraps `smol::process::Child`, ensuring the spawned command runs in its own process group/session (`set_pre_exec_to_start_new_session` on Unix) so `kill()` can send `SIGKILL` to the whole group via `libc::killpg` rather than leaking orphaned descendants; on Windows it currently kills only the immediate child, with a TODO to use Job Objects. Spawn failures are wrapped with a redacted command string (`redact::redact_command`) to avoid leaking secrets (e.g. passwords in argv) into error logs.

### Related Modules

- crates/util

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL121_ShellCommandLineBuilder: ShellCommandLineBuilder

**Type**: integration
**Trigger**: any integration that needs to construct a shell-invocation command line (task runner, terminal spawn, environment capture) tailored to the detected `ShellKind` (bash/zsh/fish/cmd/PowerShell/nu)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/util/src/shell_builder.rs
**Source Symbol**: ShellBuilder::build

### Description

Builds the correct `-c`/`/C` invocation and argument-quoting for the target shell so a program+args pair can be run as `shell -c "program args..."` (or `build_no_quote` for callers doing their own quoting), producing either a `smol`-compatible command (`build_smol_command`) or `std::process::Command` (`build_std_command`). Also composes a human-readable `command_label` for UI display and supports `non_interactive()`/`redirect_stdin_to_dev_null()` variants used by task execution and environment-capture integrations.

### Related Modules

- crates/util

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL122_LoginShellEnvironmentCapture: LoginShellEnvironmentCapture

**Type**: integration
**Trigger**: Zed starts up (or a terminal/task needs a fresh environment) and needs to capture the user's real login-shell environment (PATH, PYENV, NVM, etc.) that GUI-launched processes don't normally inherit
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/util/src/shell_env.rs
**Source Symbol**: capture

### Description

Spawns the user's shell as a login/interactive shell (platform-specific: `capture_unix`/`capture_windows`) with a marker command appended that dumps environment variables as JSON to a scratch file descriptor, then parses that JSON out of the shell's (possibly noisy, rc-file-polluted) stdout via `parse_env_map_from_noisy_output`. This is how `util::load_login_shell_environment` (util.rs) obtains PATH/toolchain env vars matching what a real terminal session would have, since GUI app launches on macOS/Linux bypass shell rc files.

### Related Modules

- crates/util

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL123_VimShellExecCommand: VimShellExecCommand

**Type**: integration
**Trigger**: user runs a vim `:!command`, `:read !command`, or a range/motion/text-object filter (`!!`, `!ip`, `.,.+N!`) in Vim mode
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/command.rs
**Source Symbol**: ShellExec::run

### Description

Parses vim's `:[range]!command` / `:[range]read !command` ex-command syntax (`ShellExec::parse`), expands `%`/`!` shell-escape substitutions (current filename, last shell command) via `Vim::prepare_shell_command`, then executes the resulting command through the project's terminal/task-spawning integration (`workspace.project()`), either running it standalone or filtering the selected buffer range through the external command's stdin/stdout. `shell_command_motion`/`shell_command_object` translate a motion or text object into an equivalent `.,.+N!` range command routed through the command palette.

### Related Modules

- crates/vim
- crates/project

### Related Routes

- N/A

### Related Data Models

- N/A

---

---

## BL124_UserStoreConnectionStatusObserver: UserStore Connection Status Observer

**Type**: observer
**Trigger**: `Client` connection status stream emits `Status::SignedOut` or `Status::ConnectionLost`
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/client/src/user.rs
**Source Symbol**: UserStore::new

### Description

`UserStore::new` spawns a background task (`_maintain_current_user`) that watches the client's connection status stream. On `SignedOut` it clears the current user, emits `Event::PrivateUserInfoUpdated`, and clears all contacts; on `ConnectionLost` it clears contacts and notifies. A second background task (`_maintain_contacts`) drains an internal `UpdateContacts` channel fed by RPC message handlers to keep the contacts/requests lists synced with server pushes.

### Related Modules

- client

### Related Routes

- N/A

### Related Data Models

- User
- Contact

---

## BL125_ContextServerNotificationObserver: Context Server Notification Dispatch

**Type**: observer
**Trigger**: Incoming MCP JSON-RPC notification received on the underlying `Client` connection
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/context_server/src/protocol.rs
**Source Symbol**: InitializedContextServerProtocol::on_notification

### Description

Registers a callback keyed by notification method name so callers can react to unsolicited MCP server notifications (e.g. progress or log messages) after the client/server handshake (`initialize`) completes. The callback is invoked by the inner `Client` transport whenever a matching notification arrives.

### Related Modules

- context_server

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL126_EditorInlayHintsRefresh: Editor Inlay Hints Refresh on Buffer/Settings/LSP Changes

**Type**: observer
**Trigger**: `InlayHintRefreshReason` variant fires — buffer edited, new lines scrolled into view, inlay hint settings changed, toggle action, modifier key change, LSP server removed, or explicit LSP refresh request
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/editor/src/inlays/inlay_hints.rs
**Source Symbol**: Editor::refresh_inlay_hints

### Description

Reacts to one of several external state changes (`InlayHintRefreshReason`) by deciding whether to invalidate the cached inlay hint set or only append to it, then debounces (edit vs scroll debounce) and re-queries the LSP semantics provider for the buffers/ranges affected, splicing the resulting hints into the display map.

### Related Modules

- editor

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL127_FeatureFlagStoreObserver: Feature Flag Store Change Observer

**Type**: observer
**Trigger**: `FeatureFlagStore` global is mutated (server flags updated, staff status changed)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/feature_flags/src/feature_flags.rs
**Source Symbol**: FeatureFlagAppExt::observe_flag

### Description

Subscribes a view/global observer to the `FeatureFlagStore` global; whenever the store changes (via `cx.observe_global::<FeatureFlagStore>`), the resolved flag value is recomputed and, if it differs from the last observed value, the registered callback fires. Also backs `FeatureFlag::watch`, `FeatureFlagViewExt::observe_flag`, `when_flag_enabled`, and `on_flags_ready`, which react to the same global-store mutation to re-render views or run deferred callbacks once server flags are received.

### Related Modules

- feature_flags

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL128_FsWatchStream: Filesystem Watch Event Stream

**Type**: observer
**Trigger**: OS-level filesystem change under a watched path (create/modify/remove/rescan)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/fs/src/fs.rs
**Source Symbol**: Fs::watch

### Description

Trait method every real/fake filesystem implementation exposes: given a path and a debounce latency, returns a stream of coalesced `PathEvent` batches. Callers (settings, snippets, prompts, worktree scanner, language grammar reload) subscribe to this stream to react to on-disk changes without polling.

### Related Modules

- fs

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL129_GlobalFsWatcherEventDispatch: Global Filesystem Watcher Event Dispatch

**Type**: observer
**Trigger**: `notify` crate delivers a raw OS filesystem event to the process-wide watcher
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/fs/src/fs_watcher.rs
**Source Symbol**: handle_event

### Description

Single process-wide callback registered with the `notify` watcher. Filters out `Access` events (workaround for a Linux `notify` bug), detects "rescan" (lost-sync) events and synthesizes a `PathEventKind::Rescan` event, coalesces descendant rescans under pending ancestor rescans, then fans the event out to every `WatcherRegistrationId` callback whose registered path is an ancestor/match of the changed path. `FsWatcher::add`/`remove` register and deregister per-path callbacks against this global dispatcher.

### Related Modules

- fs

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL130_WebWindowMediaQueryObserver: Web Window Appearance/DPR Media Query Observer

**Type**: observer
**Trigger**: Browser `matchMedia` "change" event for color-scheme (dark/light) or device-pixel-ratio, plus `ResizeObserver` canvas-size callback and `visibilitychange` DOM event
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/gpui_web/src/window.rs
**Source Symbol**: WebWindowInner::watch_dpr_changes

### Description

Registers a `matchMedia` listener that re-triggers itself (DPR-change media queries only fire once per resolution) and invokes the window's `appearance_changed` callback so GPUI can re-theme. Sibling closures on the same struct install the `ResizeObserver` (device-pixel content box) driving canvas resize callbacks and a `visibilitychange` listener that pauses/resumes the requestAnimationFrame render loop.

### Related Modules

- gpui_web

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL131_LspLogViewServerLogObserver: LSP Log View Server Entry Observer

**Type**: observer
**Trigger**: `LogStore` emits `Event::NewServerLogEntry` or changes its set of tracked language servers
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/language_tools/src/lsp_log_view.rs
**Source Symbol**: LspLogView::new

### Description

Subscribes to the shared `LogStore` twice: `cx.observe_in` reacts to the store's language-server set changing (e.g. current server removed) by switching the view to the first remaining server for the project; `cx.subscribe_in` reacts to `Event::NewServerLogEntry` by appending matching log/trace/RPC text to the read-only log editor, auto-scrolling and folding overly long entries when the view is currently displaying that server/kind.

### Related Modules

- language_tools

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL132_DebouncedDelayFire: Debounced Delay Trigger

**Type**: observer
**Trigger**: `DebouncedDelay::fire_new` called again before the previous delay elapsed (supersedes it) or the delay timer expires
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/debounced_delay.rs
**Source Symbol**: DebouncedDelay::fire_new

### Description

Generic debounce primitive: cancels any in-flight pending delay via a oneshot cancel channel, waits for the previous scheduled task to finish, then races a background-executor timer against a new cancellation signal before invoking the caller-supplied closure on the owning entity. Used throughout `project` to coalesce rapid-fire state changes (e.g. git status refresh, diagnostics refresh) into a single debounced action.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL133_DebugSessionBreakpointObserver: Debug Session Breakpoint Store Observer

**Type**: observer
**Trigger**: `BreakpointStore` emits `BreakpointStoreEvent::BreakpointsUpdated` or `BreakpointStoreEvent::BreakpointsCleared`
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/debugger/session.rs
**Source Symbol**: Session::new

### Description

Every debug `Session` subscribes to the project's shared `BreakpointStore` on construction. When breakpoints for a path are updated, the running session (if not ignoring breakpoints) sends the new breakpoint set to the debug adapter; when breakpoints are cleared for a set of paths, it unsets them on the adapter. `SetDebugLine`/`ClearDebugLines` events are observed but currently no-ops for this handler.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- Session
- BreakpointStore

---

## BL134_PromptTemplateOverrideWatcher: Prompt Template Override Directory Watcher

**Type**: observer
**Trigger**: Filesystem changes to the prompt-template-overrides directory (or its parent, for create/delete of the directory itself)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/prompt_store/src/prompts.rs
**Source Symbol**: PromptBuilder::watch_fs_for_template_overrides

### Description

Background-spawned loop that watches the parent directory until the override directory exists, does an initial scan registering every `.hbs` template found, then watches both the override directory and its parent simultaneously: reloading/re-registering a template with Handlebars when a `.hbs` file changes, and falling back to built-in templates (`register_built_in_templates`) if the override directory itself is deleted, before looping to re-wait for its recreation.

### Related Modules

- prompt_store

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL135_RemoteServerProjectsSettingsObserver: Remote Server Projects Settings Observer

**Type**: observer
**Trigger**: `SettingsStore` global changes, specifically the `RemoteSettings::read_ssh_config` value
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/recent_projects/src/remote_servers.rs
**Source Symbol**: RemoteServerProjects::new_inner

### Description

Observes the global `SettingsStore` and, when `read_ssh_config` toggles, either starts a background SSH-config file watch (`spawn_ssh_config_watch`) populating `ssh_config_servers`, or clears the cached SSH server list and cancels the watch task.

### Related Modules

- recent_projects

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL136_EditorconfigExternalConfigWatcher: External Editorconfig File Watcher

**Type**: observer
**Trigger**: Filesystem change to an `.editorconfig` file located outside a worktree (an ancestor directory of the worktree root)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/settings/src/editorconfig_store.rs
**Source Symbol**: EditorconfigStore::watch_local_external_config

### Description

For each ancestor directory (outside the worktree) that contains an `.editorconfig`, spawns a `watch_config_file`-backed task; on every emitted content change it recomputes which worktrees are affected by that external config path and emits `EditorconfigEvent::ExternalConfigChanged` so dependent worktrees re-resolve their effective editorconfig.

### Related Modules

- settings

### Related Routes

- N/A

### Related Data Models

- EditorconfigStore

---

## BL137_ConfigFileWatch: Config File Content Watcher

**Type**: observer
**Trigger**: Filesystem change event on a single watched config file (settings.json, global settings, keymap.json, external editorconfig, etc.)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/settings/src/settings_file.rs
**Source Symbol**: watch_config_file

### Description

Wraps `Fs::watch` on a single file path: loads the initial content, sends it on an unbounded channel, then on every subsequent filesystem event batch reloads and re-sends the file's content. `watch_config_dir` is the sibling directory-scoped variant that watches a set of specific config paths within a directory and additionally reacts to `Rescan` events by re-reading every tracked path.

### Related Modules

- settings

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL138_SettingsStoreFileWatcher: Settings Store User/Global File Watcher

**Type**: observer
**Trigger**: User settings file or global settings file changes on disk
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/settings/src/settings_store.rs
**Source Symbol**: SettingsStore::watch_settings_files

### Description

Sets up `watch_config_file`-backed streams for both the user settings file and the global settings file, merges them with `futures::stream::select`, and on each emitted content change re-parses and applies the settings (`set_user_settings`/`set_global_settings`), invokes the caller's `settings_changed` callback, and calls `cx.refresh_windows()` so UI reflects the new settings immediately. `observe_active_settings_profile_name` is the sibling in-process observer that recomputes settings values when the active settings profile global changes.

### Related Modules

- settings

### Related Routes

- N/A

### Related Data Models

- SettingsStore

---

## BL139_SnippetDirectoryWatcher: Snippet Directory Watcher

**Type**: observer
**Trigger**: Filesystem change under a watched snippets directory (global or per-project)
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/snippet_provider/src/lib.rs
**Source Symbol**: SnippetProvider::watch_directory

### Description

Spawns a task that performs an initial scan of the snippet directory then awaits the `Fs::watch` event stream; on each batch of changed paths it re-parses every changed `.json` snippet file (or removes the entry if the file no longer exists) and updates the in-memory `snippets` map keyed by language.

### Related Modules

- snippet_provider

### Related Routes

- N/A

### Related Data Models

- SnippetProvider

---

## BL140_VimMarksBufferLifecycleObserver: Vim Marks Buffer Lifecycle Observer

**Type**: observer
**Trigger**: `BufferStore` emits `BufferStoreEvent::BufferAdded`, or a watched `Buffer` emits `BufferEvent::Edited` / `BufferEvent::FileHandleChanged`, or a watched buffer entity is released
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/vim/src/state.rs
**Source Symbol**: MarksState::new

### Description

Subscribes to the workspace's `BufferStore` so newly loaded buffers get per-buffer mark tracking wired up (`on_buffer_loaded`). Each tracked buffer additionally gets its own subscription that serializes marks to the database on edit, migrates marks to a new path on `FileHandleChanged` (file rename), and drops tracking state via `cx.observe_release` when the buffer entity is dropped.

### Related Modules

- vim

### Related Routes

- N/A

### Related Data Models

- MarksState

---

## BL141_WorktreeBackgroundScannerObserver: Worktree Background Scanner Filesystem Observer

**Type**: observer
**Trigger**: Filesystem change under the worktree root reported by `Fs::watch`, or an explicit rescan/path-prefix scan request
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/worktree/src/worktree.rs
**Source Symbol**: Worktree::start_background_scanner

### Description

Spawns a background `BackgroundScanner` that watches the worktree's absolute path and drives entry/gitignore state from the resulting event stream; a second foreground task drains `ScanState` updates (`Started`, `Updated` with a new snapshot+changes, `RootUpdated`, `RootDeleted`) and applies them to the `Worktree` entity — swapping in the new snapshot, updating scanning status, relocating the root on rename, or emitting `Event::Deleted` when the root directory itself disappears.

### Related Modules

- worktree

### Related Routes

- N/A

### Related Data Models

- Worktree
- Entry

---

## BL142_GrammarSourceWatcher: Dev-Mode Grammar Source Watcher

**Type**: observer
**Trigger**: Filesystem change (`.scm` query file) under `crates/grammars/src` in a debug build
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/zed/src/main.rs
**Source Symbol**: watch_languages

### Description

Debug-only (`#[cfg(debug_assertions)]`) background task that watches the local grammar source tree (adding subdirectories explicitly since `fs.watch` isn't recursive on Linux) and calls `LanguageRegistry::reload()` whenever a changed `.scm` tree-sitter query file is detected, so language highlighting/queries hot-reload during Zed development.

### Related Modules

- zed

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL143_KeymapAndSettingsObserver: Keymap File and Settings-Driven Keymap Observer

**Type**: observer
**Trigger**: Keymap file changes on disk, or the `SettingsStore` global changes in a way that affects the base keymap (base keymap setting, vim-mode setting, helix-mode setting), or (platform-specific) the OS keyboard layout changes
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/zed/src/zed.rs
**Source Symbol**: handle_keymap_file_changes

### Description

Combines three reactive sources into keymap reload decisions: an `cx.observe_global::<SettingsStore>` callback that diffs base-keymap/vim/helix settings and signals a rebuild only on actual change; a platform `cx.on_keyboard_layout_change` callback that signals on keyboard layout/key-equivalents change; and the keymap file content stream from `watch_config_file`. Any of these triggers a keymap reparse/reload and emits `MigrationEvent::ContentChanged` for keymap migrations via `MigrationNotification`. `watch_settings_files` is the sibling function performing the analogous wiring for settings-file (not keymap) content changes.

### Related Modules

- zed

### Related Routes

- N/A

### Related Data Models

- N/A

---

---

## BL144_DebouncedDelayTimer: Debounced Delay Timer

**Type**: scheduled-job
**Trigger**: `DebouncedDelay::fire_new` invoked while a prior timer for the same debouncer is still pending — cancels the in-flight timer via a `oneshot` cancel channel and schedules a fresh `cx.background_executor().timer(delay)`, so only the last call within the debounce window actually runs its callback.
**Payload**: N/A — not a file-exchange type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/debounced_delay.rs
**Source Symbol**: DebouncedDelay::fire_new

### Description

`DebouncedDelay<E>` wraps a GPUI background-executor timer to coalesce rapid, repeated requests to run the same callback into a single delayed invocation. Each call to `fire_new` first sends on the previous call's `cancel_channel` (if any) to signal cancellation, then spawns a new `Task` that awaits `cx.background_executor().timer(delay)` racing (`select_biased!`) against the new cancel receiver — if cancelled it returns early without running the callback; if the timer fires it calls `entity.update(cx, |project, cx| func(project, cx))` and awaits the returned `Task<()>`. The spawned task also awaits any still-running `previous_task` first, ensuring executions are serialized rather than run concurrently. This is a generic reusable primitive (not itself a project-specific job) instantiated per debounced concern (see BL145).

### Related Modules

- crates/project/src/debounced_delay.rs

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL145_GitDiffDebounceRecalculation: Git Diff Debounced Recalculation

**Type**: scheduled-job
**Trigger**: `Project::request_buffer_diff_recalculation` is called when a buffer is registered (`register_buffer`) or on `BufferEvent::Edited` / `BufferEvent::Reloaded` (via `on_buffer_event`); the buffer is added to `buffers_needing_diff`, and the `git_diff_debouncer` (`DebouncedDelay<Project>`) fires `recalculate_buffer_diffs` after `ProjectSettings::git.gutter_debounce` milliseconds (floored to `MIN_DELAY` = 50ms), or immediately via `cx.defer` on the first insertion when the debounce setting is 0.
**Payload**: N/A — not a file-exchange type
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/project/src/project.rs
**Source Symbol**: Project::request_buffer_diff_recalculation

### Description

Coalesces repeated buffer-edit/reload events into a single deferred git-diff recalculation pass. `request_buffer_diff_recalculation` inserts the edited buffer's weak handle into `buffers_needing_diff`; if the configured debounce delay is 0, it schedules an immediate recalculation only on the first insertion (via `cx.defer`), otherwise it re-arms the `git_diff_debouncer` field (a `DebouncedDelay<Project>`, see BL144) with `Duration::from_millis(delay.max(50))`, whose callback invokes `recalculate_buffer_diffs`. `recalculate_buffer_diffs` then drains `buffers_needing_diff`, resolves the surviving buffer entities, and delegates to `GitStore::recalculate_buffer_diffs` in a loop until no buffers remain pending — so bursts of edits within the debounce window collapse into one git-status recomputation rather than one per keystroke.

### Related Modules

- crates/project/src/project.rs
- crates/project/src/debounced_delay.rs
- crates/project/src/git_store.rs

### Related Routes

- N/A

### Related Data Models

- Project
- Buffer

---

## BL146_LogCommandInvocation: Log Command Invocation

**Type**: queue-worker
**Trigger**: user confirms a command in the command palette (Enter)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/command_palette/src/command_palette.rs
**Source Symbol**: CommandPalette::confirm

### Description

After the selected action is invoked, spawns a detached background task that writes the command name and latest query string into the command-palette SQLite DB for later fuzzy-match ranking. Errors are logged, not surfaced to the user.

### Related Modules

- command_palette

### Related Routes

- N/A

### Related Data Models

- CommandPaletteDB

---

## BL147_SaveComponentPreviewActivePage: Save Component Preview Active Page

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open Component Preview tab
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/component_preview/src/component_preview.rs
**Source Symbol**: ComponentPreview::serialize

### Description

Persists the currently active preview page id for the item/workspace pair to the component-preview DB in the background so the tab can be restored on next launch.

### Related Modules

- component_preview

### Related Routes

- N/A

### Related Data Models

- ComponentPreviewDb

---

## BL148_McpServeConnection: Mcp Serve Connection

**Type**: queue-worker
**Trigger**: an MCP client connects to the local Unix socket created for the context-server listener
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/context_server/src/listener.rs
**Source Symbol**: McpServer::serve_connection

### Description

Spawns and detaches a background task running `handle_io`, which pumps outgoing responses to the socket writer and incoming requests from the socket reader for the lifetime of the connection.

### Related Modules

- context_server

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL149_DbWriteAndLog: Db Write And Log

**Type**: queue-worker
**Trigger**: any call site that needs a fire-and-forget SQLite write
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/db/src/db.rs
**Source Symbol**: db::write_and_log

### Description

Generic helper that spawns the given write future on the background executor and detaches it, logging any error instead of propagating it. Used throughout the codebase as the standard "persist and forget" primitive for workspace/item DB writes.

### Related Modules

- db

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL150_FetchRemoteProcessListForAttach: Fetch Remote Process List For Attach

**Type**: queue-worker
**Trigger**: user opens the debugger "Attach to Process" modal for a remote (SSH) project
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/debugger_ui/src/attach_modal.rs
**Source Symbol**: get_processes_for_project

### Description

Spawns a background request to the remote server for the list of running processes so the attach picker can populate candidates without blocking the UI thread.

### Related Modules

- debugger_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL151_ForwardSpawnedTerminalPid: Forward Spawned Terminal Pid

**Type**: queue-worker
**Trigger**: a debug session spawns an integrated terminal for a debuggee process
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/debugger_ui/src/session/running.rs
**Source Symbol**: RunningState::create_terminal

### Description

Runs a background task that awaits terminal creation and forwards the resulting PID (or error) through a channel to the DAP adapter, decoupling terminal-view construction from the debug protocol's runInTerminal response.

### Related Modules

- debugger_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL152_PersistStackFrameFilterPreference: Persist Stack Frame Filter Preference

**Type**: queue-worker
**Trigger**: user toggles "only user frames" filter in the debugger stack-frame list
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/debugger_ui/src/session/running/stack_frame_list.rs
**Source Symbol**: StackFrameList::toggle_frame_filter

### Description

Writes the chosen filter mode to the key-value store, keyed by adapter name and workspace database id, in a detached background task so the preference survives restarts.

### Related Modules

- debugger_ui

### Related Routes

- N/A

### Related Data Models

- KeyValueStore

---

## BL153_ComputeCompletionMenuFilterMatches: Compute Completion Menu Filter Matches

**Type**: queue-worker
**Trigger**: editor completion menu results need re-filtering against a query
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/editor/src/code_context_menus.rs
**Source Symbol**: CompletionsMenu::do_async_filtering

### Description

Runs fuzzy-match filtering of completion candidates against the current query on the background executor, returning a `Task<Vec<StringMatch>>` consumed by the completions menu to update displayed matches without blocking typing.

### Related Modules

- editor

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL154_SerializeEditorSelections: Serialize Editor Selections

**Type**: queue-worker
**Trigger**: editor selections change and the workspace item is eligible for restore-on-startup persistence
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/editor/src/editor.rs
**Source Symbol**: Editor::serialize_selections

### Description

Debounces via a background timer, then serializes the current selection offsets to the editor DB keyed by editor entity id and workspace id, so selections can be restored on the next session.

### Related Modules

- editor

### Related Routes

- N/A

### Related Data Models

- EditorDb

---

## BL155_SerializeEditorFolds: Serialize Editor Folds

**Type**: queue-worker
**Trigger**: editor fold state changes for a file-backed buffer
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/editor/src/items.rs
**Source Symbol**: Editor::serialize

### Description

Persists dirty-buffer contents/language and serialized-editor metadata (abs path, mtime) to the editor DB in a background task spawned from an async closure, invoked when a workspace item requests serialization on close/tab-update.

### Related Modules

- editor

### Related Routes

- N/A

### Related Data Models

- EditorDb

---

## BL156_ComputeRunnableRanges: Compute Runnable Ranges

**Type**: queue-worker
**Trigger**: editor needs to recompute runnable (test/run) code-lens ranges for the visible multi-buffer range
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/editor/src/runnables.rs
**Source Symbol**: Editor::update_runnable_ranges

### Description

Offloads the potentially expensive scan for runnable ranges within the visible buffer snapshot to the background executor and awaits the result before updating displayed run/debug gutter affordances.

### Related Modules

- editor

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL157_CompileDevExtension: Compile Dev Extension

**Type**: queue-worker
**Trigger**: user triggers rebuild of a locally-installed dev extension
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/extension_host/src/extension_host.rs
**Source Symbol**: ExtensionStore::rebuild_dev_extension

### Description

Spawns a background compile of the extension manifest/wasm via the extension builder, then a foreground task awaits the compile result to clear the in-flight `outstanding_operations` marker and notify the UI of success/failure.

### Related Modules

- extension_host

### Related Routes

- N/A

### Related Data Models

- ExtensionManifest

---

## BL158_DismissLanguageExtensionSuggestion: Dismiss Language Extension Suggestion

**Type**: queue-worker
**Trigger**: user clicks "No, don't install it" on the language-extension suggestion banner
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/extensions_ui/src/extension_suggest.rs
**Source Symbol**: extension_suggest::suggest_extension (secondary_on_click handler)

### Description

Detached background task writes a "dismissed" marker for the extension suggestion key into the key-value store so the banner is not shown again for that language.

### Related Modules

- extensions_ui

### Related Routes

- N/A

### Related Data Models

- KeyValueStore

---

## BL159_CheckRecentHistoryPathExists: Check Recent History Path Exists

**Type**: queue-worker
**Trigger**: file finder populates candidates from recent-navigation history for a local project
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/file_finder/src/file_finder.rs
**Source Symbol**: FileFinderDelegate::new

### Description

For each recent-history entry no longer tracked by a worktree, spawns a background filesystem check (`fs.is_file`) to confirm the path still exists before offering it as a match candidate.

### Related Modules

- file_finder

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL160_ReadGlobalGitCommitterIdentity: Read Global Git Committer Identity

**Type**: queue-worker
**Trigger**: git operations need the configured global committer name/email
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/git/src/repository.rs
**Source Symbol**: repository::git_committer

### Description

Runs `git config --global user.name`/`user.email` on the background executor via the git binary wrapper and returns a `GitCommitter`, logging (not failing) on missing config.

### Related Modules

- git

### Related Routes

- N/A

### Related Data Models

- GitCommitter

---

## BL161_SaveGitGraphSerialization: Save Git Graph Serialization

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open Git Graph tab
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/git_graph/src/git_graph.rs
**Source Symbol**: GitGraph::serialize

### Description

Persists the item id, workspace id and repository working-directory path for the git graph view to its DB so the graph view can be restored on relaunch.

### Related Modules

- git_graph

### Related Routes

- N/A

### Related Data Models

- GitGraphsDb

---

## BL162_CreateGitRemote: Create Git Remote

**Type**: queue-worker
**Trigger**: user confirms adding a new remote in the branch picker's create-remote flow
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/git_ui/src/branch_picker.rs
**Source Symbol**: BranchList::create_remote

### Description

Wraps the repository's remote-creation receiver future in a background task and detaches it with an error prompt handler, so failures surface as a dialog without blocking the picker UI.

### Related Modules

- git_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL163_CommitStagedChanges: Commit Staged Changes

**Type**: queue-worker
**Trigger**: user triggers commit in the git panel with staged changes present
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/git_ui/src/git_panel.rs
**Source Symbol**: GitPanel::commit_changes

### Description

Wraps the repository's commit future in a background-spawned task (repository serializes git operations internally) so the commit runs off the UI thread while the panel awaits and reports the outcome.

### Related Modules

- git_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL164_SaveProjectDiffBase: Save Project Diff Base

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open Project Diff tab
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/git_ui/src/project_diff.rs
**Source Symbol**: ProjectDiff::serialize

### Description

Persists the diff base identifier for the item/workspace pair to the project-diff DB in the background so the diff view is restored consistently on relaunch.

### Related Modules

- git_ui

### Related Routes

- N/A

### Related Data Models

- ProjectDiffDb

---

## BL165_DetachAndLogErrTask: Detach And Log Err Task

**Type**: queue-worker
**Trigger**: any call site holding a fallible `Task<Result<T, E>>` that should run to completion without blocking its caller
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/gpui/src/executor.rs
**Source Symbol**: Task::detach_and_log_err

### Description

Core GPUI primitive underlying the fire-and-forget pattern used throughout the codebase: spawns the task on the foreground executor, detaches it (letting it run independently of the caller's lifetime), and logs the tracked error location if it fails, rather than propagating the error.

### Related Modules

- gpui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL166_SaveImageViewerPath: Save Image Viewer Path

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open Image Viewer tab
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/image_viewer/src/image_viewer.rs
**Source Symbol**: ImageView::serialize

### Description

Persists the image's absolute path for the item/workspace pair to the image-viewer DB in the background so the viewer tab is restored on relaunch.

### Related Modules

- image_viewer

### Related Routes

- N/A

### Related Data Models

- ImageViewerDb

---

## BL167_PromptCliInstallOnLinux: Prompt Cli Install On Linux

**Type**: queue-worker
**Trigger**: user runs "Install CLI" on Linux/FreeBSD where symlinking must be done manually
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/install_cli/src/install_cli_binary.rs
**Source Symbol**: install_cli_binary

### Description

Spawns and detaches a background task that shows an informational prompt dialog explaining manual CLI installation steps, decoupling the prompt's await from the calling context.

### Related Modules

- install_cli

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL168_CreateJournalEntryFile: Create Journal Entry File

**Type**: queue-worker
**Trigger**: user invokes "New Journal Entry"
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/journal/src/journal.rs
**Source Symbol**: journal::new_journal_entry

### Description

Creates the journal's month directory and today's entry file on disk via a background task, returning the journal/entry paths so the caller can open or focus the corresponding workspace/editor.

### Related Modules

- journal

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL169_DebounceKeymapActionSearch: Debounce Keymap Action Search

**Type**: queue-worker
**Trigger**: user types in the keymap editor's action/keystroke search query fields
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/keymap_editor/src/keymap_editor.rs
**Source Symbol**: KeymapEditor::on_query_changed

### Description

Spawns a background task that waits out a 1-second debounce timer, then emits a telemetry event recording the completed action/keystroke search query.

### Related Modules

- keymap_editor

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL170_SendLspInitializeRequest: Send Lsp Initialize Request

**Type**: queue-worker
**Trigger**: a language server process is spawned and needs the LSP handshake performed
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/lsp/src/lsp.rs
**Source Symbol**: LanguageServer::initialize

### Description

Sends the LSP `initialize` request on the background executor and updates the server's recorded version/name from the response, returning a `Task<Result<Arc<Self>>>` the caller awaits to complete server startup.

### Related Modules

- lsp

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL171_ParseMarkdownSource: Parse Markdown Source

**Type**: queue-worker
**Trigger**: markdown source content changes and needs re-parsing
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/markdown/src/markdown.rs
**Source Symbol**: Markdown::start_background_parse

### Description

Offloads markdown parsing (links, HTML, Mermaid diagrams, heading slugs, footnotes) to the background executor so large documents don't block the render loop; result is stored as `pending_parse` and applied once ready.

### Related Modules

- markdown

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL172_SearchMarkdownPreviewSource: Search Markdown Preview Source

**Type**: queue-worker
**Trigger**: user runs an in-buffer search (Cmd-F) inside a markdown preview view
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/markdown_preview/src/markdown_preview_view.rs
**Source Symbol**: MarkdownPreviewView::find_matches

### Description

Runs the search query against the rendered markdown source text on the background executor and returns match ranges to the search UI without blocking the main thread.

### Related Modules

- markdown_preview

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL173_ExportMiniprofilerReport: Export Miniprofiler Report

**Type**: queue-worker
**Trigger**: user chooses a save path for exporting a performance profile from the miniprofiler UI
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/miniprofiler_ui/src/miniprofiler_ui.rs
**Source Symbol**: MiniProfilerView::export_profile

### Description

Awaits the save-path prompt then writes the serialized profiling JSON to disk in a detached background task, logging any I/O error instead of failing the UI action.

### Related Modules

- miniprofiler_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL174_SaveOnboardingActivePage: Save Onboarding Active Page

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open Onboarding tab
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/onboarding/src/onboarding.rs
**Source Symbol**: OnboardingPage::serialize

### Description

Persists the onboarding page's item id and workspace id to its DB in the background so the onboarding tab is restored on relaunch.

### Related Modules

- onboarding

### Related Routes

- N/A

### Related Data Models

- OnboardingPagesDb

---

## BL175_SerializeOutlinePanelState: Serialize Outline Panel State

**Type**: queue-worker
**Trigger**: outline panel's active/visible state changes and needs persisting
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/outline_panel/src/outline_panel.rs
**Source Symbol**: OutlinePanel::serialize

### Description

Serializes the panel's active flag to JSON and writes it to the key-value store under the panel's per-workspace serialization key, in a background task whose error is logged rather than propagated.

### Related Modules

- outline_panel

### Related Routes

- N/A

### Related Data Models

- KeyValueStore

---

## BL176_LoadBufferContentsOffThread: Load Buffer Contents Off Thread

**Type**: queue-worker
**Trigger**: a buffer is requested that is not yet resident in memory
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/buffer_store.rs
**Source Symbol**: BufferStore::open_buffer

### Description

Wraps the shared buffer-load task in a background-spawned task that normalizes internal error codes before returning, decoupling the potentially slow disk/LSP-backed load from the calling context's executor.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL177_ForwardBreakpointToggleToRemote: Forward Breakpoint Toggle To Remote

**Type**: queue-worker
**Trigger**: user toggles a breakpoint while the project is a remote (collab) session
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/debugger/breakpoint_store.rs
**Source Symbol**: BreakpointStore::toggle_breakpoint

### Description

Detaches a background request forwarding the breakpoint toggle to the upstream collab client, so the local UI does not wait on network round-trip before reflecting the change.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL178_PruneStaleJsDebugCompanionVersions: Prune Stale Js Debug Companion Versions

**Type**: queue-worker
**Trigger**: DapStore is constructed (local mode)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/debugger/dap_store.rs
**Source Symbol**: DapStore::new

### Description

Scans the installed js-debug-companion directory in the background and removes all but the newest semver-versioned copy, keeping the debug adapter cache from growing unbounded across upgrades.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL179_InstallLatestJsDebugCompanion: Install Latest Js Debug Companion

**Type**: queue-worker
**Trigger**: a newer js-debug-companion npm package version is detected than what is installed
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/debugger/session.rs
**Source Symbol**: session::get_or_install_js_debug_companion (install_latest_version call site)

### Description

Detaches a background install of the latest js-debug-companion package via npm while continuing to use the currently installed version for the active session, upgrading opportunistically without blocking debug startup.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL180_RestoreGitCheckpoints: Restore Git Checkpoints

**Type**: queue-worker
**Trigger**: an agent/edit-prediction flow requests restoring the working tree to a previously captured git checkpoint
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/git_store.rs
**Source Symbol**: GitStore::restore_checkpoint

### Description

Joins per-repository checkpoint-restore futures and runs them together on the background executor, returning a single `Task<Result<()>>` the caller awaits once all repositories have been reverted.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- GitStoreCheckpoint

---

## BL181_WaitForImageLoad: Wait For Image Load

**Type**: queue-worker
**Trigger**: an image path is requested that is not yet loaded into the image store
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/image_store.rs
**Source Symbol**: ImageStore::open_image

### Description

Spawns a background task that awaits the shared image-loading watch channel and maps errors, decoupling image decode/load latency from the requesting entity's executor context.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL182_RestartLanguageServersForBuffers: Restart Language Servers For Buffers

**Type**: queue-worker
**Trigger**: user or system requests restarting language servers for a set of buffers (remote project path)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/lsp_store.rs
**Source Symbol**: LspStore::restart_language_servers_for_buffers

### Description

Detaches a background request to the remote server to restart the selected language servers, logging failures, so the local UI is not blocked awaiting the round-trip.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL183_ForwardVueTsRequestToTypescriptServer: Forward Vue Ts Request To Typescript Server

**Type**: queue-worker
**Trigger**: the Vue language server issues an embedded TypeScript request that must be proxied to the real tsserver
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/lsp_store/vue_language_server_ext.rs
**Source Symbol**: vue_language_server_ext::register_requests

### Description

For each Vue-forwarded TS request, spawns a background task issuing `workspace/executeCommand` against the target TypeScript server and routes the response back, allowing multiple embedded requests to be proxied concurrently.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL184_ClearPrettierCachesOnSettingsChange: Clear Prettier Caches On Settings Change

**Type**: queue-worker
**Trigger**: prettier-related project settings change, requiring active prettier instances to reload
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/prettier_store.rs
**Source Symbol**: PrettierStore::update_prettier_settings

### Description

Spawns a background task that clears each affected prettier instance's internal cache (with the configured request timeout), so newly-edited config takes effect without a manual restart.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL185_ForwardBufferUpdateToRemoteServer: Forward Buffer Update To Remote Server

**Type**: queue-worker
**Trigger**: a remote-hosted buffer receives a local edit operation while acting as a remote server client
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/project.rs
**Source Symbol**: Project::handle_update_buffer_from_remote_server

### Description

Detaches a background request re-broadcasting the incoming buffer operation to the collab client under the project's own remote id, logging any failure rather than blocking the RPC handler.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL186_CopyWorktreeEntryOffThread: Copy Worktree Entry Off Thread

**Type**: queue-worker
**Trigger**: user copies a file/directory entry within a local worktree
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/project/src/worktree_store.rs
**Source Symbol**: WorktreeStore::copy_entry

### Description

Runs the recursive filesystem copy on the background executor, then a follow-up spawned task awaits completion and updates the destination worktree's entry state, keeping potentially slow disk I/O off the calling context.

### Related Modules

- project

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL187_DismissDevContainerSuggestion: Dismiss Dev Container Suggestion

**Type**: queue-worker
**Trigger**: user clicks "Don't Show Again" on the dev-container open suggestion banner
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/recent_projects/src/dev_container_suggest.rs
**Source Symbol**: dev_container_suggest::suggest_on_worktree_updated

### Description

Detached background task writes a "dismissed" marker for the dev-container suggestion key into the key-value store so the banner is not shown again for that worktree.

### Related Modules

- recent_projects

### Related Routes

- N/A

### Related Data Models

- KeyValueStore

---

## BL188_ForwardBufferUpdateOnHeadlessProject: Forward Buffer Update On Headless Project

**Type**: queue-worker
**Trigger**: a locally-originated buffer operation occurs inside a headless (remote_server) project
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/remote_server/src/headless_project.rs
**Source Symbol**: HeadlessProject::on_buffer_event

### Description

Detaches a background request sending the serialized buffer operation to the connected client session over the remote-server protocol, decoupling protocol I/O from the buffer-event callback.

### Related Modules

- remote_server

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL189_ReadRemoteServerStdinLoop: Read Remote Server Stdin Loop

**Type**: queue-worker
**Trigger**: the remote server process starts and needs to read incoming protocol frames from stdin
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/remote_server/src/server.rs
**Source Symbol**: remote_server::start_server

### Description

Spawns a background loop reading length-prefixed messages from the stdin stream and forwarding them over an internal mpsc channel, running independently for the life of the connection.

### Related Modules

- remote_server

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL190_RouteJupyterMessagesToChannels: Route Jupyter Messages To Channels

**Type**: queue-worker
**Trigger**: a Jupyter kernel session starts and needs its shell/control/stdin channels wired
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/repl/src/kernels/mod.rs
**Source Symbol**: kernels::start_kernel_tasks

### Description

Spawns a background routing task that reads outgoing Jupyter messages and dispatches debug/interrupt/shutdown requests to the control channel and all other requests to the shell channel, running for the life of the kernel session.

### Related Modules

- repl

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL191_RefreshPythonKernelspecs: Refresh Python Kernelspecs

**Type**: queue-worker
**Trigger**: a notebook editor needs to refresh available Python kernelspecs for its worktree
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/repl/src/notebook/notebook_ui.rs
**Source Symbol**: NotebookEditor::refresh_kernelspecs

### Description

Detaches the kernelspec-refresh task returned by the REPL store on the background executor with error logging, so kernel discovery does not block notebook rendering.

### Related Modules

- repl

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL192_InstallIpykernelPackage: Install Ipykernel Package

**Type**: queue-worker
**Trigger**: REPL execution detects the target Python environment is missing ipykernel
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/repl/src/repl_editor.rs
**Source Symbol**: repl_editor::install_ipykernel_and_assign

### Description

Runs `uv pip install` or `pip install` for ipykernel on the background executor while a toast notification informs the user, then assigns the kernel once the install completes.

### Related Modules

- repl

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL193_ComputeSettingsExactSearchMatches: Compute Settings Exact Search Matches

**Type**: queue-worker
**Trigger**: user types a query in the Settings window's search box
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/settings_ui/src/settings_ui.rs
**Source Symbol**: SettingsWindow::update_matches

### Description

Runs exact-word-match filtering over the settings search index on the background executor as one branch of the combined (exact + fuzzy) search task, keeping the settings search responsive while typing.

### Related Modules

- settings_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL194_BuildTaskContextsOffThread: Build Task Contexts Off Thread

**Type**: queue-worker
**Trigger**: the tasks UI (spawn/run) needs the current task execution context (cwd, selection, LSP task sources)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/tasks_ui/src/tasks_ui.rs
**Source Symbol**: tasks_ui::task_contexts

### Description

Assembles `TaskContexts` (active editor context, worktree paths, LSP task sources, latest selection) on the background executor so task-template variable substitution does not block the UI when many worktrees/LSPs are involved.

### Related Modules

- tasks_ui

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL195_SearchTerminalScrollback: Search Terminal Scrollback

**Type**: queue-worker
**Trigger**: user runs an in-terminal search (Cmd-F) inside an integrated terminal
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/terminal/src/terminal.rs
**Source Symbol**: Terminal::find_matches

### Description

Runs the regex search across the terminal's scrollback/grid on the background executor while holding the terminal lock, returning matched ranges without blocking terminal rendering.

### Related Modules

- terminal

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL196_SerializeTerminalPanelLayout: Serialize Terminal Panel Layout

**Type**: queue-worker
**Trigger**: the terminal panel's pane layout changes and needs persisting (debounced)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/terminal_view/src/terminal_panel.rs
**Source Symbol**: TerminalPanel::serialize

### Description

After a 50ms debounce timer, serializes the panel's pane group to JSON and writes it to the key-value store in a background task whose error is logged, keeping frequent layout changes from generating excessive DB writes.

### Related Modules

- terminal_view

### Related Routes

- N/A

### Related Data Models

- KeyValueStore

---

## BL197_SaveTerminalWorkingDirectoryAndTitle: Save Terminal Working Directory And Title

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open terminal tab whose cwd/title changed
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/terminal_view/src/terminal_view.rs
**Source Symbol**: TerminalView::serialize

### Description

Persists the terminal's working directory and custom title to the terminal DB in the background so the tab restores its cwd and title on relaunch.

### Related Modules

- terminal_view

### Related Routes

- N/A

### Related Data Models

- TerminalDb

---

## BL198_PipeVimFilterCommandStdin: Pipe Vim Filter Command Stdin

**Type**: queue-worker
**Trigger**: a Vim `:!` filter command is run against a buffer range with piped stdin input
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/vim/src/command.rs
**Source Symbol**: command::filter_range_through_shell_command (stdin-writer task)

### Description

Detaches a background task writing the buffer range's text chunks to the spawned shell process's stdin and flushing it, decoupled from awaiting the process's combined output.

### Related Modules

- vim

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL199_DeleteVimMarkFromDb: Delete Vim Mark From Db

**Type**: queue-worker
**Trigger**: user deletes a Vim mark (buffer-local or global)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/vim/src/state.rs
**Source Symbol**: VimGlobals::delete_mark

### Description

Detaches a background DB delete of the mark keyed by workspace id, path, and mark name, logging any error, so mark removal from in-memory state and persisted state stay eventually consistent without blocking the keystroke handler.

### Related Modules

- vim

### Related Routes

- N/A

### Related Data Models

- VimDb

---

## BL200_CloneItemOnSplitOffThread: Clone Item On Split Off Thread

**Type**: queue-worker
**Trigger**: user splits a pane, requiring the active item to be cloned into the new pane
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/workspace/src/item.rs
**Source Symbol**: Entity<T>::clone_on_split (ItemHandle impl)

### Description

Wraps the item's `clone_on_split` task in a background-spawned task that boxes the resulting handle, so cloning heavier item state does not block the split operation's caller.

### Related Modules

- workspace

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL201_BindSessionToWindowOnFlush: Bind Session To Window On Flush

**Type**: queue-worker
**Trigger**: multi-workspace flushes serialization for all open workspaces (e.g. on window close/quit)
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/workspace/src/multi_workspace.rs
**Source Symbol**: MultiWorkspace::flush_all_serialization

### Description

For each workspace with a database id, queues a background task binding the session id to the window id in the workspace DB, logging errors, alongside the per-workspace serialization tasks already collected.

### Related Modules

- workspace

### Related Routes

- N/A

### Related Data Models

- WorkspaceDb

---

## BL202_AwaitSpawnedTaskProcessOutput: Await Spawned Task Process Output

**Type**: queue-worker
**Trigger**: a task template is resolved and spawned in an integrated terminal, and the workspace needs to know its exit status
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/workspace/src/tasks.rs
**Source Symbol**: Workspace::schedule_resolved_task

### Description

Awaits the terminal-spawned task's process handle on the background executor to obtain its exit status, logging success/failure, decoupled from the terminal-provider's own task-spawning context.

### Related Modules

- workspace

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL203_UpdateWorkspaceActivationTimestamp: Update Workspace Activation Timestamp

**Type**: queue-worker
**Trigger**: a workspace window becomes active
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/workspace/src/workspace.rs
**Source Symbol**: Workspace::on_window_activation_changed

### Description

Detaches a background DB write updating the workspace's last-activation timestamp, keyed by database id, so recent-workspace ordering stays accurate without blocking window-activation handling.

### Related Modules

- workspace

### Related Routes

- N/A

### Related Data Models

- WorkspaceDb

---

## BL204_SaveWorkspaceWelcomePageState: Save Workspace Welcome Page State

**Type**: queue-worker
**Trigger**: workspace item serialization pass for an open Welcome Page tab
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/workspace/src/welcome.rs
**Source Symbol**: WelcomePage::serialize

### Description

Persists the welcome page's item id and workspace id (and a fixed "shown" flag) to its DB in the background so the tab's presence is restored on relaunch.

### Related Modules

- workspace

### Related Routes

- N/A

### Related Data Models

- WelcomePagesDb

---

## BL205_CreateWorktreeEntryOnDisk: Create Worktree Entry On Disk

**Type**: queue-worker
**Trigger**: user creates a new file or directory entry in the project panel/worktree
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/worktree/src/worktree.rs
**Source Symbol**: Worktree::create_entry

### Description

Runs the filesystem `create_dir`/`write` call on the background executor, keeping directory/file creation off the calling context while the worktree updates its in-memory entry tree once the write completes.

### Related Modules

- worktree

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL206_WatchThemeFileChanges: Watch Theme File Changes

**Type**: queue-worker
**Trigger**: Zed starts in an environment where the active theme file may change on disk
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/zed/src/main.rs
**Source Symbol**: zed::watch_themes

### Description

Spawns and detaches a background loop that watches the theme file path for changes, reloading and re-applying the theme registry whenever the file is rewritten, so external theme edits take effect live.

### Related Modules

- zed

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL207_RunKeymapOrSettingsMigration: Run Keymap Or Settings Migration

**Type**: queue-worker
**Trigger**: user clicks "Backup and Update" in the settings/keymap migration dialog
**Payload**: N/A
**File Schema**: N/A
**Source File**: crates/zed/src/zed/migrate.rs
**Source Symbol**: MigrationNotification::render (backup-and-migrate on_click handler)

### Description

Spawns a background task running the keymap or settings migration writer (backing up and rewriting the config file), then notifies the workspace of success/failure once the migration completes.

### Related Modules

- zed

### Related Routes

- N/A

### Related Data Models

- N/A

---

## BL208_ZedCliPathResolution: Zed CLI Path Resolution

**Type**: integration
**Trigger**: Something needs to invoke the `zed`/`zed-cli` executable from a shell context (e.g. the `zed` shell alias, a task runner, or an install/reinstall action) and must resolve a shell-safe path to the current app bundle's CLI binary
**Payload**: N/A
**File Schema**: N/A — not a file-exchange type
**Source File**: crates/util/src/util.rs
**Source Symbol**: get_zed_cli_path

### Description

Resolves the path to the `zed`/`zed-cli` binary relative to the running app bundle/executable, then shell-quotes it via `get_shell_safe_zed_path` so it can be safely embedded in a generated shell command (e.g. for the `zed` CLI shim installed by `install_cli`, or a task/terminal command that needs to re-invoke the editor). Distinct from `BL122_LoginShellEnvironmentCapture` (also in this file's crate but a different symbol/behavior: that one captures the user's login-shell environment via `shell_env.rs::capture`, this one resolves and quotes the CLI's own executable path).

### Related Modules

- crates/util

### Related Routes

- N/A

### Related Data Models

- N/A

---

## Summary

- **Total Behavior Logic Items**: 208
- **By Type**: scheduled-job: 2, queue-worker: 62, event-listener: 0, observer: 20, mail: 0, notification: 0, middleware: 0, custom-command: 100, integration: 24, webhook: 0

## Scope Notes

- Native GPUI desktop app — no HTTP request/response surface. `mail`, `middleware`, `webhook` are genuinely empty categories (no email sending, no HTTP middleware chain, no inbound/outbound HTTP webhooks in this codebase).
- `notification` (as a distinct background-dispatch worker) is empty: `crates/notifications/` exists but implements in-app collaboration notification data model/UI, not a background dispatch worker.
- `event-listener` is empty by design — see the preamble Note above.
- BL124 and BL132/BL144 both derive from `crates/project/src/debounced_delay.rs` — this file legitimately appears under both `observer` and `scheduled-job` per the scout's dual-intent allowance (a generic debounce primitive used for both external-state observation and periodic re-scan scheduling).

---

## Cross-Reference Validation

- [x] All BL### codes are unique (BL001–BL208, verified no duplicates)
- [x] All BL### codes are contiguous (no gaps 001–207)
- [x] All BL### codes are referenced in UserStories.md (type=system) or FeatureList.md — cross-checked: every BL### is mapped to at least one F### in feature-list.md (Wave 5 complete)
- [x] All BL### codes are referenced in FeatureList.md — 0 orphans as of the Wave 7a fix cycle
- [x] All related route references are valid — N/A, no routes in this profile
- [x] All related data model references are valid (MODEL### in DataModel) — spot-checked against data-model.md
- [x] No orphaned behavior logic references
- [x] All BL items have Source File + Source Symbol fields (Rule C2)
- [x] All Source File paths match scout Background Logic Source Inventory entries (Rule C2/C3)

---

## Client-Side Logic

This is a native desktop application (Rust/GPUI), not a web client — there is no browser-side JS/TS "client" distinct from the application itself. The patterns below are evaluated against the GPUI UI layer directly.

### Debounce / Throttle

BL-C01 — Git-diff recalculation debounce (see BL145_GitDiffDebounceRecalculation)
pattern: debounce
source: crates/project/src/project.rs:3645 (`Project::request_buffer_diff_recalculation`, via `DebouncedDelay::fire_new`)
trigger: buffer edited/reloaded (`BufferEvent::Edited`/`Reloaded`)
delay: `ProjectSettings::git.gutter_debounce` (floored to 50ms)
description: Coalesces bursts of buffer edits into one diff-gutter recalculation pass instead of recalculating on every keystroke.

BL-C01b — Filesystem/settings watcher debounce (see BL141_WorktreeBackgroundScanner, BL138_SettingsStoreWatchSettingsFiles)
pattern: debounce
source: crates/worktree/src/worktree.rs (`Worktree::start_background_scanner`), crates/settings/src/settings_store.rs (`SettingsStore::watch_settings_files`)
trigger: rapid successive filesystem/settings-file change events
description: Multiple observer-category BL items internally debounce/coalesce bursts of fs events before re-scanning/re-parsing — see individual BL descriptions for per-case detail.

### Optimistic UI

N/A — no optimistic UI patterns detected. This is a local-first editor operating on the local filesystem; edits apply directly to in-memory buffers with no client/server round-trip to optimistically predict.

### Polling

N/A — no `setInterval`-style recurring poll loops detected as a distinct pattern beyond the debounce-driven re-scan/refresh already captured under `scheduled-job` (BL144/BL145) and the fs/settings watchers under `observer`. This codebase favors event-driven (fs watcher / entity-event) reactivity over fixed-interval polling.

### Upload Progress

N/A — no file upload / progress-tracking UI detected (no client-server file upload surface in a local-first desktop editor).

### Realtime (WebSocket / SSE / EventSource)

N/A as a general web-client pattern — however, `crates/client`/`crates/rpc`/`crates/remote`/`crates/remote_server` implement a persistent binary RPC connection over SSH for **remote development** (not multiplayer collaboration — that subsystem was removed from this fork; see architecture.md). This is a maintained bidirectional connection with reconnect logic, analogous in *shape* to a WebSocket client but purpose-built for a remote dev-server protocol rather than realtime UI updates. Flagged here for completeness; not modeled as its own BL item since it is the RPC transport layer already covered by the `integration` BL entries for remote/headless project management.
