<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
<!--
Wave 5 also emits plans/<active-plan>/artifacts/_canonical-fcodes.json and pre-creates
plans/<active-plan>/artifacts/features/{slug}/ folders — orchestrator step, not this draft.
F### numbering below is PROVISIONAL (first-pass, sequential by discovery order); the
orchestrator renumbers for contiguity/canonical slugs after this draft.
-->

# Feature List

**Project**: Zode (Zode Editor fork)
**Generated**: 2026-08-07
**Analysis Scope**: Full monorepo, `generic-source` profile (native Rust/GPUI desktop editor, no
HTTP surface, no ScreenList/RouteList upstream — `screen_source: none`)

**Code Format**: All codes follow `F###_NameSlug` format.

**Feature Types**:

- `ui` — feature has no significant standalone background subsystem (rare here; nearly every
  UI action in this desktop app is backed by some process/store lifecycle)
- `background` — feature has no direct user-facing action, only system lifecycle
- `mixed` — feature has both direct user-facing actions and background logic (the dominant type
  in this codebase)

## Rewrite Note (corrects a fabricated prior pass)

The 2026-07-26 draft of this file built 6 of its 23 features (`F001_ProjectSharing`,
`F002_JoinSharedProject`, `F003_AiAgentChat`, `F004_CollaboratorRoleManagement`,
`F005_VoiceVideoCall`, `F006_InlineEditPrediction`, `F007_LlmProviderSwitching`) around a
collaboration/AI-agent/LiveKit subsystem that **does not exist in this fork** — `crates/collab`,
`crates/call`, `crates/agent*`, `crates/language_model` + provider crates, and
`livekit_api`/`livekit_client` are all absent from the workspace (re-verified this session: `ls
crates` has no `collab`, `call`, `agent`, or `livekit_*` directories). This pass clusters only
from the current, corrected `user-stories.md` (67 US), `behavior-logic.md` (207 BL), `data-model.md`
(18 MODEL), and `permissions-matrix.md` (6 PERM) — no code below cites a fabricated subsystem.

## Adaptation Note (no ScreenList/RouteList upstream)

`screen_source: none` — this is a native desktop app with a `Render`-dispatch UI, not a routed web
surface. Per the template's screen-optional convention, this feature list omits the `Related
Screens` and `Related APIs/Routes` sections entirely rather than fabricating SCR###/ROUTE### codes.
Each feature's `Feature Area` (matching `user-stories.md`'s own column) stands in for a screen
grouping.

## Feature Hierarchy

**Note**: Features are sorted by priority from highest to lowest (P0 → P1 → P2 → P3).

| Code                               | Name                           | Type  | Language | Workspace                                                          | Priority |
| ---------------------------------- | ------------------------------ | ----- | -------- | ------------------------------------------------------------------ | -------- |
| F008_EditorCore                    | Editor Core                    | mixed | Rust     | Zode (crates/editor, text, multi_buffer, workspace)                | P0       |
| F009_Diagnostics                   | Diagnostics                    | mixed | Rust     | Zode (crates/diagnostics, project — LspStore)                      | P0       |
| F010_Debugging                     | Debugging                      | mixed | Rust     | Zode (crates/debugger, dap, project — DapStore)                    | P0       |
| F011_GitIntegration                | Git Integration                | mixed | Rust     | Zode (crates/git, git_ui, project — GitStore)                      | P0       |
| F012_ExtensionSystem               | Extension System               | mixed | Rust     | Zode (crates/extension, extension_host, context_server)            | P0       |
| F013_WorkspaceAndProjectManagement | Workspace & Project Management | mixed | Rust     | Zode (crates/workspace, project, project_panel, sidebar, worktree) | P0       |
| F001_Terminal                      | Terminal                       | mixed | Rust     | Zode (crates/terminal, terminal_view, project — Terminals)         | P0       |
| F002_LanguageIntelligence          | Language Intelligence          | mixed | Rust     | Zode (crates/project — LspStore, lsp, language, toolchain)         | P0       |
| F014_VimEmulation                  | Vim Emulation                  | mixed | Rust     | Zode (crates/vim)                                                  | P1       |
| F015_SettingsAndKeymaps            | Settings & Keymaps             | mixed | Rust     | Zode (crates/settings, theme, keymap_editor, feature_flags)        | P0       |
| F016_Search                        | Search                         | ui    | Rust     | Zode (crates/search, file_finder, project — project search)        | P0       |

## Feature Details

---

### F008_EditorCore: Editor Core

**Type**: mixed
**Description**: The text-editing surface itself — cursor/selection motions, multi-select, text
deletion, split-diff comparison views, encoding/line-ending handling, inlay hints, and the
`MultiBuffer`/`Buffer`/`Editor` stack every other panel (diagnostics, search, git diff) reuses to
render text. This is the one subsystem every other feature in this list ultimately renders through.

**Workspace**: Zode (native GPUI desktop app)
**Languages**: Rust

**Related User Stories**:

- US001_NavigateCursorWithMotions: Navigate cursor with structural motions
- US009_ExtendSelectionToNextMatch: Extend selection to next match
- US010_DeleteTextToLineBoundary: Delete text to line boundary
- US011_ToggleSplitDiffView: Toggle split-diff view

**Related Data Models**:

- MODEL007_TextBuffer
- MODEL008_Buffer
- MODEL009_MultiBuffer
- MODEL010_Editor
- MODEL011_Pane
- MODEL006_EntityHandle (cross-cutting GPUI handle primitive every entity above is stored as)

**Related Background Logic**:

- BL002: CSV preview open action (buffer-adjacent file preview)
- BL031: image viewer zoom controls (image preview buffer)
- BL045: markdown preview copy action
- BL046: markdown preview scroll-sync action
- BL064: SVG open-following-preview action
- BL165: generic detach-and-log-err task helper (cross-cutting executor utility)
- BL166: image viewer path persistence
- BL171: markdown source parsing
- BL172: markdown preview source search
- BL013_EditorCoreActions: bundled cursor/selection/edit action registry (US001, US009, US010)
- BL014_ToggleSplitDiffAction: split-diff style toggle (US011)
- BL015_EncodingSelectorToggleAction: buffer text-encoding switch
- BL044_LineEndingSelectorToggleAction: buffer line-ending switch
- BL126_EditorInlayHintsRefresh: inlay hints re-render on buffer/settings/LSP change
- BL153_ComputeCompletionMenuFilterMatches: completion menu fuzzy filtering
- BL154_SerializeEditorSelections / BL155_SerializeEditorFolds: session-restore serialization
- BL156_ComputeRunnableRanges: gutter run/debug affordances over buffer text
- BL176_LoadBufferContentsOffThread: async buffer load
- BL181_WaitForImageLoad: image-preview buffer load (image_store)
- BL200_CloneItemOnSplitOffThread: pane-split item cloning
- BL030_ActionsMacroDefinition: the `actions!`/`#[derive(Action)]` macro/derive definition site itself (cross-cutting — every custom-command BL across every feature in this list is a consumer of this mechanism)
- BL052_PickerConfirmActions: shared picker-confirm action pattern used by file finder, command palette, and other picker-based UI across the app (cross-cutting infra, anchored here as the generic picker/UI-shell primitive)

**Related Permissions**:

- PERM004_BufferCapabilityGate: `Capability::ReadWrite`/`Read`/`ReadOnly` gates every edit operation on every buffer this feature renders

---

### F009_Diagnostics: Diagnostics

**Type**: mixed
**Description**: Surfaces LSP diagnostics (errors/warnings) both per-buffer and project-wide, and
exposes the last language-server error from the status bar.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US012_OpenBufferDiagnostics: Open buffer diagnostics
- US013_OpenProjectDiagnostics: Open project diagnostics
- US014_ViewLanguageServerErrorStatus: View language server error status

**Related Data Models**:

- MODEL008_Buffer (`diagnostics: TreeMap<LanguageServerId, DiagnosticSet>`)
- MODEL017_LanguageServer (diagnostic source)

**Related Background Logic**:

- BL016: ETW tracing action (Windows perf/diagnostic tooling)
- BL065: copy system specs action (for bug reports)
- BL117: system specs GPU probe
- BL173: miniprofiler report export
- BL011_DeployCurrentFileDiagnosticsAction: buffer-scoped diagnostics view
- BL012_ProjectDiagnosticsActions: project-wide diagnostics aggregation + severity filter
- BL001_ActivityIndicatorStatusActions: status-bar last-error surface/dismiss
- BL032_DumpInputLatencyHistogramAction: dev-tooling action dumping the input-latency histogram for perf diagnosis

---

### F010_Debugging: Debugging

**Type**: mixed
**Description**: Full debug-session lifecycle over the Debug Adapter Protocol — start/stop/step,
breakpoints, watch expressions, variable inspection/editing, remote-process attach, and DAP log
viewing for troubleshooting adapters themselves.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US015_StartDebugSession, US016_StepThroughCodeWhileDebugging, US002_StopDebugSession,
  US003_ClearAllBreakpoints, US004_AddWatchExpression, US005_InspectVariableInDebugPanel,
  US017_EditVariableValueWhileDebugging, US018_AttachDebuggerToRemoteProcess,
  US067_OpenDebugAdapterLogs

**Related Data Models**:

- MODEL003_Project (`dap_store`, `breakpoint_store` fields)

**Related Background Logic**:

- BL005_DebuggerSessionControlActions: start/continue/step/stop/detach
- BL006_NewProcessModalTabActions: launch-config picker
- BL007_BreakpointListNavigationActions
- BL008_ConsoleWatchExpressionAction
- BL009_MemoryViewGoToAddressAction
- BL010_VariableListActions: expand/collapse/copy/edit variable
- BL004_OpenDebugAdapterLogsAction
- BL133_DebugSessionBreakpointObserver: breakpoint-store change dispatch
- BL150_FetchRemoteProcessListForAttach: remote-attach process listing
- BL152_PersistStackFrameFilterPreference
- BL177_ForwardBreakpointToggleToRemote: remote-dev breakpoint sync
- BL178_PruneStaleJsDebugCompanionVersions / BL179_InstallLatestJsDebugCompanion: JS debug adapter companion management
- BL201_BindSessionToWindowOnFlush: session-to-window binding on flush

---

### F011_GitIntegration: Git Integration

**Type**: mixed
**Description**: Git hunk staging/unstaging, branch switching/creation, stash, discard, commit,
project-wide diff review, and commit-graph visualization — all wrapping a local `git` CLI
integration, no server-side collab component.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US019_StageGitHunk, US006_UnstageGitHunk, US007_SwitchGitBranch, US008_CreateGitBranch,
  US020_StashUncommittedChanges, US021_DiscardFileChangesInGitPanel, US022_CommitStagedChanges,
  US023_ViewProjectWideDiff, US024_ViewGitCommitGraph

**Related Data Models**:

- MODEL014_Repository (`GitStore`/`Repository`/`RepositorySnapshot`)

**Related Background Logic**:

- BL144: generic debounce-timer primitive underlying BL145/BL132
- BL021_GitHunkStagingActions, BL022_GitGraphActions, BL023_BranchPickerActions,
  BL024_CommitViewStashActions, BL025_GitPanelActions, BL026_GitPickerTabActions,
  BL027_ProjectDiffActions, BL028_StashPickerActions
- BL106_GitCliRepositoryIntegration: underlying `git` CLI process wrapper
- BL145_GitDiffDebounceRecalculation
- BL132_DebouncedDelayFire: the underlying generic debounce-timer trigger `BL145_GitDiffDebounceRecalculation` builds on
- BL160_ReadGlobalGitCommitterIdentity, BL161_SaveGitGraphSerialization,
  BL162_CreateGitRemote, BL163_CommitStagedChanges, BL164_SaveProjectDiffBase,
  BL180_RestoreGitCheckpoints

---

### F012_ExtensionSystem: Extension System

**Type**: mixed
**Description**: The WASM extension platform — install/reload extensions, author and iterate on a
local dev extension, connect to MCP context servers, and the sandbox capability allowlist
(`ProcessExec`/`DownloadFile`/`NpmInstallPackage`) that fences what an extension can do at runtime.

**Workspace**: Zode
**Languages**: Rust (host), WASM (extension guest)

**Related User Stories**:

- US025_ReloadExtensions, US026_InstallDevExtension, US027_CompileDevExtension,
  US028_RestartContextServer, US029_ConnectToContextServerOverMcp,
  US030_DeclareProcessExecCapability, US031_DeclareDownloadFileCapability,
  US032_DeclareNpmInstallCapability, US033_RejectUndeclaredExtensionCapability

**Related Data Models**:

- MODEL018_ExtensionManifest

**Related Background Logic**:

- BL017_ReloadExtensionsAction, BL018_InstallDevExtensionAction, BL157_CompileDevExtension,
  BL158_DismissLanguageExtensionSuggestion
- BL054_ContextServerRestartAction, BL125_ContextServerNotificationObserver,
  BL148_McpServeConnection

**Related Permissions**:

- PERM001_ExtensionProcessExecCapability (US030, US033)
- PERM002_ExtensionDownloadFileCapability (US031, US033)
- PERM003_ExtensionNpmInstallCapability (US032, US033)

---

### F013_WorkspaceAndProjectManagement: Workspace & Project Management

**Type**: mixed
**Description**: Opening/navigating projects and their worktrees, the project-panel file tree, the
always-visible multi-project sidebar (this fork's rail), the fork-specific idle-hibernation
lifecycle for background projects, dev-container bootstrap, the worktree-trust security gate, and
window-level tab/pane navigation (folded in here rather than as its own thin feature — same
"navigate within an open workspace" intent as project-panel/sidebar navigation, consistent with
KISS).

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US034_OpenRecentProjectFromWelcomeScreen, US035_NavigateProjectPanelEntries,
  US036_CreateFileInProjectPanel, US037_DeleteWorktreeFromPicker,
  US038_ToggleMultiProjectSidebar, US039_SwitchActiveProjectInSidebar,
  US040_HibernateIdleProject, US041_ReactivateHibernatedProject,
  US042_InitializeDevContainerForProject, US043_BuildDevContainerImage,
  US044_RunDevContainerLifecycleScripts, US066_SwitchBetweenOpenTabs

**Related Data Models**:

- MODEL001_MultiWorkspace, MODEL002_Workspace, MODEL003_Project, MODEL004_Worktree,
  MODEL005_Entry, MODEL015_ProjectPanel

**Related Background Logic**:

- BL019: app feedback submission action
- BL035: new journal entry action
- BL049: onboarding flow actions
- BL105: Windows Explorer context-menu shell integration
- BL124: UserStore connection-status observer (account/auth connectivity)
- BL129: global filesystem watcher event dispatch
- BL130: web-window appearance/DPR media-query observer (gpui_web platform backend)
- BL134: prompt-template override directory watcher (Handlebars templates in prompt_store)
- BL135: remote-server projects settings observer
- BL147: component-preview dev-tool active-page persistence
- BL149: local db write+log helper
- BL167: Linux CLI-install prompt action
- BL168: journal entry file creation
- BL174: onboarding active-page persistence
- BL185: remote-dev buffer-update forwarding to remote server
- BL188: remote-dev buffer-update forwarding on headless project
- BL189: remote-dev server stdin read loop
- BL003_InitializeDevContainerAction, BL102_DevContainerJsonParsing,
  BL103_DevContainerManifestBuildAndRun, BL104_DockerCliIntegration
- BL033_InstallCliBinaryAction, BL034_RegisterZedSchemeAction: app-shell CLI/URL-scheme install actions
- BL208_ZedCliPathResolution: resolves/quotes the zed CLI binary path for shell invocation (used by the CLI install flow above)
- BL128_FsWatchStream: filesystem watch event stream backing worktree/project-panel refresh
- BL029_DeleteWorktreeAction, BL055_ProjectPanelActions, BL056_RecentProjectsMenuActions,
  BL057_OpenWslPathAction, BL062_FocusSidebarFilterAction, BL097_WelcomeScreenActions,
  BL141_WorktreeBackgroundScannerObserver, BL186_CopyWorktreeEntryOffThread,
  BL187_DismissDevContainerSuggestion, BL200_CloneItemOnSplitOffThread,
  BL203_UpdateWorkspaceActivationTimestamp, BL204_SaveWorkspaceWelcomePageState,
  BL205_CreateWorktreeEntryOnDisk
- BL094_MultiWorkspaceSidebarActions, BL095_PaneItemManagementActions,
  BL098_WorkspacePaneNavigationActions
- BL047_MenuNavigationActions, BL051_PanelTabNavigationActions,
  BL053_WindowTabManagementActions, BL066_TabSwitcherActions, BL071_ApplicationMenuActivationActions,
  BL072_TitleBarProjectMenuActions, BL099_ZedApplicationWindowActions, BL100_ZedActionsSharedRegistry
- Fork-specific hibernation lifecycle (no dedicated BL###, cited directly per US040/US041's own
  citation): `crates/project/src/project.rs:355,4740,4958`, `crates/project/src/lsp_store.rs:11612`,
  `crates/project/src/prettier_store.rs:118`

**Related Permissions**:

- PERM005_WorktreeTrustGate: gates LSP/git-integration spawn on worktree trust state, surfaced via `SecurityModal`

---

### F001_Terminal: Terminal

**Type**: mixed
**Description**: The embedded terminal (Alacritty-backed) — interactive shell sessions, running
configured `tasks.json` tasks, toggling panel visibility, and searching terminal scrollback.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US045_RunCommandInIntegratedTerminal, US046_ToggleTerminalPanel,
  US047_RunConfiguredTask, US048_SearchTerminalScrollback

**Related Data Models**:

- MODEL016_Terminal

**Related Background Logic**:

- BL067_TerminalCoreActions, BL068_TerminalPanelToggleActions, BL069_TerminalViewTextActions
- BL113_ProjectTerminalShellSpawn, BL114_ProjectTaskTerminalSpawn,
  BL194_BuildTaskContextsOffThread, BL195_SearchTerminalScrollback,
  BL196_SerializeTerminalPanelLayout, BL197_SaveTerminalWorkingDirectoryAndTitle
- BL118_CrossPlatformCommandWrapper, BL119_MacosPosixSpawnCommand,
  BL120_ChildProcessGroupLifecycle, BL121_ShellCommandLineBuilder,
  BL122_LoginShellEnvironmentCapture, BL151_ForwardSpawnedTerminalPid,
  BL202_AwaitSpawnedTaskProcessOutput

---

### F002_LanguageIntelligence: Language Intelligence

**Type**: mixed
**Description**: Language-server process lifecycle, toolchain/language selection per buffer,
formatter (Prettier) integration, and cross-language bridging (e.g. Vue↔TS server forwarding) —
the LSP client layer that Diagnostics (F009) and Editor Core (F008) consume.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US049_SwitchLanguageServerToolchain, US050_SwitchBufferLanguage,
  US051_RestartLanguageServersForBuffer

**Related Data Models**:

- MODEL017_LanguageServer

**Related Background Logic**:

- BL039: syntax-highlights tree view dev tool
- BL043: syntax tree view dev tool
- BL175: outline-panel state serialization (pairs with BL050)
- BL038_LanguageSelectorToggleAction, BL073_ToolchainSelectorActions,
  BL041_LspToolMenuToggleAction, BL042_OpenLanguageServerLogsAction
- BL050_OutlinePanelActions: symbol-outline panel navigation, sourced from LSP document-symbol responses
- BL107_LanguageServerProcessLifecycle, BL108_LspStoreStartLanguageServer,
  BL109_JsonLanguageServerSchemaContentExtension, BL110_RustAnalyzerServerStatusExtension,
  BL111_VueLanguageServerTsServerBridge, BL183_ForwardVueTsRequestToTypescriptServer,
  BL170_SendLspInitializeRequest, BL182_RestartLanguageServersForBuffers
- BL112_PrettierProcessLifecycle, BL184_ClearPrettierCachesOnSettingsChange
- BL131_LspLogViewServerLogObserver, BL136_EditorconfigExternalConfigWatcher,
  BL142_GrammarSourceWatcher, BL191_RefreshPythonKernelspecs, BL192_InstallIpykernelPackage,
  BL115_HeadlessProjectRemoteJupyterKernelSpawn, BL116_NativeJupyterKernelProcessLifecycle,
  BL190_RouteJupyterMessagesToChannels, BL058_ReplSessionActions

---

### F014_VimEmulation: Vim Emulation

**Type**: mixed
**Description**: Full Vim-mode text editing — motions, mode switching, visual selection, ex
commands, macro repeat/record, and text-object operators. Self-contained emulation layer (`crates/vim`)
gated behind `Editor.use_modal_editing`.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US052_NavigateTextWithVimMotions, US053_EnterVimInsertMode,
  US054_SelectTextInVimVisualMode, US055_RunVimExCommand,
  US056_RepeatLastVimChange, US057_SelectVimTextObject

**Related Data Models**:

- MODEL010_Editor (`use_modal_editing` gate field)

**Related Background Logic**:

- BL081_VimMotionActions, BL082_VimNormalModeInsertActions, BL093_VimVisualModeActions,
  BL075_VimExCommandActions, BL085_VimRepeatMacroActions, BL089_VimTextObjectActions
- BL074_VimChangeListNavigationActions, BL076_VimDigraphLiteralAction,
  BL077_VimHelixModeActions, BL078_VimHelixPasteAction, BL079_VimIndentActions,
  BL080_VimInsertModeActions, BL083_VimIncrementDecrementActions, BL084_VimPasteAction,
  BL086_VimScrollActions, BL087_VimSearchActions, BL088_VimSubstituteActions,
  BL090_VimReplaceModeActions, BL091_VimRewrapAction, BL092_VimOperatorPendingPushActions
- BL123_VimShellExecCommand, BL140_VimMarksBufferLifecycleObserver, BL198_PipeVimFilterCommandStdin,
  BL199_DeleteVimMarkFromDb
- _(15 further Vim BL items — increment/decrement, rewrap, replace mode, digraphs, Helix mode,
  change-list, register/mark deletion, shell-filter piping — are structurally identical
  one-keybinding-to-one-action items per `user-stories.md`'s own Limits note; listed above by
  BL### even though not individually storied)_

---

### F015_SettingsAndKeymaps: Settings & Keymaps

**Type**: mixed
**Description**: Live settings/keymap editing, base-keymap preset switching, theme
selection/appearance, and the backup-then-migrate flow that protects a user's configuration across
schema changes on update. Also owns the client-side staff feature-flag gate (an app-wide
configuration concern with no better home in this partition).

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US058_EditSettingsJson, US059_EditKeymapBinding, US060_SwitchBaseKeymapPreset,
  US061_BackupSettingsBeforeMigration, US062_MigrateSettingsToCurrentSchema

**Related Data Models**:

- MODEL012_SettingsStore, MODEL013_Theme

**Related Background Logic**:

- BL040: keymap key-context view action
- BL061_SettingsEditorActions, BL036_KeymapEditorActions, BL048_ToggleBaseKeymapSelectorAction,
  BL207_RunKeymapOrSettingsMigration
- BL037_KeystrokeRecordingActions: keystroke-capture actions used by the keymap editor's binding-recorder UI
- BL070_ThemeSelectorReloadAction, BL096_OpenThemePreviewAction, BL206_WatchThemeFileChanges
- BL063_SnippetsConfigActions, BL139_SnippetDirectoryWatcher
- BL137_ConfigFileWatch, BL138_SettingsStoreFileWatcher, BL143_KeymapAndSettingsObserver,
  BL169_DebounceKeymapActionSearch, BL193_ComputeSettingsExactSearchMatches
- BL101_EncryptedPasswordAtRest: secrets-at-rest for stored credentials referenced from settings-adjacent flows
- BL127_FeatureFlagStoreObserver: reactive flag re-render on staff/flag-value change

**Related Permissions**:

- PERM006_StaffFeatureFlagGate

---

### F016_Search: Search

**Type**: ui
**Description**: Fuzzy file finding, whole-project text search, and in-buffer search — the three
find surfaces of the editor.

**Workspace**: Zode
**Languages**: Rust

**Related User Stories**:

- US063_FindFileByFuzzyName, US064_SearchAcrossProject, US065_SearchWithinCurrentBuffer

**Related Data Models**:

- MODEL009_MultiBuffer (project-search results rendered as a multi-buffer)

**Related Background Logic**:

- BL146: command palette invocation logging
- BL020_FileFinderActions, BL159_CheckRecentHistoryPathExists
- BL059_ProjectSearchActions
- BL060_BufferSearchActions

---

## Summary

- **Total Features**: 11
- **Total Screens**: N/A (`screen_source: none`, no ScreenList upstream)
- **Total User Stories**: 67 (US001–US067, all mapped to a feature above)
- **Total Routes**: N/A (no HTTP surface)
- **Total Data Models**: 18 (MODEL001–MODEL018, all referenced above; MODEL006_EntityHandle is
  cross-cutting and cited once under Editor Core rather than repeated per feature)
- **Total Background Logic**: 208 items in `behavior-logic.md` (BL001–BL208), all 208 mapped to
  at least one feature above. Mapping is pattern-matched by name/domain, not individually
  re-verified per BL### — see Limits.
- **Total Permissions**: 6 (PERM001–PERM006, all mapped)
- **Languages Detected**: Rust (host + GPUI UI), WASM (extension guest runtime only)

## Cross-Reference Validation

- [x] All F### codes are unique
- [x] All F### codes are referenced against `user-stories.md`'s Feature Area column (1:1, no
      invented feature area)
- [x] Related-screens/routes sections omitted rather than fabricated (`screen_source: none`)
- [x] All user story references are valid (every US001–US067 appears under exactly one feature)
- [x] All data model references are valid (every MODEL001–MODEL018 appears under exactly one
      primary feature)
- [x] All permission references are valid (every PERM001–PERM006 appears under exactly one feature)
- [x] Every US has a parent feature (F###)
- [x] Every data model maps to a feature (F###)
- [x] Every permission maps to a feature (F###)
- [x] No AI-agent, collaboration, or LiveKit-based feature reintroduced (verified via
      `grep -i "agent\|collab\|livekit\|language_model"` against this file before finalizing — the one
      hit, `agent_server_store`/`agent_location` fields on `Project` in `data-model.md`, is the external
      ACP agent-server _registry_ contributed by extensions, not an in-repo AI chat subsystem, and is
      not cited as its own feature here; it would belong under F012_ExtensionSystem if storied in a
      future pass)

## Limits

- **Background Logic mapping is pattern-matched by name/domain, not individually re-verified
  per BL###, but coverage IS complete.** All 208 BL items in `behavior-logic.md` (BL001–BL208,
  including BL208 added during the Wave 7a fix cycle for `crates/util/src/util.rs`) are now
  assigned to at least one feature above — the 36 items initially clustered as "dev-only/low-signal
  and deliberately uncovered" (per `user-stories.md`'s narrower story-level scope) were subsequently
  mapped in during the Wave 7a review fix cycle per the mandatory orphan-BL rule in
  `verification-checklist-core-artifacts.md`. Several of these (dev-tooling actions, window-chrome
  actions, remote-dev plumbing) are cross-cutting or low-signal enough that they don't warrant their
  own US###, but every BL### still has a feature home.
- **No independent second source cross-checked the Feature Area groupings** beyond
  `system-overview.md`/`architecture.md`, matching the same limitation `user-stories.md` already
  states for its own Feature Area column — this list inherits that column directly rather than
  re-deriving it.
- **Remote development (SSH/WSL) has no feature of its own.** Its plumbing (BL177, BL185, BL188,
  BL189, BL135) is scattered across Debugging, Git, Workspace, and Language Intelligence above
  rather than surfaced as `F0xx_RemoteDevelopment` — consistent with `user-stories.md`'s own Limits
  note that remote dev was only lightly storied (US018) in this wave. A dedicated remote-dev
  feature spec, if commissioned, should pull these BL items out into their own feature rather than
  leaving them split as they are here.
- **Jupyter/REPL support (BL058, BL115, BL116, BL190–BL192) is folded into Language Intelligence**
  rather than given its own feature — no US directly stories it and it is a small, single-purpose
  cluster; flag for a split if Jupyter functionality grows a dedicated user-story set later.
