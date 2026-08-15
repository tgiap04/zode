<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Job List

**Project**: Zode (Zed editor fork)
**Generated**: 2026-08-07
**Analysis Scope**: Full codebase — every `behavior-logic.md` BL### entry whose Type is `scheduled-job`, `queue-worker`, or `custom-command` (164 total).

**Code Format**: All codes follow `JOB###_NameSlug` format. File-global, sequential, never reset per type.

**Scope**: One row/section per `behavior-logic.md` (`BL###`) entry whose `**Type**` is `scheduled-job`, `queue-worker`, or `custom-command`. This is a re-projection, not a re-detection — every JOB### traces back to exactly one BL###. This is a native GPUI desktop app: `custom-command` entries here are user-triggered keybinding/command-palette actions (Zode's `actions!()` dispatch mechanism), not traditional CLI batch jobs — documented as such per the shipped pipeline's type taxonomy, not re-classified.

**No `docs/jobs/` namespace**: single artifact (inventory + per-job detail in one file, per F13).

---

## Job Index

| Code | Name | BL Ref | Type | Schedule/Trigger |
|------|------|--------|------|-------------------|
| JOB001 | ActivityIndicatorStatusActions | BL001 | custom-command | User-invoked via keybinding (keymap.json) or command palette, dispatched from th |
| JOB002 | CsvPreviewOpenActions | BL002 | custom-command | User-invoked via command palette or editor tab context menu on `.csv` files; gat |
| JOB003 | InitializeDevContainerAction | BL003 | custom-command | User-invoked via command palette; no automatic schedule. |
| JOB004 | OpenDebugAdapterLogsAction | BL004 | custom-command | User-invoked via command palette (dev tools); no automatic schedule. |
| JOB005 | DebuggerSessionControlActions | BL005 | custom-command | User-invoked via keybinding (keymap.json), command palette, or debugger toolbar  |
| JOB006 | NewProcessModalTabActions | BL006 | custom-command | User-invoked via keybinding (keymap.json) while the New Process modal is open; n |
| JOB007 | BreakpointListNavigationActions | BL007 | custom-command | User-invoked via keybinding while the breakpoint list panel is focused; no autom |
| JOB008 | ConsoleWatchExpressionAction | BL008 | custom-command | User-invoked via keybinding or context menu while the debugger console is focuse |
| JOB009 | MemoryViewGoToAddressAction | BL009 | custom-command | User-invoked via keybinding or query-bar submit inside the memory view; no autom |
| JOB010 | VariableListActions | BL010 | custom-command | User-invoked via keybinding or context menu while the variable list is focused;  |
| JOB011 | DeployCurrentFileDiagnosticsAction | BL011 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB012 | ProjectDiagnosticsActions | BL012 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB013 | EditorCoreActions | BL013 | custom-command | User-invoked via keybinding (keymap.json), the editor's primary command surface; |
| JOB014 | ToggleSplitDiffAction | BL014 | custom-command | User-invoked via keybinding or toolbar toggle in a split editor; no automatic sc |
| JOB015 | EncodingSelectorToggleAction | BL015 | custom-command | User-invoked via command palette or a click on the status-bar encoding indicator |
| JOB016 | EtwTracingActions | BL016 | custom-command | User-invoked via command palette (Windows-only diagnostic tooling); no automatic |
| JOB017 | ReloadExtensionsAction | BL017 | custom-command | User-invoked via command palette; no automatic schedule. |
| JOB018 | InstallDevExtensionAction | BL018 | custom-command | User-invoked via command palette or the "Install Dev Extension" button on the Ex |
| JOB019 | FeedbackActions | BL019 | custom-command | User-invoked via command palette or the Help menu; no automatic schedule. |
| JOB020 | FileFinderActions | BL020 | custom-command | User-invoked via keybinding while the file finder modal is open; no automatic sc |
| JOB021 | GitHunkStagingActions | BL021 | custom-command | User-invoked via keybinding or the git panel context menu on a hunk or status en |
| JOB022 | GitGraphActions | BL022 | custom-command | User-invoked via keybinding or command palette from the Git Graph tab; no automa |
| JOB023 | BranchPickerActions | BL023 | custom-command | User-invoked via keybinding while the branch picker is open; no automatic schedu |
| JOB024 | CommitViewStashActions | BL024 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB025 | GitPanelActions | BL025 | custom-command | User-invoked via keybinding, or the git panel toolbar and context menu; no autom |
| JOB026 | GitPickerTabActions | BL026 | custom-command | User-invoked via keybinding while the combined git picker is open; no automatic  |
| JOB027 | ProjectDiffActions | BL027 | custom-command | User-invoked via keybinding or command palette; no automatic schedule. |
| JOB028 | StashPickerActions | BL028 | custom-command | User-invoked via keybinding while the stash picker is open; no automatic schedul |
| JOB029 | DeleteWorktreeAction | BL029 | custom-command | User-invoked via keybinding while the worktree picker is open; no automatic sche |
| JOB030 | ActionsMacroDefinition | BL030 | custom-command | N/A — compile-time declarative macro, not an invokable command; no runtime trigg |
| JOB031 | ImageViewerZoomActions | BL031 | custom-command | User-invoked via keybinding or toolbar buttons while viewing an image; no automa |
| JOB032 | DumpInputLatencyHistogramAction | BL032 | custom-command | User-invoked via command palette (dev tools); no automatic schedule. |
| JOB033 | InstallCliBinaryAction | BL033 | custom-command | User-invoked via command palette; no automatic schedule. |
| JOB034 | RegisterZedSchemeAction | BL034_RegisterZedSchemeAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB035 | NewJournalEntryAction | BL035_NewJournalEntryAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB036 | KeymapEditorActions | BL036_KeymapEditorActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB037 | KeystrokeRecordingActions | BL037_KeystrokeRecordingActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB038 | LanguageSelectorToggleAction | BL038_LanguageSelectorToggleAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB039 | HighlightsTreeViewActions | BL039_HighlightsTreeViewActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB040 | OpenKeyContextViewAction | BL040_OpenKeyContextViewAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB041 | LspToolMenuToggleAction | BL041_LspToolMenuToggleAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB042 | OpenLanguageServerLogsAction | BL042_OpenLanguageServerLogsAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB043 | SyntaxTreeViewActions | BL043_SyntaxTreeViewActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB044 | LineEndingSelectorToggleAction | BL044_LineEndingSelectorToggleAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB045 | MarkdownCopyActions | BL045_MarkdownCopyActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB046 | MarkdownPreviewScrollActions | BL046_MarkdownPreviewScrollActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB047 | MenuNavigationActions | BL047_MenuNavigationActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB048 | ToggleBaseKeymapSelectorAction | BL048_ToggleBaseKeymapSelectorAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB049 | OnboardingFlowActions | BL049_OnboardingFlowActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB050 | OutlinePanelActions | BL050_OutlinePanelActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB051 | PanelTabNavigationActions | BL051_PanelTabNavigationActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB052 | PickerConfirmActions | BL052_PickerConfirmActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB053 | WindowTabManagementActions | BL053_WindowTabManagementActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB054 | ContextServerRestartAction | BL054_ContextServerRestartAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB055 | ProjectPanelActions | BL055_ProjectPanelActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB056 | RecentProjectsMenuActions | BL056_RecentProjectsMenuActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB057 | OpenWslPathAction | BL057_OpenWslPathAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. (In this  |
| JOB058 | ReplSessionActions | BL058_ReplSessionActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB059 | ProjectSearchActions | BL059_ProjectSearchActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB060 | BufferSearchActions | BL060_BufferSearchActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB061 | SettingsEditorActions | BL061_SettingsEditorActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB062 | FocusSidebarFilterAction | BL062_FocusSidebarFilterAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB063 | SnippetsConfigActions | BL063_SnippetsConfigActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB064 | SvgOpenFollowingPreviewAction | BL064_SvgOpenFollowingPreviewAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB065 | CopySystemSpecsAction | BL065_CopySystemSpecsAction | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB066 | TabSwitcherActions | BL066_TabSwitcherActions | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB067 | TerminalCoreActions | BL067 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB068 | TerminalPanelToggleActions | BL068 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB069 | TerminalViewTextActions | BL069 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB070 | ThemeSelectorReloadAction | BL070 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB071 | ApplicationMenuActivationActions | BL071 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB072 | TitleBarProjectMenuActions | BL072 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB073 | ToolchainSelectorActions | BL073 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB074 | VimChangeListNavigationActions | BL074 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB075 | VimExCommandActions | BL075 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB076 | VimDigraphLiteralAction | BL076 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB077 | VimHelixModeActions | BL077 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB078 | VimHelixPasteAction | BL078 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB079 | VimIndentActions | BL079 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB080 | VimInsertModeActions | BL080 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB081 | VimMotionActions | BL081 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB082 | VimNormalModeInsertActions | BL082 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB083 | VimIncrementDecrementActions | BL083 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB084 | VimPasteAction | BL084 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB085 | VimRepeatMacroActions | BL085 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB086 | VimScrollActions | BL086 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB087 | VimSearchActions | BL087 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB088 | VimSubstituteActions | BL088 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB089 | VimTextObjectActions | BL089 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB090 | VimReplaceModeActions | BL090 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB091 | VimRewrapAction | BL091 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB092 | VimOperatorPendingPushActions | BL092 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB093 | VimVisualModeActions | BL093 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB094 | MultiWorkspaceSidebarActions | BL094 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB095 | PaneItemManagementActions | BL095 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB096 | OpenThemePreviewAction | BL096 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB097 | WelcomeScreenActions | BL097 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB098 | WorkspacePaneNavigationActions | BL098 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB099 | ZedApplicationWindowActions | BL099 | custom-command | User-invoked via command palette, keybinding, or native application menu; no aut |
| JOB100 | ZedActionsSharedRegistry | BL100 | custom-command | User-invoked via command palette or keybinding; no automatic schedule. |
| JOB101 | Debounced Delay Timer | BL144 | scheduled-job | Re-armed on every `fire_new` call with a caller-supplied `Duration`; previous pe |
| JOB102 | Git Diff Debounced Recalculation | BL145 | scheduled-job | Debounced via the `git_diff_debouncer` (`DebouncedDelay<Project>`, JOB101) using |
| JOB103 | Log Command Invocation | BL146 | queue-worker | Dispatched via a detached background task from `CommandPalette::confirm` immedia |
| JOB104 | Save Component Preview Active Page | BL147 | queue-worker | Dispatched via `cx.background_spawn` when the workspace's item-serialization pas |
| JOB105 | Mcp Serve Connection | BL148 | queue-worker | Dispatched and detached for the duration of the connection each time an MCP clie |
| JOB106 | Db Write And Log | BL149 | queue-worker | Dispatched via `cx.background_spawn` whenever a call site invokes `db::write_and |
| JOB107 | Fetch Remote Process List For Attach | BL150 | queue-worker | Dispatched as a background task when the user opens the Attach-to-Process modal  |
| JOB108 | Forward Spawned Terminal Pid | BL151 | queue-worker | Dispatched as a background task when a debug session spawns an integrated termin |
| JOB109 | Persist Stack Frame Filter Preference | BL152 | queue-worker | Dispatched as a detached background task each time the user toggles the stack-fr |
| JOB110 | Compute Completion Menu Filter Matches | BL153 | queue-worker | Dispatched via `cx.background_spawn` each time the editor's completion menu need |
| JOB111 | Serialize Editor Selections | BL154 | queue-worker | Debounced via a background timer, re-armed on each selection change for editors  |
| JOB112 | Serialize Editor Folds | BL155 | queue-worker | Dispatched as a background task from an async closure when a workspace item requ |
| JOB113 | Compute Runnable Ranges | BL156 | queue-worker | Dispatched via `cx.spawn_in` from `refresh_runnables` whenever the visible multi |
| JOB114 | Compile Dev Extension | BL157 | queue-worker | Dispatched when the user triggers a rebuild of a locally-installed dev extension |
| JOB115 | Dismiss Language Extension Suggestion | BL158 | queue-worker | Dispatched as a detached background task from the banner's `secondary_on_click`  |
| JOB116 | Check Recent History Path Exists | BL159 | queue-worker | Dispatched per untracked recent-history entry as a background filesystem check ( |
| JOB117 | Read Global Git Committer Identity | BL160 | queue-worker | Dispatched on the background executor whenever a call site needs the committer i |
| JOB118 | Save Git Graph Serialization | BL161 | queue-worker | Dispatched in the background when the workspace's item-serialization pass reache |
| JOB119 | Create Git Remote | BL162 | queue-worker | Dispatched and detached when the user confirms adding a new remote in the branch |
| JOB120 | Commit Staged Changes | BL163 | queue-worker | Dispatched as a background-spawned task when the user triggers commit in the git |
| JOB121 | Save Project Diff Base | BL164 | queue-worker | Dispatched in the background when the workspace's item-serialization pass reache |
| JOB122 | Detach And Log Err Task | BL165 | queue-worker | Dispatched on the foreground executor whenever a call site holding a `Task<Resul |
| JOB123 | Save Image Viewer Path | BL166 | queue-worker | Dispatched in the background when the workspace's item-serialization pass reache |
| JOB124 | Prompt Cli Install On Linux | BL167 | queue-worker | Dispatched and detached when the user runs "Install CLI" on a platform requiring |
| JOB125 | Create Journal Entry File | BL168 | queue-worker | Dispatched as a background task when the user invokes "New Journal Entry". |
| JOB126 | Debounce Keymap Action Search | BL169 | queue-worker | Dispatched as a background task on each query change in the keymap editor's sear |
| JOB127 | Send Lsp Initialize Request | BL170 | queue-worker | Dispatched on the background executor immediately after a language server proces |
| JOB128 | Parse Markdown Source | BL171 | queue-worker | Dispatched via `start_background_parse` whenever the markdown source content cha |
| JOB129 | Search Markdown Preview Source | BL172 | queue-worker | Dispatched on the background executor each time the user runs an in-buffer searc |
| JOB130 | Export Miniprofiler Report | BL173 | queue-worker | Dispatched as a detached background task after the user picks a save path in the |
| JOB131 | Save Onboarding Active Page | BL174 | queue-worker | Dispatched in the background when the workspace's item-serialization pass reache |
| JOB132 | Serialize Outline Panel State | BL175 | queue-worker | Dispatched as a background task whenever the outline panel's active/visible stat |
| JOB133 | Load Buffer Contents Off Thread | BL176 | queue-worker | Dispatched via `cx.background_spawn` whenever a buffer is requested (`BufferStor |
| JOB134 | Forward Breakpoint Toggle To Remote | BL177 | queue-worker | Dispatched from `BreakpointStore::toggle_breakpoint` (`crates/project/src/debugg |
| JOB135 | Prune Stale Js Debug Companion Versions | BL178 | queue-worker | Dispatched via `cx.background_spawn` inside `DapStore::new` (`crates/project/src |
| JOB136 | Install Latest Js Debug Companion | BL179 | queue-worker | Dispatched via `cx.background_spawn(install_latest_version(...))` (`crates/proje |
| JOB137 | Restore Git Checkpoints | BL180 | queue-worker | Dispatched via the background executor from `GitStore::restore_checkpoint` (`cra |
| JOB138 | Wait For Image Load | BL181 | queue-worker | Dispatched via `cx.background_spawn` from `ImageStore::open_image` (`crates/proj |
| JOB139 | Restart Language Servers For Buffers | BL182 | queue-worker | Dispatched from `LspStore::restart_language_servers_for_buffers` (`crates/projec |
| JOB140 | Forward Vue Ts Request To Typescript Server | BL183 | queue-worker | Dispatched via `cx.background_spawn` inside `register_requests` (`crates/project |
| JOB141 | Clear Prettier Caches On Settings Change | BL184 | queue-worker | Dispatched via the background executor from `PrettierStore::update_prettier_sett |
| JOB142 | Forward Buffer Update To Remote Server | BL185 | queue-worker | Dispatched via a detached background request from `Project::handle_update_buffer |
| JOB143 | Copy Worktree Entry Off Thread | BL186 | queue-worker | Dispatched via the background executor from `WorktreeStore::copy_entry` (`crates |
| JOB144 | Dismiss Dev Container Suggestion | BL187 | queue-worker | Dispatched via `cx.background_spawn` inside `suggest_on_worktree_updated` (`crat |
| JOB145 | Forward Buffer Update On Headless Project | BL188 | queue-worker | Dispatched via a detached background request from `HeadlessProject::on_buffer_ev |
| JOB146 | Read Remote Server Stdin Loop | BL189 | queue-worker | Dispatched via `cx.background_spawn` inside `start_server` (`crates/remote_serve |
| JOB147 | Route Jupyter Messages To Channels | BL190 | queue-worker | Dispatched via the background executor from `start_kernel_tasks` (`crates/repl/s |
| JOB148 | Refresh Python Kernelspecs | BL191 | queue-worker | Dispatched via `NotebookEditor::refresh_kernelspecs` (`crates/repl/src/notebook/ |
| JOB149 | Install Ipykernel Package | BL192 | queue-worker | Dispatched via the background executor from `install_ipykernel_and_assign` (`cra |
| JOB150 | Compute Settings Exact Search Matches | BL193 | queue-worker | Dispatched via the background executor from `SettingsWindow::update_matches` (`c |
| JOB151 | Build Task Contexts Off Thread | BL194 | queue-worker | Dispatched via the background executor from `task_contexts` (`crates/tasks_ui/sr |
| JOB152 | Search Terminal Scrollback | BL195 | queue-worker | Dispatched via the background executor from `Terminal::find_matches` (`crates/te |
| JOB153 | Serialize Terminal Panel Layout | BL196 | queue-worker | Dispatched via a background task from `TerminalPanel::serialize` (`crates/termin |
| JOB154 | Save Terminal Working Directory And Title | BL197 | queue-worker | Dispatched via the background executor from `TerminalView::serialize` (`crates/t |
| JOB155 | Pipe Vim Filter Command Stdin | BL198 | queue-worker | Dispatched via `cx.background_spawn` inside the `:!` filter-command handler (`cr |
| JOB156 | Delete Vim Mark From Db | BL199 | queue-worker | Dispatched via a detached background DB delete from `VimGlobals::delete_mark` (` |
| JOB157 | Clone Item On Split Off Thread | BL200 | queue-worker | Dispatched via a background-spawned wrapper around the `ItemHandle::clone_on_spl |
| JOB158 | Bind Session To Window On Flush | BL201 | queue-worker | Dispatched as queued background tasks from `MultiWorkspace::flush_all_serializat |
| JOB159 | Await Spawned Task Process Output | BL202 | queue-worker | Dispatched via the background executor from `Workspace::schedule_resolved_task`  |
| JOB160 | Update Workspace Activation Timestamp | BL203 | queue-worker | Dispatched via a detached background DB write from `Workspace::on_window_activat |
| JOB161 | Save Workspace Welcome Page State | BL204 | queue-worker | Dispatched via the background executor from `WelcomePage::serialize` (`crates/wo |
| JOB162 | Create Worktree Entry On Disk | BL205 | queue-worker | Dispatched via the background executor from `Worktree::create_entry` (`crates/wo |
| JOB163 | Watch Theme File Changes | BL206 | queue-worker | Dispatched and detached as a background loop from `watch_themes` (`crates/zed/sr |
| JOB164 | Run Keymap Or Settings Migration | BL207 | queue-worker | Dispatched via `cx.background_spawn(write_keymap_migration(...))` / `write_setti |

---

## JOB001_ActivityIndicatorStatusActions: ActivityIndicatorStatusActions

**BL Ref**: BL001
**Type**: custom-command
**Source**: `crates/activity_indicator/src/activity_indicator.rs:37`

### Purpose

Lets the user surface or dismiss the last language-server error reported by the status-bar activity indicator, without opening a separate log view.

### Schedule / Trigger

User-invoked via keybinding (keymap.json) or command palette, dispatched from the status-bar activity indicator; no automatic schedule.

### Data Touched

N/A — no persisted data touched; reads in-memory last-error state from the language-server activity tracker.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB002_CsvPreviewOpenActions: CsvPreviewOpenActions

**BL Ref**: BL002
**Type**: custom-command
**Source**: `crates/csv_preview/src/csv_preview.rs:26`

### Purpose

Opens a tabular-data preview of the active CSV buffer, either in place or split to the side, so the user can inspect structured CSV content instead of raw text.

### Schedule / Trigger

User-invoked via command palette or editor tab context menu on `.csv` files; gated behind the `tabular-data-preview` feature flag; no automatic schedule.

### Data Touched

N/A — no persisted data touched; reads the active Editor buffer's in-memory content.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB003_InitializeDevContainerAction: InitializeDevContainerAction

**BL Ref**: BL003
**Type**: custom-command
**Source**: `crates/dev_container/src/lib.rs:154`

### Purpose

Opens the Dev Container modal so the user can scaffold or initialize a `.devcontainer` configuration for the active or a new workspace.

### Schedule / Trigger

User-invoked via command palette; no automatic schedule.

### Data Touched

N/A — no persisted data touched by the action itself; opens a modal that may later write `.devcontainer/devcontainer.json` to the workspace filesystem.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB004_OpenDebugAdapterLogsAction: OpenDebugAdapterLogsAction

**BL Ref**: BL004
**Type**: custom-command
**Source**: `crates/debugger_tools/src/dap_log.rs:954`

### Purpose

Opens the Debug Adapter Protocol log viewer so developers can inspect DAP traffic for all active debug sessions.

### Schedule / Trigger

User-invoked via command palette (dev tools); no automatic schedule.

### Data Touched

N/A — no persisted data touched; reads the in-memory `LogStore` observing active debug sessions.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB005_DebuggerSessionControlActions: DebuggerSessionControlActions

**BL Ref**: BL005
**Type**: custom-command
**Source**: `crates/debugger_ui/src/debugger_ui.rs:32`

### Purpose

Provides the full set of debugger session controls — start, step, pause, restart, stop, focus-panel, and toggle commands — so the user can drive an active debug session from keybindings, the command palette, or the debugger toolbar.

### Schedule / Trigger

User-invoked via keybinding (keymap.json), command palette, or debugger toolbar buttons; no automatic schedule.

### Data Touched

- DebugSession — read/write (session state transitions such as continue/pause/stop/restart)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action; underlying DAP request failures are surfaced through the debug session's own error reporting, not a job retry mechanism).

---

## JOB006_NewProcessModalTabActions: NewProcessModalTabActions

**BL Ref**: BL006
**Type**: custom-command
**Source**: `crates/debugger_ui/src/new_process_modal.rs:42`

### Purpose

Switches between the Task/Debug/Attach/Launch tabs inside the modal used to start a new debug session or task process.

### Schedule / Trigger

User-invoked via keybinding (keymap.json) while the New Process modal is open; no automatic schedule.

### Data Touched

N/A — no persisted data touched; toggles in-memory modal tab-selection state.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB007_BreakpointListNavigationActions: BreakpointListNavigationActions

**BL Ref**: BL007
**Type**: custom-command
**Source**: `crates/debugger_ui/src/session/running/breakpoint_list.rs:38`

### Purpose

Moves focus between the editable properties (condition, hit count, log message) of the selected breakpoint entry in the breakpoint list panel.

### Schedule / Trigger

User-invoked via keybinding while the breakpoint list panel is focused; no automatic schedule.

### Data Touched

N/A — no persisted data touched; moves in-memory UI focus within the breakpoint list.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB008_ConsoleWatchExpressionAction: ConsoleWatchExpressionAction

**BL Ref**: BL008
**Type**: custom-command
**Source**: `crates/debugger_ui/src/session/running/console.rs:38`

### Purpose

Adds the currently selected or typed expression in the debug console to the session's watch list for continuous evaluation.

### Schedule / Trigger

User-invoked via keybinding or context menu while the debugger console is focused; no automatic schedule.

### Data Touched

- DebugSession — write (appends to the session's in-memory watch-expression list)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB009_MemoryViewGoToAddressAction: MemoryViewGoToAddressAction

**BL Ref**: BL009
**Type**: custom-command
**Source**: `crates/debugger_ui/src/session/running/memory_view.rs:29`

### Purpose

Navigates the debugger's memory inspection view to the address entered by the user in the view's query bar.

### Schedule / Trigger

User-invoked via keybinding or query-bar submit inside the memory view; no automatic schedule.

### Data Touched

N/A — no persisted data touched; requests a live memory read from the attached debug adapter at the given address.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); a failed memory read is surfaced by the debug adapter's own response, not retried by this action.

---

## JOB010_VariableListActions: VariableListActions

**BL Ref**: BL010
**Type**: custom-command
**Source**: `crates/debugger_ui/src/session/running/variable_list.rs:30`

### Purpose

Drives expand/collapse of the debug variable tree plus copy, edit, and add-to-watch operations on the currently selected variable entry.

### Schedule / Trigger

User-invoked via keybinding or context menu while the variable list is focused; no automatic schedule.

### Data Touched

- DebugSession — read/write (variable tree expansion state, watch list, and edited variable values sent to the debug adapter)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB011_DeployCurrentFileDiagnosticsAction: DeployCurrentFileDiagnosticsAction

**BL Ref**: BL011
**Type**: custom-command
**Source**: `crates/diagnostics/src/buffer_diagnostics.rs:41`

### Purpose

Opens a buffer-scoped diagnostics editor limited to excerpts around diagnostics for the currently focused file, letting the user review file-level errors/warnings without the full project diagnostics view.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Project — read (diagnostic entries scoped to the active buffer)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB012_ProjectDiagnosticsActions: ProjectDiagnosticsActions

**BL Ref**: BL012
**Type**: custom-command
**Source**: `crates/diagnostics/src/diagnostics.rs:57`

### Purpose

Opens the project-wide diagnostics view and toggles whether warning-level diagnostics and automatic refresh are displayed.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched; reads project-wide diagnostic state held in memory and toggles in-memory view options.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB013_EditorCoreActions: EditorCoreActions

**BL Ref**: BL013
**Type**: custom-command
**Source**: `crates/editor/src/actions.rs:12`

### Purpose

Implements the editor's primary keybinding-driven command surface — cursor movement, selection extension, and text-editing operations dispatched dozens of times per editing session.

### Schedule / Trigger

User-invoked via keybinding (keymap.json), the editor's primary command surface; no automatic schedule.

### Data Touched

- Editor — write (cursor position, selection ranges, buffer content depending on the specific action)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB014_ToggleSplitDiffAction: ToggleSplitDiffAction

**BL Ref**: BL014
**Type**: custom-command
**Source**: `crates/editor/src/split.rs:402`

### Purpose

Toggles the diff-view style shown between the two panes of a split editor.

### Schedule / Trigger

User-invoked via keybinding or toolbar toggle in a split editor; no automatic schedule.

### Data Touched

- Editor — write (in-memory split-diff display mode)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB015_EncodingSelectorToggleAction: EncodingSelectorToggleAction

**BL Ref**: BL015
**Type**: custom-command
**Source**: `crates/encoding_selector/src/encoding_selector.rs:14`

### Purpose

Opens the modal picker that lets the user change the active buffer's text encoding.

### Schedule / Trigger

User-invoked via command palette or a click on the status-bar encoding indicator; no automatic schedule.

### Data Touched

N/A — no persisted data touched by opening the picker; a confirmed selection re-encodes the active buffer in memory.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB016_EtwTracingActions: EtwTracingActions

**BL Ref**: BL016
**Type**: custom-command
**Source**: `crates/etw_tracing/etw_tracing.rs:21`

### Purpose

Starts, stops, saves, or cancels Event Tracing for Windows (ETW) performance traces, optionally including heap tracing, for Windows-only performance diagnostics.

### Schedule / Trigger

User-invoked via command palette (Windows-only diagnostic tooling); no automatic schedule.

### Data Touched

N/A — no persisted data touched by the trigger itself; `SaveEtwTrace` writes the captured trace to a user-selected file on disk.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB017_ReloadExtensionsAction: ReloadExtensionsAction

**BL Ref**: BL017
**Type**: custom-command
**Source**: `crates/extension_host/src/extension_host.rs:192`

### Purpose

Reloads all installed extensions from disk without requiring a full application restart.

### Schedule / Trigger

User-invoked via command palette; no automatic schedule.

### Data Touched

N/A — no persisted data touched; reloads extension state held in memory from the on-disk extension directory.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB018_InstallDevExtensionAction: InstallDevExtensionAction

**BL Ref**: BL018
**Type**: custom-command
**Source**: `crates/extensions_ui/src/extensions_ui.rs:48`

### Purpose

Prompts the user for a local directory and installs it as a development extension for local iteration.

### Schedule / Trigger

User-invoked via command palette or the "Install Dev Extension" button on the Extensions page; no automatic schedule.

### Data Touched

N/A — no persisted data touched beyond registering the extension's local directory path with the extension host in memory.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB019_FeedbackActions: FeedbackActions

**BL Ref**: BL019
**Type**: custom-command
**Source**: `crates/feedback/src/feedback.rs:12`

### Purpose

Opens the project's GitHub repository in the browser and copies the list of installed extensions to the clipboard so users can attach it to bug reports.

### Schedule / Trigger

User-invoked via command palette or the Help menu; no automatic schedule.

### Data Touched

N/A — no persisted data touched; reads the in-memory installed-extensions list for clipboard copy.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB020_FileFinderActions: FileFinderActions

**BL Ref**: BL020
**Type**: custom-command
**Source**: `crates/file_finder/src/file_finder.rs:56`

### Purpose

Navigates results within the file finder modal and toggles its filter and split option menus.

### Schedule / Trigger

User-invoked via keybinding while the file finder modal is open; no automatic schedule.

### Data Touched

N/A — no persisted data touched; moves in-memory selection/menu state within the file finder modal.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB021_GitHunkStagingActions: GitHunkStagingActions

**BL Ref**: BL021
**Type**: custom-command
**Source**: `crates/git/src/git.rs:33`

### Purpose

Drives per-hunk staging, unstaging, and restoring of file changes, plus branch renaming and discarding local file changes, from the git panel.

### Schedule / Trigger

User-invoked via keybinding or the git panel context menu on a hunk or status entry; no automatic schedule.

### Data Touched

- Oid — read (commit/blob identifiers referenced when staging/restoring against a given git object)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); underlying git CLI failures surface through the git integration's own error reporting, not a job retry mechanism.

---

## JOB022_GitGraphActions: GitGraphActions

**BL Ref**: BL022
**Type**: custom-command
**Source**: `crates/git_graph/src/git_graph.rs:278`

### Purpose

Opens the commit-detail view for the selected node in the Git Graph and focuses the graph's search field.

### Schedule / Trigger

User-invoked via keybinding or command palette from the Git Graph tab; no automatic schedule.

### Data Touched

N/A — no persisted data touched; reads in-memory git graph node data to populate the commit view.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB023_BranchPickerActions: BranchPickerActions

**BL Ref**: BL023
**Type**: custom-command
**Source**: `crates/git_ui/src/branch_picker.rs:31`

### Purpose

Deletes the selected local or remote branch and toggles whether remote branches are filtered from the branch picker list.

### Schedule / Trigger

User-invoked via keybinding while the branch picker is open; no automatic schedule.

### Data Touched

N/A — no persisted app data touched; `DeleteBranch` mutates the on-disk git repository's branch refs via the git integration.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); underlying git CLI failures surface through the git integration's own error reporting.

---

## JOB024_CommitViewStashActions: CommitViewStashActions

**BL Ref**: BL024
**Type**: custom-command
**Source**: `crates/git_ui/src/commit_view.rs:45`

### Purpose

Applies, pops, or drops the currently selected stash entry from the commit view.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted app data touched; mutates the on-disk git stash via the git integration.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); underlying git CLI failures surface through the git integration's own error reporting.

---

## JOB025_GitPanelActions: GitPanelActions

**BL Ref**: BL025
**Type**: custom-command
**Source**: `crates/git_ui/src/git_panel.rs:82`

### Purpose

Controls the git panel's visibility, focus, and menu, and opens the Git Graph tab positioned at a specific commit.

### Schedule / Trigger

User-invoked via keybinding, or the git panel toolbar and context menu; no automatic schedule.

### Data Touched

N/A — no persisted data touched; toggles in-memory panel visibility/focus state.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB026_GitPickerTabActions: GitPickerTabActions

**BL Ref**: BL026
**Type**: custom-command
**Source**: `crates/git_ui/src/git_picker.rs:18`

### Purpose

Switches between the Branches and Stash tabs of the combined git picker.

### Schedule / Trigger

User-invoked via keybinding while the combined git picker is open; no automatic schedule.

### Data Touched

N/A — no persisted data touched; toggles in-memory tab-selection state.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB027_ProjectDiffActions: ProjectDiffActions

**BL Ref**: BL027
**Type**: custom-command
**Source**: `crates/git_ui/src/project_diff.rs:7`

### Purpose

Opens the working-directory diff view, stages files, and diffs the working tree against the default branch.

### Schedule / Trigger

User-invoked via keybinding or command palette; no automatic schedule.

### Data Touched

- Project — read (working-directory diff computed against the default branch)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); underlying git CLI failures surface through the git integration's own error reporting.

---

## JOB028_StashPickerActions: StashPickerActions

**BL Ref**: BL028
**Type**: custom-command
**Source**: `crates/git_ui/src/stash_picker.rs:26`

### Purpose

Drops or shows the diff of the currently selected stash entry from the stash picker.

### Schedule / Trigger

User-invoked via keybinding while the stash picker is open; no automatic schedule.

### Data Touched

N/A — no persisted app data touched; `DropStashItem` mutates the on-disk git stash via the git integration.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); underlying git CLI failures surface through the git integration's own error reporting.

---

## JOB029_DeleteWorktreeAction: DeleteWorktreeAction

**BL Ref**: BL029
**Type**: custom-command
**Source**: `crates/git_ui/src/worktree_picker.rs:31`

### Purpose

Removes the selected git worktree from disk and detaches it from the project.

### Schedule / Trigger

User-invoked via keybinding while the worktree picker is open; no automatic schedule.

### Data Touched

- Project — write (removes the worktree from the project's tracked worktree list; also deletes the worktree directory on disk)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); underlying git CLI/filesystem failures surface through the git integration's own error reporting.

---

## JOB030_ActionsMacroDefinition: ActionsMacroDefinition

**BL Ref**: BL030
**Type**: custom-command
**Source**: `crates/gpui/src/action.rs:11`

### Purpose

Defines the shared `actions!` macro and `Action` derive machinery that every other custom-command entry in this shard depends on; it is the infrastructure the actions are built with, not an invokable command itself.

### Schedule / Trigger

N/A — compile-time declarative macro, not an invokable command; no runtime trigger or schedule applies.

### Data Touched

N/A — no persisted data touched; a compile-time macro definition, not a runtime data path.

### Failure / Retry Behavior

N/A — no retry policy found in source (compile-time macro, not a runtime operation subject to retry).

---

## JOB031_ImageViewerZoomActions: ImageViewerZoomActions

**BL Ref**: BL031
**Type**: custom-command
**Source**: `crates/image_viewer/src/image_viewer.rs:37`

### Purpose

Controls the zoom level of the built-in image preview — zoom in/out, reset, fit-to-view, and zoom-to-actual-size.

### Schedule / Trigger

User-invoked via keybinding or toolbar buttons while viewing an image; no automatic schedule.

### Data Touched

N/A — no persisted data touched; adjusts in-memory zoom/view state of the image viewer.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB032_DumpInputLatencyHistogramAction: DumpInputLatencyHistogramAction

**BL Ref**: BL032
**Type**: custom-command
**Source**: `crates/input_latency_ui/src/input_latency_ui.rs:8`

### Purpose

Writes a formatted input-to-frame latency histogram report for the current window, diffed against the previous report when one exists, to help diagnose input responsiveness regressions.

### Schedule / Trigger

User-invoked via command palette (dev tools); no automatic schedule.

### Data Touched

N/A — no persisted app data touched; writes a diagnostic report file to disk (path determined by the dev-tools reporting convention) but touches no application entity/table.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB033_InstallCliBinaryAction: InstallCliBinaryAction

**BL Ref**: BL033
**Type**: custom-command
**Source**: `crates/install_cli/src/install_cli_binary.rs:15`

### Purpose

Symlinks the Zode CLI auxiliary executable into `/usr/local/bin/zed`, skipping the operation if the existing symlink is already up to date.

### Schedule / Trigger

User-invoked via command palette; no automatic schedule.

### Data Touched

N/A — no persisted app data touched; creates/updates a filesystem symlink at `/usr/local/bin/zed`, not an application entity.

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); a failed symlink write (e.g. permission denied) surfaces as an error to the user, not retried.

---

## JOB034_RegisterZedSchemeAction: RegisterZedSchemeAction

**BL Ref**: BL034
**Type**: custom-command
**Source**: `crates/install_cli/src/register_zed_scheme.rs:8`

### Purpose

Registers the `zed://` URL scheme with the host OS so external links and CLI invocations can hand off to the running app instance.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB035_NewJournalEntryAction: NewJournalEntryAction

**BL Ref**: BL035
**Type**: custom-command
**Source**: `crates/journal/src/journal.rs:49`

### Purpose

Creates today's journal entry file (or appends to it if already present) at the user-configured journal directory and opens it in the editor.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Journal entry file (filesystem) — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB036_KeymapEditorActions: KeymapEditorActions

**BL Ref**: BL036
**Type**: custom-command
**Source**: `crates/keymap_editor/src/keymap_editor.rs:64`

### Purpose

Drives creation, editing, deletion, and copying of keybindings and their context predicates from within the keymap editor UI.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- User keymap file (filesystem) — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB037_KeystrokeRecordingActions: KeystrokeRecordingActions

**BL Ref**: BL037
**Type**: custom-command
**Source**: `crates/keymap_editor/src/ui_components/keystroke_input.rs:14`

### Purpose

Starts, stops, and clears live keystroke capture in the keybinding-definition widget so a user can record a new key chord by pressing it.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB038_LanguageSelectorToggleAction: LanguageSelectorToggleAction

**BL Ref**: BL038
**Type**: custom-command
**Source**: `crates/language_selector/src/language_selector.rs:21`

### Purpose

Opens the modal picker that lets a user reassign the syntax-highlighting/language grammar associated with the active buffer.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB039_HighlightsTreeViewActions: HighlightsTreeViewActions

**BL Ref**: BL039
**Type**: custom-command
**Source**: `crates/language_tools/src/highlights_tree_view.rs:30`

### Purpose

Opens a developer-tools view that visualizes the active editor's highlight layers (text highlights, semantic tokens, syntax tokens) and lets each layer be toggled on/off for debugging.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB040_OpenKeyContextViewAction: OpenKeyContextViewAction

**BL Ref**: BL040
**Type**: custom-command
**Source**: `crates/language_tools/src/key_context_view.rs:20`

### Purpose

Opens a developer-tools view showing the live keybinding-context predicate stack for the focused element, to debug why a binding fires or doesn't.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB041_LspToolMenuToggleAction: LspToolMenuToggleAction

**BL Ref**: BL041
**Type**: custom-command
**Source**: `crates/language_tools/src/lsp_button.rs:33`

### Purpose

Opens or closes the language-server status popover menu anchored to the status bar's LSP indicator.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB042_OpenLanguageServerLogsAction: OpenLanguageServerLogsAction

**BL Ref**: BL042
**Type**: custom-command
**Source**: `crates/language_tools/src/lsp_log_view.rs:110`

### Purpose

Opens the language-server-protocol log viewer, reading from a project-scoped `LogStore` so a user can inspect LSP traffic for debugging.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- LogStore (in-memory, project-scoped) — read

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB043_SyntaxTreeViewActions: SyntaxTreeViewActions

**BL Ref**: BL043
**Type**: custom-command
**Source**: `crates/language_tools/src/syntax_tree_view.rs:30`

### Purpose

Opens the Tree-sitter syntax-tree view for the current file and keeps it synced to the last-focused editor for debugging grammar/highlighting issues.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB044_LineEndingSelectorToggleAction: LineEndingSelectorToggleAction

**BL Ref**: BL044
**Type**: custom-command
**Source**: `crates/line_ending_selector/src/line_ending_selector.rs:14`

### Purpose

Opens the modal picker for switching the active buffer's line-ending convention (LF/CRLF).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB045_MarkdownCopyActions: MarkdownCopyActions

**BL Ref**: BL045
**Type**: custom-command
**Source**: `crates/markdown/src/markdown.rs:283`

### Purpose

Copies the current markdown selection either as plain rendered text or as raw markdown source, to the system clipboard.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- System clipboard — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB046_MarkdownPreviewScrollActions: MarkdownPreviewScrollActions

**BL Ref**: BL046
**Type**: custom-command
**Source**: `crates/markdown_preview/src/markdown_preview.rs:8`

### Purpose

Scrolls the rendered markdown preview pane by page, line, or rendered element, keeping legacy `MovePageUp`/`MovePageDown` aliases working.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB047_MenuNavigationActions: MenuNavigationActions

**BL Ref**: BL047
**Type**: custom-command
**Source**: `crates/menu/src/menu.rs:12`

### Purpose

Provides the shared cancel/confirm/select-previous/select-next navigation vocabulary reused by every list-like UI surface (pickers, context menus, completion popovers) in the app.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB048_ToggleBaseKeymapSelectorAction: ToggleBaseKeymapSelectorAction

**BL Ref**: BL048
**Type**: custom-command
**Source**: `crates/onboarding/src/base_keymap_picker.rs:14`

### Purpose

Opens the modal picker for choosing the app's base keymap preset (Default, VS Code, Vim, etc.), used both standalone and during onboarding.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- User keymap/settings — write (on selection)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB049_OnboardingFlowActions: OnboardingFlowActions

**BL Ref**: BL049
**Type**: custom-command
**Source**: `crates/onboarding/src/onboarding.rs:56`

### Purpose

Completes the first-run onboarding flow, resets multibuffer hint counters, and performs one-shot settings import from VS Code or Cursor when the user opts in.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- User settings file — write (on VS Code/Cursor import via `ImportVsCodeSettings`/`ImportCursorSettings`, `crates/onboarding/src/onboarding.rs:131,151`)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB050_OutlinePanelActions: OutlinePanelActions

**BL Ref**: BL050
**Type**: custom-command
**Source**: `crates/outline_panel/src/outline_panel.rs:68`

### Purpose

Drives expand/collapse, selection navigation, directory folding, and reveal-in-file-manager for entries in the outline/symbol panel.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB051_PanelTabNavigationActions: PanelTabNavigationActions

**BL Ref**: BL051
**Type**: custom-command
**Source**: `crates/panel/src/panel.rs:5`

### Purpose

Provides shared next/previous-tab navigation for any dock panel that implements the `PanelHeader` trait.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB052_PickerConfirmActions: PickerConfirmActions

**BL Ref**: BL052
**Type**: custom-command
**Source**: `crates/picker/src/picker.rs:38`

### Purpose

Confirms an in-progress inline completion inside a picker's editor, or (via `ConfirmInput`, `crates/picker/src/picker.rs:599`) acts on the picker's literal typed text rather than the highlighted entry.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB053_WindowTabManagementActions: WindowTabManagementActions

**BL Ref**: BL053
**Type**: custom-command
**Source**: `crates/platform_title_bar/src/system_window_tabs.rs:18`

### Purpose

Manages OS-native tabbed windows (macOS-style): shows the next/previous window tab, merges all windows into one tabbed group, or moves the current tab into a new window.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB054_ContextServerRestartAction: ContextServerRestartAction

**BL Ref**: BL054
**Type**: custom-command
**Source**: `crates/project/src/context_server_store.rs:40`

### Purpose

Restarts a stopped or errored MCP context server from the context-server status UI or command palette.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- ContextServerStatus — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action); the restart itself is a manual re-trigger, not an automatic retry.

---

## JOB055_ProjectPanelActions: ProjectPanelActions

**BL Ref**: BL055
**Type**: custom-command
**Source**: `crates/project_panel/src/project_panel.rs:343`

### Purpose

Drives file-tree navigation, expand/collapse, creation of new files/directories, and copy in the project panel; data-carrying variants (`Delete`, `Trash` at `:311,:320`) support an optional skip-confirmation prompt, and diagnostic-severity navigation (`SelectNextDiagnostic`/`SelectPrevDiagnostic` at `:329,:338`) jumps between flagged entries by severity.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Project (filesystem tree) — write (create/delete/trash/copy)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB056_RecentProjectsMenuActions: RecentProjectsMenuActions

**BL Ref**: BL056
**Type**: custom-command
**Source**: `crates/recent_projects/src/recent_projects.rs:55`

### Purpose

Toggles the per-entry actions menu in the recent-projects picker and removes or adds recent-project entries to the active multi-project workspace.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- WorkspaceId — write (remove/add)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB057_OpenWslPathAction: OpenWslPathAction

**BL Ref**: BL057
**Type**: custom-command
**Source**: `crates/remote/src/remote_client.rs:1369`

### Purpose

Opens a set of filesystem paths inside a WSL distro as part of the remote-connection flow; dispatched programmatically rather than by direct keybinding.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule. (In this case the invocation is programmatic — fired by the WSL remote-connection flow rather than a direct keypress — but it remains a non-scheduled, on-demand action rather than a timed job.)

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB058_ReplSessionActions: ReplSessionActions

**BL Ref**: BL058
**Type**: custom-command
**Source**: `crates/repl/src/repl_sessions_ui.rs:15`

### Purpose

Runs, interrupts, and clears output for Jupyter-kernel cells, and manages the REPL sessions panel listing active kernel connections.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB059_ProjectSearchActions: ProjectSearchActions

**BL Ref**: BL059
**Type**: custom-command
**Source**: `crates/search/src/project_search.rs:56`

### Purpose

Opens a new project-wide search tab and navigates its input fields and filters.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB060_BufferSearchActions: BufferSearchActions

**BL Ref**: BL060
**Type**: custom-command
**Source**: `crates/search/src/search.rs:29`

### Purpose

Toggles in-buffer search options — whole word, case sensitivity, regex, replace mode, and selection-only scope — for the in-editor search bar.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Editor (search state) — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB061_SettingsEditorActions: SettingsEditorActions

**BL Ref**: BL061
**Type**: custom-command
**Source**: `crates/settings_ui/src/settings_ui.rs:63`

### Purpose

Drives the Settings UI's navbar and file navigation (minimize, expand/collapse nav entries, focus next/previous file), plus direct jump-to-file-by-index via `FocusFile` (`:97`).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB062_FocusSidebarFilterAction: FocusSidebarFilterAction

**BL Ref**: BL062
**Type**: custom-command
**Source**: `crates/sidebar/src/sidebar.rs:36`

### Purpose

Moves keyboard focus into the multi-project sidebar's search/filter input editor.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- MultiWorkspace (focus state) — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB063_SnippetsConfigActions: SnippetsConfigActions

**BL Ref**: BL063
**Type**: custom-command
**Source**: `crates/snippets_ui/src/snippets_ui.rs:57`

### Purpose

Opens the user's snippets JSON file, or the underlying snippets folder in the system file manager, for manual editing.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- User snippets file (filesystem) — read/open

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB064_SvgOpenFollowingPreviewAction: SvgOpenFollowingPreviewAction

**BL Ref**: BL064
**Type**: custom-command
**Source**: `crates/svg_preview/src/svg_preview.rs:8`

### Purpose

Opens an SVG preview pane that follows and re-renders the content of the active `.svg` editor as it changes.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB065_CopySystemSpecsAction: CopySystemSpecsAction

**BL Ref**: BL065
**Type**: custom-command
**Source**: `crates/system_specs/src/system_specs.rs:11`

### Purpose

Copies OS, architecture, memory, and app-version diagnostics to the system clipboard for inclusion in bug reports.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- System clipboard — write

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB066_TabSwitcherActions: TabSwitcherActions

**BL Ref**: BL066
**Type**: custom-command
**Source**: `crates/tab_switcher/src/tab_switcher.rs:41`

### Purpose

Opens the modal Ctrl/Cmd+Tab-style tab switcher (`Toggle { select_last }` at `:37`), closes the selected tab, and toggles between per-pane and all-tabs view.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB067_TerminalCoreActions: TerminalCoreActions

**BL Ref**: BL067
**Type**: custom-command
**Source**: `crates/terminal/src/terminal.rs:77`

### Purpose

Provides the core terminal-emulator commands — clearing the screen, clipboard copy/paste, character-palette entry, and line-based scrolling — dispatched to the focused terminal pane.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB068_TerminalPanelToggleActions: TerminalPanelToggleActions

**BL Ref**: BL068
**Type**: custom-command
**Source**: `crates/terminal_view/src/terminal_panel.rs:45`

### Purpose

Shows or hides the terminal dock panel and moves keyboard focus into it.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB069_TerminalViewTextActions: TerminalViewTextActions

**BL Ref**: BL069
**Type**: custom-command
**Source**: `crates/terminal_view/src/terminal_view.rs:87`

### Purpose

Feeds text or keystrokes into a terminal programmatically (task-runner integration), re-runs the last task, and renames a terminal tab.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB070_ThemeSelectorReloadAction: ThemeSelectorReloadAction

**BL Ref**: BL070
**Type**: custom-command
**Source**: `crates/theme_selector/src/theme_selector.rs:23`

### Purpose

Reloads all themes from disk during theme-authoring iteration, without restarting the app.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB071_ApplicationMenuActivationActions: ApplicationMenuActivationActions

**BL Ref**: BL071
**Type**: custom-command
**Source**: `crates/title_bar/src/application_menu.rs:12`

### Purpose

Moves activation between adjacent top-level menus in the client-rendered (non-macOS) application menu bar, and opens a named menu directly.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB072_TitleBarProjectMenuActions: TitleBarProjectMenuActions

**BL Ref**: BL072
**Type**: custom-command
**Source**: `crates/title_bar/src/title_bar.rs:47`

### Purpose

Opens the title bar's project/branch dropdown menu and initiates a git branch switch from it.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB073_ToolchainSelectorActions: ToolchainSelectorActions

**BL Ref**: BL073
**Type**: custom-command
**Source**: `crates/toolchain_selector/src/toolchain_selector.rs:31`

### Purpose

Opens the modal picker for selecting or adding a language toolchain (e.g. Python venv, Node version) for the current project.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Project — read

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB074_VimChangeListNavigationActions: VimChangeListNavigationActions

**BL Ref**: BL074
**Type**: custom-command
**Source**: `crates/vim/src/change_list.rs:6`

### Purpose

Navigates backward and forward through Vim's change-list, the positions of recently edited text.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB075_VimExCommandActions: VimExCommandActions

**BL Ref**: BL075
**Type**: custom-command
**Source**: `crates/vim/src/command.rs:225`

### Purpose

Represents the internal data-carrying actions that Vim's `:` command-line parser dispatches to (go-to-line, yank, ranged commands, save, split, set options).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB076_VimDigraphLiteralAction: VimDigraphLiteralAction

**BL Ref**: BL076
**Type**: custom-command
**Source**: `crates/vim/src/digraph.rs:15`

### Purpose

Inserts the Unicode character mapped to a two-character digraph code entered via `Ctrl-K` in Vim insert mode.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB077_VimHelixModeActions: VimHelixModeActions

**BL Ref**: BL077
**Type**: custom-command
**Source**: `crates/vim/src/helix.rs:30`

### Purpose

Implements Helix-editor-style selection-first commands (yank, insert, append, goto-last-modification, select-line) within Vim's Helix emulation mode.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB078_VimHelixPasteAction: VimHelixPasteAction

**BL Ref**: BL078
**Type**: custom-command
**Source**: `crates/vim/src/helix/paste.rs:9`

### Purpose

Pastes register contents before or after the current selection while in Helix mode.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB079_VimIndentActions: VimIndentActions

**BL Ref**: BL079
**Type**: custom-command
**Source**: `crates/vim/src/indent.rs:18`

### Purpose

Increases, decreases, or syntax-auto-adjusts the indentation of the selected lines in Vim normal/visual mode.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB080_VimInsertModeActions: VimInsertModeActions

**BL Ref**: BL080
**Type**: custom-command
**Source**: `crates/vim/src/insert.rs:10`

### Purpose

Exits Vim insert mode back to normal, runs a one-shot normal-mode command from insert mode, or inserts the character directly above/below the cursor.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB081_VimMotionActions: VimMotionActions

**BL Ref**: BL081
**Type**: custom-command
**Source**: `crates/vim/src/motion.rs:339`

### Purpose

Implements Vim's word/subword/line motion vocabulary (next/previous word start/end, subword variants, vertical motions), most parameterized by punctuation and display-line flags.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB082_VimNormalModeInsertActions: VimNormalModeInsertActions

**BL Ref**: BL082
**Type**: custom-command
**Source**: `crates/vim/src/normal.rs:36`

### Purpose

Transitions from Vim normal mode into insert mode at various cursor positions (`i`, `a`, `I`, `A`, `o`, `O`-style entry points).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB083_VimIncrementDecrementActions: VimIncrementDecrementActions

**BL Ref**: BL083
**Type**: custom-command
**Source**: `crates/vim/src/normal/increment.rs:13`

### Purpose

Increments or decrements the number (or toggles the boolean) under the cursor in Vim normal mode.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB084_VimPasteAction: VimPasteAction

**BL Ref**: BL084
**Type**: custom-command
**Source**: `crates/vim/src/normal/paste.rs:21`

### Purpose

Pastes the active register's contents before or after the cursor/selection in Vim normal mode, optionally preserving the system clipboard.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB085_VimRepeatMacroActions: VimRepeatMacroActions

**BL Ref**: BL085
**Type**: custom-command
**Source**: `crates/vim/src/normal/repeat.rs:14`

### Purpose

Repeats the last change and drives macro recording/playback in Vim (`.`, `q`, `@@` semantics).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB086_VimScrollActions: VimScrollActions

**BL Ref**: BL086
**Type**: custom-command
**Source**: `crates/vim/src/normal/scroll.rs:11`

### Purpose

Scrolls the editor viewport by line, column, or half/full page without moving the cursor semantically.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB087_VimSearchActions: VimSearchActions

**BL Ref**: BL087
**Type**: custom-command
**Source**: `crates/vim/src/normal/search.rs:109`

### Purpose

Implements Vim's `/`, `?`, `*`, `#` search motions and `:s` substitute-via-search, each carrying case-sensitivity, partial-word, and regex flags.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB088_VimSubstituteActions: VimSubstituteActions

**BL Ref**: BL088
**Type**: custom-command
**Source**: `crates/vim/src/normal/substitute.rs:11`

### Purpose

Deletes the current selection or line and enters insert mode in its place (Vim `s`/`S`).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB089_VimTextObjectActions: VimTextObjectActions

**BL Ref**: BL089
**Type**: custom-command
**Source**: `crates/vim/src/object.rs:316`

### Purpose

Declares the text-object vocabulary (word, subword, indent block, brackets of each kind, sentence, paragraph, etc.) consumed as the object half of a Vim operator+object combo such as `diw` or `ci(`.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB090_VimReplaceModeActions: VimReplaceModeActions

**BL Ref**: BL090
**Type**: custom-command
**Source**: `crates/vim/src/replace.rs:16`

### Purpose

Toggles Vim replace (overtype) mode and undoes the last in-place character replacement.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB091_VimRewrapAction: VimRewrapAction

**BL Ref**: BL091
**Type**: custom-command
**Source**: `crates/vim/src/rewrap.rs:10`

### Purpose

Reflows/rewraps the selected text to a given or configured line length (Vim `gq`).

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB092_VimOperatorPendingPushActions: VimOperatorPendingPushActions

**BL Ref**: BL092
**Type**: custom-command
**Source**: `crates/vim/src/vim.rs:159`

### Purpose

Builds up Vim's multi-keystroke command state machine — pushing pending object, find, sneak, surrounds, jump, or numeric-count state while a composite command is mid-entry.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB093_VimVisualModeActions: VimVisualModeActions

**BL Ref**: BL093
**Type**: custom-command
**Source**: `crates/vim/src/visual.rs:23`

### Purpose

Enters and exits Vim's three visual-selection sub-modes (character, line, block) and deletes the active visual selection.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB094_MultiWorkspaceSidebarActions: MultiWorkspaceSidebarActions

**BL Ref**: BL094
**Type**: custom-command
**Source**: `crates/workspace/src/multi_workspace.rs:38`

### Purpose

Toggles and focuses the multi-project sidebar and cycles the active project within a window.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- MultiWorkspace — read/write (active-project selection state)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB095_PaneItemManagementActions: PaneItemManagementActions

**BL Ref**: BL095
**Type**: custom-command
**Source**: `crates/workspace/src/pane.rs:279`

### Purpose

Governs which tab/item in a pane is activated or closed, and whether unsaved-changes prompts are shown when closing.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Pane — read/write (item list, active item, pinned/close state)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB096_OpenThemePreviewAction: OpenThemePreviewAction

**BL Ref**: BL096
**Type**: custom-command
**Source**: `crates/workspace/src/theme_preview.rs:16`

### Purpose

Opens a dev-tools window rendering a gallery of the current theme's UI colors and tokens.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB097_WelcomeScreenActions: WelcomeScreenActions

**BL Ref**: BL097
**Type**: custom-command
**Source**: `crates/workspace/src/welcome.rs:28`

### Purpose

Opens the welcome screen and opens a recent-project entry at a given index directly from it.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB098_WorkspacePaneNavigationActions: WorkspacePaneNavigationActions

**BL Ref**: BL098
**Type**: custom-command
**Source**: `crates/workspace/src/workspace.rs:249`

### Purpose

Core workspace pane/window navigation and file-opening: opening paths into a new window or the current multi-project workspace, cycling panes/windows, moving items between panes, splitting, and save-all.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

- Workspace — read/write (pane layout, open items, save state)

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB099_ZedApplicationWindowActions: ZedApplicationWindowActions

**BL Ref**: BL099
**Type**: custom-command
**Source**: `crates/zed/src/zed.rs:95`

### Purpose

Top-level application window commands — element inspector (DebugElements), hide/hide-others/minimize window, opening default settings — plus a separate URL-entry prompt action.

### Schedule / Trigger

User-invoked via command palette, keybinding, or native application menu; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source (synchronous UI action).

---

## JOB100_ZedActionsSharedRegistry: ZedActionsSharedRegistry

**BL Ref**: BL100
**Type**: custom-command
**Source**: `crates/zed_actions/src/lib.rs:19`

### Purpose

Provides the shared `zed` namespace action types (`OpenBrowser`, `OpenZedUrl`, `ChangeKeybinding`, `Extensions`, `DecreaseBufferFontSize`, etc.) so leaf UI crates can dispatch these commands without a direct dependency on `zed`/`workspace`, avoiding circular crate references.

### Schedule / Trigger

User-invoked via command palette or keybinding; no automatic schedule.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB101_DebouncedDelayTimer: Debounced Delay Timer

**BL Ref**: BL144
**Type**: scheduled-job
**Source**: `crates/project/src/debounced_delay.rs:26`

### Purpose

Generic reusable primitive that coalesces bursts of repeated `fire_new` calls into a single delayed callback execution, cancelling any in-flight timer for the same debouncer instance.

### Schedule / Trigger

Re-armed on every `fire_new` call with a caller-supplied `Duration`; previous pending timer is cancelled via a `oneshot` channel and a fresh `cx.background_executor().timer(delay)` is scheduled. Only the last call within the window fires.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source; cancellation (not failure) is the only early-exit path via the `select_biased!` race against the cancel receiver.

---

## JOB102_GitDiffDebounceRecalculation: Git Diff Debounced Recalculation

**BL Ref**: BL145
**Type**: scheduled-job
**Source**: `crates/project/src/project.rs:3616`

### Purpose

Coalesces bursts of buffer edit/reload events into one deferred git-diff recalculation pass per project, avoiding a full diff recompute on every keystroke.

### Schedule / Trigger

Debounced via the `git_diff_debouncer` (`DebouncedDelay<Project>`, JOB101) using `ProjectSettings::git.gutter_debounce` milliseconds, floored to a 50ms minimum; if the setting is 0, the first insertion runs immediately via `cx.defer` instead of a timer.

### Data Touched

- Project — read
- Buffer — read

### Failure / Retry Behavior

N/A — no retry policy found in source; `recalculate_buffer_diffs` drains `buffers_needing_diff` in a loop until empty regardless of individual buffer resolution failures.

---

## JOB103_LogCommandInvocation: Log Command Invocation

**BL Ref**: BL146
**Type**: queue-worker
**Source**: `crates/command_palette/src/command_palette.rs:561`

### Purpose

Records the invoked command name and the query string used to find it into a local SQLite ranking store, feeding fuzzy-match ranking on subsequent command-palette opens.

### Schedule / Trigger

Dispatched via a detached background task from `CommandPalette::confirm` immediately after the selected action executes, on every Enter-confirm in the command palette.

### Data Touched

- CommandPaletteDB — write

### Failure / Retry Behavior

No retry; write errors are logged and swallowed rather than surfaced to the user (per BL146 description).

---

## JOB104_SaveComponentPreviewActivePage: Save Component Preview Active Page

**BL Ref**: BL147
**Type**: queue-worker
**Source**: `crates/component_preview/src/component_preview.rs:857`

### Purpose

Keeps the last-viewed page of an open Component Preview tab durable so the tab can restore to the same page on next launch.

### Schedule / Trigger

Dispatched via `cx.background_spawn` when the workspace's item-serialization pass reaches an open Component Preview tab (on tab/workspace state changes and app close).

### Data Touched

- ComponentPreviewDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB105_McpServeConnection: Mcp Serve Connection

**BL Ref**: BL148
**Type**: queue-worker
**Source**: `crates/context_server/src/listener.rs:198`

### Purpose

Runs the request/response pump for a single MCP client connection over its Unix-socket lifetime, decoupling the connection's I/O loop from the accept path.

### Schedule / Trigger

Dispatched and detached for the duration of the connection each time an MCP client connects to the local Unix socket the context-server listener created.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source; the task simply ends when the connection's I/O pump (`handle_io`) completes or errors.

---

## JOB106_DbWriteAndLog: Db Write And Log

**BL Ref**: BL149
**Type**: queue-worker
**Source**: `crates/db/src/db.rs:288`

### Purpose

Standard "persist and forget" primitive used throughout the codebase for fire-and-forget SQLite writes (workspace/item state), so callers don't block on or propagate DB-write failures.

### Schedule / Trigger

Dispatched via `cx.background_spawn` whenever a call site invokes `db::write_and_log` with a write future to persist.

### Data Touched

- (varies by call site — generic SQLite write helper) — write

### Failure / Retry Behavior

No retry; any write error is logged, never propagated to the caller.

---

## JOB107_FetchRemoteProcessListForAttach: Fetch Remote Process List For Attach

**BL Ref**: BL150
**Type**: queue-worker
**Source**: `crates/debugger_ui/src/attach_modal.rs:360`

### Purpose

Populates the debugger's "Attach to Process" picker with the remote (SSH) host's running-process list without blocking the modal's UI thread.

### Schedule / Trigger

Dispatched as a background task when the user opens the Attach-to-Process modal against a remote SSH project.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB108_ForwardSpawnedTerminalPid: Forward Spawned Terminal Pid

**BL Ref**: BL151
**Type**: queue-worker
**Source**: `crates/debugger_ui/src/session/running.rs:1152`

### Purpose

Decouples integrated-terminal construction from the DAP `runInTerminal` response by asynchronously forwarding the spawned terminal's PID (or spawn error) back to the debug adapter over a channel.

### Schedule / Trigger

Dispatched as a background task when a debug session spawns an integrated terminal for a debuggee process (via `project.create_terminal_task`).

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source; a spawn error is forwarded through the channel as the `Err` variant rather than retried.

---

## JOB109_PersistStackFrameFilterPreference: Persist Stack Frame Filter Preference

**BL Ref**: BL152
**Type**: queue-worker
**Source**: `crates/debugger_ui/src/session/running/stack_frame_list.rs:838`

### Purpose

Saves the debugger's "only user frames" filter choice per adapter and workspace so the preference survives app restarts.

### Schedule / Trigger

Dispatched as a detached background task each time the user toggles the stack-frame filter in the debugger UI.

### Data Touched

- KeyValueStore — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB110_ComputeCompletionMenuFilterMatches: Compute Completion Menu Filter Matches

**BL Ref**: BL153
**Type**: queue-worker
**Source**: `crates/editor/src/code_context_menus.rs:1156`

### Purpose

Keeps the completions menu responsive while typing by offloading fuzzy-match filtering of completion candidates against the current query to the background executor.

### Schedule / Trigger

Dispatched via `cx.background_spawn` each time the editor's completion menu needs its displayed matches re-filtered against an updated query.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB111_SerializeEditorSelections: Serialize Editor Selections

**BL Ref**: BL154
**Type**: queue-worker
**Source**: `crates/editor/src/editor.rs:3855`

### Purpose

Persists the current selection offsets for an editor tab so cursor/selection state can be restored when the workspace session reloads.

### Schedule / Trigger

Debounced via a background timer, re-armed on each selection change for editors eligible for restore-on-startup persistence; the task is held in the `serialize_selections: Task<()>` field.

### Data Touched

- EditorDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB112_SerializeEditorFolds: Serialize Editor Folds

**BL Ref**: BL155
**Type**: queue-worker
**Source**: `crates/editor/src/items.rs:1346`

### Purpose

Persists dirty-buffer content/language plus serialized-editor metadata (absolute path, mtime) so an editor tab's fold and file state can be restored on relaunch.

### Schedule / Trigger

Dispatched as a background task from an async closure when a workspace item requests serialization — on tab close or tab-update passes.

### Data Touched

- EditorDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB113_ComputeRunnableRanges: Compute Runnable Ranges

**BL Ref**: BL156
**Type**: queue-worker
**Source**: `crates/editor/src/runnables.rs:107`

### Purpose

Recomputes the run/debug gutter affordances (test/run code-lens ranges) for the currently visible buffer range so scanning cost doesn't block editor rendering.

### Schedule / Trigger

Dispatched via `cx.spawn_in` from `refresh_runnables` whenever the visible multi-buffer range changes or buffer data is invalidated, gated by `EditorSettings.gutter.runnables` and a per-buffer-version cache check.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB114_CompileDevExtension: Compile Dev Extension

**BL Ref**: BL157
**Type**: queue-worker
**Source**: `crates/extension_host/src/extension_host.rs:1030`

### Purpose

Rebuilds a locally-installed dev extension's manifest/wasm in the background so the extension can be reloaded without freezing the editor during compilation.

### Schedule / Trigger

Dispatched when the user triggers a rebuild of a locally-installed dev extension; a foreground task then awaits the background compile to clear the `outstanding_operations` marker.

### Data Touched

- ExtensionManifest — write

### Failure / Retry Behavior

No automatic retry found in source; compile failure clears the in-flight marker and notifies the UI of failure rather than re-attempting.

---

## JOB115_DismissLanguageExtensionSuggestion: Dismiss Language Extension Suggestion

**BL Ref**: BL158
**Type**: queue-worker
**Source**: `crates/extensions_ui/src/extension_suggest.rs:196`

### Purpose

Suppresses the "install this language extension" suggestion banner for a given language once the user has explicitly declined it.

### Schedule / Trigger

Dispatched as a detached background task from the banner's `secondary_on_click` handler when the user clicks "No, don't install it".

### Data Touched

- KeyValueStore — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB116_CheckRecentHistoryPathExists: Check Recent History Path Exists

**BL Ref**: BL159
**Type**: queue-worker
**Source**: `crates/file_finder/src/file_finder.rs:185`

### Purpose

Filters out stale recent-navigation history entries whose backing file no longer exists before offering them as file-finder match candidates.

### Schedule / Trigger

Dispatched per untracked recent-history entry as a background filesystem check (`fs.is_file`) when `FileFinderDelegate::new` builds the initial candidate set for a local project.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB117_ReadGlobalGitCommitterIdentity: Read Global Git Committer Identity

**BL Ref**: BL160
**Type**: queue-worker
**Source**: `crates/git/src/repository.rs:1156`

### Purpose

Supplies the configured global git committer name/email to git operations that need to attribute a commit or similar identity-bearing action.

### Schedule / Trigger

Dispatched on the background executor whenever a call site needs the committer identity; runs `git config --global user.name`/`user.email` via the git binary wrapper (`get_git_committer`).

### Data Touched

N/A — no persisted data touched (reads external git config, not an app-owned entity)

### Failure / Retry Behavior

No retry; missing config is logged and returns a default `GitCommitter` rather than failing the caller.

---

## JOB118_SaveGitGraphSerialization: Save Git Graph Serialization

**BL Ref**: BL161
**Type**: queue-worker
**Source**: `crates/git_graph/src/git_graph.rs:3011`

### Purpose

Persists an open Git Graph tab's identity and target repository path so the graph view restores to the same repository on relaunch.

### Schedule / Trigger

Dispatched in the background when the workspace's item-serialization pass reaches an open Git Graph tab.

### Data Touched

- GitGraphsDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB119_CreateGitRemote: Create Git Remote

**BL Ref**: BL162
**Type**: queue-worker
**Source**: `crates/git_ui/src/branch_picker.rs:492`

### Purpose

Adds a new git remote from the branch picker's create-remote flow without blocking the picker UI while the repository operation completes.

### Schedule / Trigger

Dispatched and detached when the user confirms adding a new remote in the branch picker's create-remote flow.

### Data Touched

N/A — no persisted data touched (mutates the on-disk git repository's remote config, not an app-owned entity)

### Failure / Retry Behavior

No retry; failures surface as a dialog via an error-prompt handler attached to the detached task.

---

## JOB120_CommitStagedChanges: Commit Staged Changes

**BL Ref**: BL163
**Type**: queue-worker
**Source**: `crates/git_ui/src/git_panel.rs:2280`

### Purpose

Runs a git commit of staged changes off the UI thread so the git panel stays responsive while the commit is in flight.

### Schedule / Trigger

Dispatched as a background-spawned task when the user triggers commit in the git panel with staged changes present; the underlying repository serializes git operations internally.

### Data Touched

N/A — no persisted app-owned data touched (mutates the on-disk git repository)

### Failure / Retry Behavior

No retry; the panel awaits the background task and reports the outcome to the user.

---

## JOB121_SaveProjectDiffBase: Save Project Diff Base

**BL Ref**: BL164
**Type**: queue-worker
**Source**: `crates/git_ui/src/project_diff.rs:1166`

### Purpose

Persists which diff base an open Project Diff tab is comparing against so the diff view restores consistently on relaunch.

### Schedule / Trigger

Dispatched in the background when the workspace's item-serialization pass reaches an open Project Diff tab.

### Data Touched

- ProjectDiffDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB122_DetachAndLogErrTask: Detach And Log Err Task

**BL Ref**: BL165
**Type**: queue-worker
**Source**: `crates/gpui/src/executor.rs:91`

### Purpose

Core GPUI fire-and-forget primitive: runs a fallible task to completion independent of the caller's lifetime, converting any error into a logged diagnostic instead of a propagated failure.

### Schedule / Trigger

Dispatched on the foreground executor whenever a call site holding a `Task<Result<T, E>>` invokes `.detach_and_log_err(cx)`; used pervasively across the codebase for fire-and-forget task lifecycles.

### Data Touched

N/A — no persisted data touched (generic executor primitive)

### Failure / Retry Behavior

No retry; the tracked error location is logged and the failure is otherwise silently dropped.

---

## JOB123_SaveImageViewerPath: Save Image Viewer Path

**BL Ref**: BL166
**Type**: queue-worker
**Source**: `crates/image_viewer/src/image_viewer.rs:639`

### Purpose

Persists the absolute path of an open image so the Image Viewer tab restores the same image on relaunch.

### Schedule / Trigger

Dispatched in the background when the workspace's item-serialization pass reaches an open Image Viewer tab.

### Data Touched

- ImageViewerDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB124_PromptCliInstallOnLinux: Prompt Cli Install On Linux

**BL Ref**: BL167
**Type**: queue-worker
**Source**: `crates/install_cli/src/install_cli_binary.rs:64`

### Purpose

Shows the user manual CLI-installation instructions on Linux/FreeBSD, where the CLI symlink cannot be created automatically.

### Schedule / Trigger

Dispatched and detached when the user runs "Install CLI" on a platform requiring manual symlinking, decoupling the informational dialog's await from the calling context.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB125_CreateJournalEntryFile: Create Journal Entry File

**BL Ref**: BL168
**Type**: queue-worker
**Source**: `crates/journal/src/journal.rs:57`

### Purpose

Creates the on-disk journal month directory and today's entry file so the caller can open or focus the corresponding editor tab.

### Schedule / Trigger

Dispatched as a background task when the user invokes "New Journal Entry".

### Data Touched

N/A — no persisted app-owned data touched (writes to the user's on-disk journal directory)

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB126_DebounceKeymapActionSearch: Debounce Keymap Action Search

**BL Ref**: BL169
**Type**: queue-worker
**Source**: `crates/keymap_editor/src/keymap_editor.rs:670`

### Purpose

Records a telemetry event for a completed keymap-editor action/keystroke search without emitting one per keystroke.

### Schedule / Trigger

Dispatched as a background task on each query change in the keymap editor's search fields; waits out a fixed 1-second debounce before emitting the telemetry event.

### Data Touched

N/A — no persisted data touched (emits telemetry, not app-owned entity storage)

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB127_SendLspInitializeRequest: Send Lsp Initialize Request

**BL Ref**: BL170
**Type**: queue-worker
**Source**: `crates/lsp/src/lsp.rs:1051`

### Purpose

Performs the LSP handshake for a newly spawned language server process, recording the server's advertised name/version so startup can complete.

### Schedule / Trigger

Dispatched on the background executor immediately after a language server process is spawned; the caller awaits the returned `Task<Result<Arc<Self>>>` to complete server startup.

### Data Touched

N/A — no persisted data touched (in-memory server-state update)

### Failure / Retry Behavior

N/A — no retry policy found in source; the `Result` is propagated to the awaiting caller rather than retried internally.

---

## JOB128_ParseMarkdownSource: Parse Markdown Source

**BL Ref**: BL171
**Type**: queue-worker
**Source**: `crates/markdown/src/markdown.rs:666`

### Purpose

Keeps markdown rendering responsive by parsing links, HTML, Mermaid diagrams, heading slugs, and footnotes off the render loop for large documents.

### Schedule / Trigger

Dispatched via `start_background_parse` whenever the markdown source content changes and needs re-parsing; result is staged in `pending_parse` and applied once ready.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB129_SearchMarkdownPreviewSource: Search Markdown Preview Source

**BL Ref**: BL172
**Type**: queue-worker
**Source**: `crates/markdown_preview/src/markdown_preview_view.rs:1025`

### Purpose

Runs an in-buffer search against the rendered markdown preview's source text without blocking the main thread while highlighting matches.

### Schedule / Trigger

Dispatched on the background executor each time the user runs an in-buffer search (Cmd-F) inside a markdown preview view.

### Data Touched

N/A — no persisted data touched

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB130_ExportMiniprofilerReport: Export Miniprofiler Report

**BL Ref**: BL173
**Type**: queue-worker
**Source**: `crates/miniprofiler_ui/src/miniprofiler_ui.rs:546`

### Purpose

Writes a serialized performance-profile JSON to disk at a user-chosen path so profiling data can be shared or analyzed outside the app.

### Schedule / Trigger

Dispatched as a detached background task after the user picks a save path in the miniprofiler export flow (`Button::new("export-data", "Save")` click handler); awaits the path prompt then writes via `smol::fs::write`.

### Data Touched

N/A — no persisted app-owned data touched (writes to a user-chosen file path)

### Failure / Retry Behavior

No retry; I/O errors are logged (`log_err()`) rather than surfaced as a failed UI action.

---

## JOB131_SaveOnboardingActivePage: Save Onboarding Active Page

**BL Ref**: BL174
**Type**: queue-worker
**Source**: `crates/onboarding/src/onboarding.rs:568`

### Purpose

Persists which onboarding page was active so an open Onboarding tab restores to the same page on relaunch.

### Schedule / Trigger

Dispatched in the background when the workspace's item-serialization pass reaches an open Onboarding tab.

### Data Touched

- OnboardingPagesDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB132_SerializeOutlinePanelState: Serialize Outline Panel State

**BL Ref**: BL175
**Type**: queue-worker
**Source**: `crates/outline_panel/src/outline_panel.rs:905`

### Purpose

Persists the outline panel's active/visible flag so the panel's UI state is restored per workspace on relaunch.

### Schedule / Trigger

Dispatched as a background task whenever the outline panel's active/visible state changes; serializes to JSON and writes under the panel's per-workspace key.

### Data Touched

- KeyValueStore — write

### Failure / Retry Behavior

No retry; write errors are logged rather than propagated.

---

---

## JOB133_LoadBufferContentsOffThread: Load Buffer Contents Off Thread

**BL Ref**: BL176
**Type**: queue-worker
**Source**: `crates/project/src/buffer_store.rs:848`

### Purpose

Decouples the disk/LSP-backed buffer load (which can be slow) from the calling context's own executor by having `BufferStore::open_buffer` hand the shared load task to a normalized background wrapper.

### Schedule / Trigger

Dispatched via `cx.background_spawn` whenever a buffer is requested (`BufferStore::open_buffer`, `crates/project/src/buffer_store.rs:848`) that is not already resident and not already mid-load in `loading_buffers`.

### Data Touched

N/A — no persisted data touched; operates on in-memory `Buffer` entities only.

### Failure / Retry Behavior

N/A — no retry policy found in source; the task normalizes and propagates the load error to the awaiting caller rather than retrying.

---

## JOB134_ForwardBreakpointToggleToRemote: Forward Breakpoint Toggle To Remote

**BL Ref**: BL177
**Type**: queue-worker
**Source**: `crates/project/src/debugger/breakpoint_store.rs:404`

### Purpose

Keeps the local breakpoint UI responsive by not waiting on the collab network round-trip when a breakpoint toggle must be mirrored to the upstream remote session.

### Schedule / Trigger

Dispatched from `BreakpointStore::toggle_breakpoint` (`crates/project/src/debugger/breakpoint_store.rs:404`) whenever the user toggles a breakpoint while the project is a remote (collab) session; the forwarding request is detached rather than awaited.

### Data Touched

N/A — no persisted data touched; forwards a proto message to the collab client.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB135_PruneStaleJsDebugCompanionVersions: Prune Stale Js Debug Companion Versions

**BL Ref**: BL178
**Type**: queue-worker
**Source**: `crates/project/src/debugger/dap_store.rs:195` (background spawn at `:200`)

### Purpose

Keeps the on-disk js-debug-companion cache from growing unbounded across upgrades by removing all but the newest semver-versioned copy each time a local `DapStore` is constructed.

### Schedule / Trigger

Dispatched via `cx.background_spawn` inside `DapStore::new` (`crates/project/src/debugger/dap_store.rs:195`) at construction time in local mode; scans `paths::debug_adapters_dir().join("js-debug-companion")`.

### Data Touched

N/A — no persisted data touched; operates on the filesystem cache directory only.

### Failure / Retry Behavior

N/A — no retry policy found in source; directory-read/parse failures propagate as `Result` errors within the spawned task, not retried.

---

## JOB136_InstallLatestJsDebugCompanion: Install Latest Js Debug Companion

**BL Ref**: BL179
**Type**: queue-worker
**Source**: `crates/project/src/debugger/session.rs:3146` (`install_latest_version`), spawned at `:3203`/`:3208`

### Purpose

Upgrades the js-debug-companion npm package opportunistically in the background while the active debug session keeps using the currently installed version, so debug startup is never blocked on an npm install.

### Schedule / Trigger

Dispatched via `cx.background_spawn(install_latest_version(...))` (`crates/project/src/debugger/session.rs:3203`/`:3208`) whenever a newer js-debug-companion npm package version is detected than what is installed.

### Data Touched

N/A — no persisted data touched; writes the installed package to the on-disk debug-adapters cache.

### Failure / Retry Behavior

N/A — no retry policy found in source; the install task is detached and its result is not awaited by the active session.

---

## JOB137_RestoreGitCheckpoints: Restore Git Checkpoints

**BL Ref**: BL180
**Type**: queue-worker
**Source**: `crates/project/src/git_store.rs:1144`

### Purpose

Reverts the working tree(s) to a previously captured git checkpoint on behalf of agent/edit-prediction flows, running all affected repositories' restores concurrently rather than serially.

### Schedule / Trigger

Dispatched via the background executor from `GitStore::restore_checkpoint` (`crates/project/src/git_store.rs:1144`) whenever an agent/edit-prediction flow requests a checkpoint restore; per-repository futures are joined into a single `Task<Result<()>>`.

### Data Touched

- GitStoreCheckpoint — read

### Failure / Retry Behavior

N/A — no retry policy found in source; the joined task returns a `Result` the caller awaits and handles once.

---

## JOB138_WaitForImageLoad: Wait For Image Load

**BL Ref**: BL181
**Type**: queue-worker
**Source**: `crates/project/src/image_store.rs:374`

### Purpose

Decouples image decode/load latency from the requesting entity's executor by spawning a background task that awaits the shared image-loading watch channel and maps any error.

### Schedule / Trigger

Dispatched via `cx.background_spawn` from `ImageStore::open_image` (`crates/project/src/image_store.rs:374`) whenever an image path is requested that is not yet loaded into the image store.

### Data Touched

N/A — no persisted data touched; operates on in-memory image entities only.

### Failure / Retry Behavior

N/A — no retry policy found in source; watch-channel errors are mapped and returned to the awaiter, not retried.

---

## JOB139_RestartLanguageServersForBuffers: Restart Language Servers For Buffers

**BL Ref**: BL182
**Type**: queue-worker
**Source**: `crates/project/src/lsp_store.rs:11717`

### Purpose

Lets the local UI proceed without blocking on the network round-trip when restarting language servers for a set of buffers on a remote project, by detaching the restart request to the remote server.

### Schedule / Trigger

Dispatched from `LspStore::restart_language_servers_for_buffers` (`crates/project/src/lsp_store.rs:11717`) whenever a user or system action requests a language-server restart for buffers belonging to a remote project path.

### Data Touched

N/A — no persisted data touched; sends a `RestartLanguageServers` proto request.

### Failure / Retry Behavior

Failures are logged (per BL entry's "logging failures") rather than retried; no retry/backoff policy found in source.

---

## JOB140_ForwardVueTsRequestToTypescriptServer: Forward Vue Ts Request To Typescript Server

**BL Ref**: BL183
**Type**: queue-worker
**Source**: `crates/project/src/lsp_store/vue_language_server_ext.rs:28` (background spawn at `:87`)

### Purpose

Proxies embedded TypeScript requests issued by the Vue language server to the real tsserver, allowing several such requests to be handled concurrently instead of serializing them on one executor context.

### Schedule / Trigger

Dispatched via `cx.background_spawn` inside `register_requests` (`crates/project/src/lsp_store/vue_language_server_ext.rs:87`) each time the Vue language server issues a `workspace/executeCommand`-forwarded TS request.

### Data Touched

N/A — no persisted data touched; routes an LSP request/response pair between two language servers.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB141_ClearPrettierCachesOnSettingsChange: Clear Prettier Caches On Settings Change

**BL Ref**: BL184
**Type**: queue-worker
**Source**: `crates/project/src/prettier_store.rs:474`

### Purpose

Ensures newly-edited prettier configuration takes effect without a manual restart by clearing each affected running prettier instance's internal cache after a settings change.

### Schedule / Trigger

Dispatched via the background executor from `PrettierStore::update_prettier_settings` (`crates/project/src/prettier_store.rs:474`) whenever prettier-related project settings change; runs with the store's configured request timeout.

### Data Touched

N/A — no persisted data touched; clears in-process prettier instance caches.

### Failure / Retry Behavior

N/A — no retry policy found in source; a per-instance timeout bounds the clear request but does not retry on expiry.

---

## JOB142_ForwardBufferUpdateToRemoteServer: Forward Buffer Update To Remote Server

**BL Ref**: BL185
**Type**: queue-worker
**Source**: `crates/project/src/project.rs:5462`

### Purpose

Re-broadcasts a buffer edit operation received from a remote server back out to the collab client under the project's own remote id, without blocking the RPC handler that received it.

### Schedule / Trigger

Dispatched via a detached background request from `Project::handle_update_buffer_from_remote_server` (`crates/project/src/project.rs:5462`) whenever a remote-hosted buffer receives a local edit operation while the project acts as a remote-server client.

### Data Touched

N/A — no persisted data touched; forwards a buffer-operation proto message.

### Failure / Retry Behavior

Failures are logged rather than retried (per BL entry's "logging any failure"); no retry/backoff policy found in source.

---

## JOB143_CopyWorktreeEntryOffThread: Copy Worktree Entry Off Thread

**BL Ref**: BL186
**Type**: queue-worker
**Source**: `crates/project/src/worktree_store.rs:467`

### Purpose

Keeps potentially slow recursive filesystem copies off the calling context when a user copies a file/directory entry within a local worktree, updating the destination worktree's entry state once the copy completes.

### Schedule / Trigger

Dispatched via the background executor from `WorktreeStore::copy_entry` (`crates/project/src/worktree_store.rs:467`) whenever a user copies a file/directory entry within a local worktree; a follow-up spawned task awaits completion.

### Data Touched

N/A — no persisted data touched (outside the worktree's own filesystem); updates the in-memory worktree entry tree.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB144_DismissDevContainerSuggestion: Dismiss Dev Container Suggestion

**BL Ref**: BL187
**Type**: queue-worker
**Source**: `crates/recent_projects/src/dev_container_suggest.rs:33` (background spawn at `:144`)

### Purpose

Persists the user's "Don't Show Again" choice so the dev-container open suggestion banner is not shown again for that worktree.

### Schedule / Trigger

Dispatched via `cx.background_spawn` inside `suggest_on_worktree_updated` (`crates/recent_projects/src/dev_container_suggest.rs:144`) when the user clicks "Don't Show Again" on the dev-container suggestion banner.

### Data Touched

- KeyValueStore — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB145_ForwardBufferUpdateOnHeadlessProject: Forward Buffer Update On Headless Project

**BL Ref**: BL188
**Type**: queue-worker
**Source**: `crates/remote_server/src/headless_project.rs:364`

### Purpose

Decouples remote-server protocol I/O from the buffer-event callback by detaching the send of a serialized buffer operation to the connected client session.

### Schedule / Trigger

Dispatched via a detached background request from `HeadlessProject::on_buffer_event` (`crates/remote_server/src/headless_project.rs:364`) whenever a locally-originated buffer operation occurs inside a headless (remote_server) project.

### Data Touched

N/A — no persisted data touched; sends a serialized buffer-operation frame over the remote-server protocol.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB146_ReadRemoteServerStdinLoop: Read Remote Server Stdin Loop

**BL Ref**: BL189
**Type**: queue-worker
**Source**: `crates/remote_server/src/server.rs:294` (background spawn at `:358`)

### Purpose

Reads length-prefixed protocol frames from stdin and forwards them over an internal mpsc channel for the entire life of the remote-server connection, keeping stdin I/O off the main protocol-handling path.

### Schedule / Trigger

Dispatched via `cx.background_spawn` inside `start_server` (`crates/remote_server/src/server.rs:358`) when the remote server process starts; the loop runs continuously for the life of the connection.

### Data Touched

N/A — no persisted data touched; reads bytes from stdin into an in-process channel.

### Failure / Retry Behavior

N/A — no retry policy found in source; the loop simply ends when the stdin stream closes.

---

## JOB147_RouteJupyterMessagesToChannels: Route Jupyter Messages To Channels

**BL Ref**: BL190
**Type**: queue-worker
**Source**: `crates/repl/src/kernels/mod.rs:90`

### Purpose

Wires a Jupyter kernel session's shell/control/stdin channels by routing debug/interrupt/shutdown requests to the control channel and all other outgoing messages to the shell channel, for the duration of the session.

### Schedule / Trigger

Dispatched via the background executor from `start_kernel_tasks` (`crates/repl/src/kernels/mod.rs:90`) when a Jupyter kernel session starts; the routing task runs for the life of the kernel session.

### Data Touched

N/A — no persisted data touched; routes in-memory Jupyter protocol messages between channels.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB148_RefreshPythonKernelspecs: Refresh Python Kernelspecs

**BL Ref**: BL191
**Type**: queue-worker
**Source**: `crates/repl/src/notebook/notebook_ui.rs:230`

### Purpose

Discovers available Python kernelspecs for a notebook's worktree without blocking notebook rendering on the disk/process scan the REPL store performs.

### Schedule / Trigger

Dispatched via `NotebookEditor::refresh_kernelspecs` (`crates/repl/src/notebook/notebook_ui.rs:230`), which detaches the REPL store's kernelspec-refresh task on the background executor, whenever a notebook editor needs to refresh available kernelspecs.

### Data Touched

N/A — no persisted data touched; queries the local Python environment for kernelspecs.

### Failure / Retry Behavior

Errors are logged (per BL entry's error-logging on detach) rather than retried; no retry/backoff policy found in source.

---

## JOB149_InstallIpykernelPackage: Install Ipykernel Package

**BL Ref**: BL192
**Type**: queue-worker
**Source**: `crates/repl/src/repl_editor.rs:78`

### Purpose

Installs the missing `ipykernel` package into the target Python environment so a REPL kernel can be assigned, showing a toast to keep the user informed during the install.

### Schedule / Trigger

Dispatched via the background executor from `install_ipykernel_and_assign` (`crates/repl/src/repl_editor.rs:78`) when REPL execution detects the target Python environment is missing ipykernel; runs `uv pip install` or `pip install`.

### Data Touched

N/A — no persisted data touched; installs a package into the Python environment on disk.

### Failure / Retry Behavior

N/A — no retry policy found in source; install failure is surfaced via the toast notification rather than retried automatically.

---

## JOB150_ComputeSettingsExactSearchMatches: Compute Settings Exact Search Matches

**BL Ref**: BL193
**Type**: queue-worker
**Source**: `crates/settings_ui/src/settings_ui.rs:1906`

### Purpose

Keeps the Settings window's search box responsive while typing by running the exact-word-match branch of the combined search over the settings index on the background executor.

### Schedule / Trigger

Dispatched via the background executor from `SettingsWindow::update_matches` (`crates/settings_ui/src/settings_ui.rs:1906`) whenever the user types a query in the Settings window's search box; runs alongside the fuzzy-match branch.

### Data Touched

N/A — no persisted data touched; searches an in-memory settings index.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB151_BuildTaskContextsOffThread: Build Task Contexts Off Thread

**BL Ref**: BL194
**Type**: queue-worker
**Source**: `crates/tasks_ui/src/tasks_ui.rs:271`

### Purpose

Assembles the active editor context, worktree paths, LSP task sources, and latest selection needed for task-template variable substitution without blocking the UI when many worktrees/LSPs are involved.

### Schedule / Trigger

Dispatched via the background executor from `task_contexts` (`crates/tasks_ui/src/tasks_ui.rs:271`) whenever the tasks UI (spawn/run) needs the current `TaskContexts`.

### Data Touched

N/A — no persisted data touched; assembles an in-memory `TaskContexts` value.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB152_SearchTerminalScrollback: Search Terminal Scrollback

**BL Ref**: BL195
**Type**: queue-worker
**Source**: `crates/terminal/src/terminal.rs:2148`

### Purpose

Runs the user's in-terminal regex search across the terminal's scrollback/grid without blocking terminal rendering, returning the matched ranges once complete.

### Schedule / Trigger

Dispatched via the background executor from `Terminal::find_matches` (`crates/terminal/src/terminal.rs:2148`) whenever the user runs an in-terminal search (Cmd-F); holds the terminal lock while searching.

### Data Touched

N/A — no persisted data touched; searches the in-memory terminal grid/scrollback buffer.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB153_SerializeTerminalPanelLayout: Serialize Terminal Panel Layout

**BL Ref**: BL196
**Type**: queue-worker
**Source**: `crates/terminal_view/src/terminal_panel.rs:947`

### Purpose

Persists the terminal panel's pane group layout so frequent layout changes (splits, resizes) do not generate excessive database writes.

### Schedule / Trigger

Dispatched via a background task from `TerminalPanel::serialize` (`crates/terminal_view/src/terminal_panel.rs:947`), fired after a 50ms debounce whenever the terminal panel's pane layout changes.

### Data Touched

- KeyValueStore — write

### Failure / Retry Behavior

Errors are logged (per BL entry's "whose error is logged") rather than retried; no retry/backoff policy found in source.

---

## JOB154_SaveTerminalWorkingDirectoryAndTitle: Save Terminal Working Directory And Title

**BL Ref**: BL197
**Type**: queue-worker
**Source**: `crates/terminal_view/src/terminal_view.rs:1724`

### Purpose

Persists an open terminal tab's working directory and custom title so the tab restores its cwd and title correctly on relaunch.

### Schedule / Trigger

Dispatched via the background executor from `TerminalView::serialize` (`crates/terminal_view/src/terminal_view.rs:1724`) during the workspace item serialization pass, for any open terminal tab whose cwd/title changed; skipped when the terminal has an attached task.

### Data Touched

- TerminalDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB155_PipeVimFilterCommandStdin: Pipe Vim Filter Command Stdin

**BL Ref**: BL198
**Type**: queue-worker
**Source**: `crates/vim/src/command.rs:2588`

### Purpose

Feeds the selected buffer range's text into a Vim `:!` filter command's spawned shell process without making the caller wait for the process's combined output to be awaited separately.

### Schedule / Trigger

Dispatched via `cx.background_spawn` inside the `:!` filter-command handler (`crates/vim/src/command.rs:2588`) whenever a Vim filter command is run against a buffer range with piped stdin input; writes text chunks then flushes stdin.

### Data Touched

N/A — no persisted data touched; streams buffer text into a child process's stdin.

### Failure / Retry Behavior

N/A — no retry policy found in source; write/flush errors are logged via `.log_err()` and the write loop stops early on error, without retry.

---

## JOB156_DeleteVimMarkFromDb: Delete Vim Mark From Db

**BL Ref**: BL199
**Type**: queue-worker
**Source**: `crates/vim/src/state.rs:666`

### Purpose

Removes a persisted Vim mark from the database keyed by workspace id, path, and mark name, keeping stored marks eventually consistent with the in-memory state without blocking the keystroke that deleted it.

### Schedule / Trigger

Dispatched via a detached background DB delete from `VimGlobals::delete_mark` (`crates/vim/src/state.rs:666`) whenever the user deletes a Vim mark (buffer-local or global).

### Data Touched

- VimDb — write (delete)

### Failure / Retry Behavior

Errors are logged (per BL entry's "logging any error") rather than retried; no retry/backoff policy found in source.

---

## JOB157_CloneItemOnSplitOffThread: Clone Item On Split Off Thread

**BL Ref**: BL200
**Type**: queue-worker
**Source**: `crates/workspace/src/item.rs:245`

### Purpose

Keeps a pane-split responsive when cloning a heavier active item into the new pane, by running the item's own `clone_on_split` task on the background executor and boxing the resulting handle.

### Schedule / Trigger

Dispatched via a background-spawned wrapper around the `ItemHandle::clone_on_split` implementation (`crates/workspace/src/item.rs:245`) whenever the user splits a pane, requiring the active item to be cloned.

### Data Touched

N/A — no persisted data touched; clones an in-memory item entity handle.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB158_BindSessionToWindowOnFlush: Bind Session To Window On Flush

**BL Ref**: BL201
**Type**: queue-worker
**Source**: `crates/workspace/src/multi_workspace.rs:2399`

### Purpose

Records which session id is bound to which window id in the workspace database as part of the multi-workspace serialization flush, alongside each workspace's own per-workspace serialization task.

### Schedule / Trigger

Dispatched as queued background tasks from `MultiWorkspace::flush_all_serialization` (`crates/workspace/src/multi_workspace.rs:2399`) whenever multi-workspace flushes serialization for all open workspaces (e.g. on window close/quit); one task per workspace with a database id.

### Data Touched

- WorkspaceDb — write

### Failure / Retry Behavior

Errors are logged (per BL entry's "logging errors") rather than retried; no retry/backoff policy found in source.

---

## JOB159_AwaitSpawnedTaskProcessOutput: Await Spawned Task Process Output

**BL Ref**: BL202
**Type**: queue-worker
**Source**: `crates/workspace/src/tasks.rs:54`

### Purpose

Determines a terminal-spawned task's exit status independently of the terminal-provider's own task-spawning context, so the workspace can react to task success/failure asynchronously.

### Schedule / Trigger

Dispatched via the background executor from `Workspace::schedule_resolved_task` (`crates/workspace/src/tasks.rs:54`) whenever a resolved task template is spawned in an integrated terminal and its process handle must be awaited.

### Data Touched

N/A — no persisted data touched; awaits an in-process terminal process handle.

### Failure / Retry Behavior

Success/failure is logged (per BL entry's "logging success/failure") rather than retried; no retry/backoff policy found in source.

---

## JOB160_UpdateWorkspaceActivationTimestamp: Update Workspace Activation Timestamp

**BL Ref**: BL203
**Type**: queue-worker
**Source**: `crates/workspace/src/workspace.rs:6460`

### Purpose

Keeps recent-workspace ordering accurate by recording the workspace's last-activation timestamp whenever its window becomes active, without blocking window-activation handling on the DB write.

### Schedule / Trigger

Dispatched via a detached background DB write from `Workspace::on_window_activation_changed` (`crates/workspace/src/workspace.rs:6460`) whenever a workspace window becomes active; keyed by the workspace's database id.

### Data Touched

- WorkspaceDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB161_SaveWorkspaceWelcomePageState: Save Workspace Welcome Page State

**BL Ref**: BL204
**Type**: queue-worker
**Source**: `crates/workspace/src/welcome.rs:524`

### Purpose

Persists an open Welcome Page tab's item id, workspace id, and a fixed "shown" flag so the tab's presence is restored on relaunch.

### Schedule / Trigger

Dispatched via the background executor from `WelcomePage::serialize` (`crates/workspace/src/welcome.rs:524`) during the workspace item serialization pass, for any open Welcome Page tab.

### Data Touched

- WelcomePagesDb — write

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB162_CreateWorktreeEntryOnDisk: Create Worktree Entry On Disk

**BL Ref**: BL205
**Type**: queue-worker
**Source**: `crates/worktree/src/worktree.rs:845`

### Purpose

Keeps directory/file creation off the calling context when a user creates a new file or directory entry in the project panel, updating the worktree's in-memory entry tree once the write completes.

### Schedule / Trigger

Dispatched via the background executor from `Worktree::create_entry` (`crates/worktree/src/worktree.rs:845`) whenever a user creates a new file or directory entry in the project panel/worktree; runs the filesystem `create_dir`/`write` call.

### Data Touched

N/A — no persisted data touched (outside the worktree's own filesystem entry); updates the in-memory worktree entry tree.

### Failure / Retry Behavior

N/A — no retry policy found in source.

---

## JOB163_WatchThemeFileChanges: Watch Theme File Changes

**BL Ref**: BL206
**Type**: queue-worker
**Source**: `crates/zed/src/main.rs:1543`

### Purpose

Lets external edits to the active theme file take effect live by continuously watching the theme file path and reloading/re-applying the theme registry whenever it changes.

### Schedule / Trigger

Dispatched and detached as a background loop from `watch_themes` (`crates/zed/src/main.rs:1543`) when Zode starts in an environment where the active theme file may change on disk; runs for the life of the application.

### Data Touched

N/A — no persisted data touched; reloads the in-memory theme registry from the watched file.

### Failure / Retry Behavior

N/A — no retry policy found in source; the watch loop simply re-fires on each detected file change.

---

## JOB164_RunKeymapOrSettingsMigration: Run Keymap Or Settings Migration

**BL Ref**: BL207
**Type**: queue-worker
**Source**: `crates/zed/src/zed/migrate.rs:247` (keymap) / `:250` (settings)

### Purpose

Backs up and rewrites the user's keymap or settings config file when the user opts into a migration, then reports success/failure back to the workspace once the write completes.

### Schedule / Trigger

Dispatched via `cx.background_spawn(write_keymap_migration(...))` / `write_settings_migration(...)` (`crates/zed/src/zed/migrate.rs:247`/`:250`) when the user clicks "Backup and Update" in the settings/keymap migration dialog's `on_click` handler.

### Data Touched

N/A — no persisted data touched (via app database); reads/writes the user's on-disk keymap or settings config file.

### Failure / Retry Behavior

N/A — no retry policy found in source; the migration result (success/failure) is surfaced to the workspace once, not retried automatically.


---

## Summary

- **Total Jobs**: 164
- **By Type**: scheduled-job: 2, queue-worker: 62, custom-command: 100

---

## Cross-Reference Validation

- [x] All JOB### codes are unique (file-global — never reset, verified 164/164 no duplicates)
- [x] Every JOB### traces to exactly one BL### in `behavior-logic.md` (no invented jobs)
- [x] No job content duplicates its source BL### entry verbatim (dedup — operational detail only)
- [x] Every job section has a `**Source**` citation (`file:line` or `file`, per DRY boundary)
