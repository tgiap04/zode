<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# User Stories

**Project**: zode (Zed fork)
**Generated**: 2026-08-07
**Analysis Scope**: Full monorepo — native GPUI desktop editor (`generic-source` profile, `screen_source: none`)

**Code Format**: All US codes follow `US###_NameSlug` format (e.g., US001_NavigateCursorWithMotions).

**US Types**:
- `ui` - User-facing stories (developer or extension author interacting with an editor surface/API)
- `system` - System stories: background lifecycle, sandbox enforcement, persistence (no screen mapping)

## Rewrite Note (corrects a fabricated prior pass)

The 2026-07-26 draft of this file invented an AI-agent/collaboration subsystem (`US002_ShareProjectForCollaboration`,
`US003_JoinSharedProject`, `US004_ChangeCollaboratorRole`, `US005_StartVoiceCall`, `US006_ChatWithAiAgent`,
`US007_AcceptInlineEditPrediction`, `US008_SwitchLlmProvider`) that **does not exist in this fork**. Per
`architecture.md`'s correction note: `crates/collab`, `crates/call`, `crates/agent*`, `crates/language_model`
+ provider crates, and `livekit_api`/`livekit_client` are all absent from the workspace (verified against root
`Cargo.toml` members). This pass is derived only from the current `behavior-logic.md` (207 BL items) and
`permissions-matrix.md` (6 PERM items) — every US below cites a real BL### or PERM### code, or (for the two
hibernation stories with no dedicated BL entry) a direct `file:line` citation re-verified against source in
this session.

## Adaptation Note (no ScreenList upstream, `screen_source: none`)

zode is a native desktop app with no routed web surface — no `screen-list.md`/`screen-flow.md` exists upstream.
Per the IPE protocol's headless-profile rule (`references/user-stories-ipe-protocol.md` Step 3), interaction
points are sourced from `behavior-logic.md`'s `custom-command` items (the app's keybinding/command-palette/
context-menu action surface — the functional analogue of "screen interactions" for a `Render`-dispatch desktop
app) plus select `integration`/`observer`/`queue-worker` items that are directly user-reachable, plus
`permissions-matrix.md`'s capability gates. The **Feature Area** column stands in for a Screen field.

**Roles used**: `developer` (the single local user of this desktop editor — its only application-level actor;
no admin/manager/multi-tenant roles exist per `permissions-matrix.md`) and `extension author` (the non-interactive
role that authors a WASM extension manifest consumed by the sandbox — used only for the three capability-
declaration stories, where the actor is genuinely distinct from the editor's end-user).

## Scope and Method

`behavior-logic.md` documents 100 `custom-command` items (BL001–BL100), several of which are themselves
umbrella action registries bundling many single-purpose keybindings under one source file (e.g. `BL013_EditorCoreActions`
lists "dozens" of cursor/selection/edit commands; `BL005_DebuggerSessionControlActions` lists 20+ debugger
commands). Per Step 4 of the IPE protocol (anti-CRUD, exactly one verb per US), a bundling BL is **split** across
multiple US — one per distinct user intent — rather than merged into one "manage X" story. Conversely, closely
related sub-verbs that form one conceptual action for this actor (e.g. Vim's `w`/`e`/`b` motions, or the
debugger's step-over/into/out/back) are treated as **one** US under a single umbrella verb ("navigate", "step
through"), consistent with how the source BL entry itself already groups them as one registered action set —
this mirrors the accepted Vim-motion precedent in the IPE protocol's own merge-exception spirit (same actor,
same handler family, no branching between them).

This pass covers **67 user stories**, a curated, representative selection across every functional domain named
in the Wave 4 task brief (core editing, extension capabilities, workspace/project management incl. multi-project
hibernation, git, LSP/toolchain, debugging, terminal, vim) rather than an exhaustive 1:1 mapping of all 207 BL
items — see **Limits** at the end for what was deliberately left uncovered and why.

## Interaction Inventory

> One row per interactive element mapped to a user story below. `N/A` in Endpoint = no HTTP surface (desktop app).

| Feature Area | Element | Type | Action | Endpoint |
|---|---|---|---|---|
| Editor Core | Motion keybindings (`w`/`e`/next-word etc.) | primary-action | Move cursor by structural unit | N/A |
| Editor Core | Selection keybindings (`SelectNext`/`SelectPrevious`) | primary-action | Extend selection to next/prev match | N/A |
| Editor Core | `DeleteToBeginningOfLine` keybinding | destructive-action | Delete text to line boundary | N/A |
| Editor Core | `ToggleSplitDiff` toolbar/keybinding | secondary-action | Toggle split-diff view style | N/A |
| Diagnostics | `DeployCurrentFile` command | navigation | Open buffer-scoped diagnostics | N/A |
| Diagnostics | `Deploy` (project diagnostics) command | navigation | Open project-wide diagnostics | N/A |
| Diagnostics | Status-bar activity indicator click | secondary-action | View last LSP error message | N/A |
| Debugging | `Start` debugger command | primary-action | Start a debug session | N/A |
| Debugging | `Continue`/`StepInto`/`StepOver`/`StepOut`/`StepBack` | primary-action | Step through code while debugging | N/A |
| Debugging | `Stop`/`Detach` debugger command | destructive-action | Stop a debug session | N/A |
| Debugging | `ClearAllBreakpoints` command | destructive-action | Clear all breakpoints | N/A |
| Debugging | `WatchExpression` console command | primary-action | Add a watch expression | N/A |
| Debugging | Variable-list expand/select | secondary-action | Inspect a variable in the debug panel | N/A |
| Debugging | `EditVariable` command | primary-action | Edit a variable's value while debugging | N/A |
| Debugging | "Attach to Process" modal (remote) | system-action | Attach debugger to a remote process | N/A |
| Git | Hunk "Stage" gutter/context action | primary-action | Stage a git hunk | N/A |
| Git | Hunk "Unstage" gutter/context action | primary-action | Unstage a git hunk | N/A |
| Git | Branch picker entry select | primary-action | Switch git branch | N/A |
| Git | Branch picker "create new branch" | primary-action | Create a git branch | N/A |
| Git | Commit-view stash command | primary-action | Stash uncommitted changes | N/A |
| Git | Git-panel "Discard" context action | destructive-action | Discard file changes in git panel | N/A |
| Git | Git-panel "Commit" button | primary-action | Commit staged changes | N/A |
| Git | Project Diff command | navigation | View project-wide diff | N/A |
| Git | Git Graph tab command | navigation | View git commit graph | N/A |
| Extensions | "Reload Extensions" command | secondary-action | Reload all extensions | N/A |
| Extensions | "Install Dev Extension" button | primary-action | Install a local dev extension | N/A |
| Extensions | Dev-extension file-change trigger | system-action | Compile a dev extension | N/A |
| Extensions | Context-server status UI restart button | primary-action | Restart a context/MCP server | N/A |
| Extensions | MCP client connects to local socket | system-action | Connect to a context server over MCP | N/A |
| Extensions | Extension manifest `capabilities` entry | system-action | Declare a process-exec capability | N/A |
| Extensions | Extension manifest `capabilities` entry | system-action | Declare a download-file capability | N/A |
| Extensions | Extension manifest `capabilities` entry | system-action | Declare an npm-install capability | N/A |
| Extensions | Sandbox capability check (deny path) | system-action | Reject an undeclared capability request | N/A |
| Workspace | Welcome-screen recent-project entry click | navigation | Open a recent project from Welcome | N/A |
| Workspace | Project-panel keybinding/context menu | navigation | Navigate project panel entries | N/A |
| Workspace | Project-panel "New File" action | primary-action | Create a file in the project panel | N/A |
| Workspace | Worktree picker "Delete" | destructive-action | Delete a worktree | N/A |
| Workspace | Sidebar toggle keybinding/click | secondary-action | Toggle the multi-project sidebar | N/A |
| Workspace | Sidebar `NextProject`/`PreviousProject` | navigation | Switch active project in sidebar | N/A |
| Workspace | Idle timer expiry (automatic) | system-action | Hibernate an idle project | N/A |
| Workspace | Click a hibernated project entry | primary-action | Reactivate a hibernated project | N/A |
| Workspace | "Initialize Dev Container" command | primary-action | Initialize a dev container for a project | N/A |
| Workspace | Dev-container open/attach trigger | system-action | Build and run a dev container | N/A |
| Terminal | New terminal panel + keystrokes | primary-action | Run a command in the integrated terminal | N/A |
| Terminal | Terminal panel toggle keybinding | secondary-action | Toggle the terminal panel | N/A |
| Terminal | Task-runner "Run Task" command | primary-action | Run a configured task | N/A |
| Terminal | Cmd-F inside terminal pane | secondary-action | Search terminal scrollback | N/A |
| Language Intelligence | Status-bar toolchain indicator click | secondary-action | Switch language server toolchain | N/A |
| Language Intelligence | Status-bar language indicator click | secondary-action | Switch a buffer's language | N/A |
| Language Intelligence | "Restart Language Servers" command | system-action | Restart language servers for a buffer | N/A |
| Vim | Motion keys (`w`/`e`/`b`/`j`/`k`) | primary-action | Navigate text with Vim motions | N/A |
| Vim | `i`/`a`/`I`/`A`/`o`/`O` keys | primary-action | Enter Vim insert mode | N/A |
| Vim | `v`/`V`/`Ctrl-V` keys | primary-action | Select text in Vim visual mode | N/A |
| Vim | `:`-command line | primary-action | Run a Vim ex command | N/A |
| Vim | `.` repeat key | primary-action | Repeat the last Vim change | N/A |
| Vim | Operator+object combo (`diw`, `ci(`) | primary-action | Select a Vim text object | N/A |
| Settings | Settings editor field edit | primary-action | Edit a setting in settings.json | N/A |
| Settings | Keymap editor binding edit | primary-action | Edit a keymap binding | N/A |
| Settings | "Base Keymap" selector | secondary-action | Switch the base keymap preset | N/A |
| Settings | "Backup and Update" migration dialog button | system-action | Back up and migrate settings on update | N/A |
| Search | File Finder modal | primary-action | Find a file by fuzzy name | N/A |
| Search | Project Search panel | primary-action | Search across the whole project | N/A |
| Search | In-buffer search bar (Cmd-F) | primary-action | Search within the current buffer | N/A |
| App Shell | Ctrl/Cmd+Tab modal | navigation | Switch between open tabs | N/A |

## User Story Index

| Code | Title | Type | Priority | Feature Area |
|---|---|---|---|---|
| US001_NavigateCursorWithMotions | Navigate cursor with structural motions | ui | must | Editor Core |
| US009_ExtendSelectionToNextMatch | Extend selection to next match | ui | should | Editor Core |
| US010_DeleteTextToLineBoundary | Delete text to line boundary | ui | must | Editor Core |
| US011_ToggleSplitDiffView | Toggle split-diff view | ui | should | Editor Core |
| US012_OpenBufferDiagnostics | Open buffer diagnostics | ui | must | Diagnostics |
| US013_OpenProjectDiagnostics | Open project diagnostics | ui | must | Diagnostics |
| US014_ViewLanguageServerErrorStatus | View language server error status | ui | should | Diagnostics |
| US015_StartDebugSession | Start a debug session | ui | must | Debugging |
| US016_StepThroughCodeWhileDebugging | Step through code while debugging | ui | must | Debugging |
| US002_StopDebugSession | Stop a debug session | ui | must | Debugging |
| US003_ClearAllBreakpoints | Clear all breakpoints | ui | should | Debugging |
| US004_AddWatchExpression | Add a watch expression | ui | should | Debugging |
| US005_InspectVariableInDebugPanel | Inspect a variable in the debug panel | ui | should | Debugging |
| US017_EditVariableValueWhileDebugging | Edit a variable's value while debugging | ui | should | Debugging |
| US018_AttachDebuggerToRemoteProcess | Attach debugger to a remote process | ui | should | Debugging |
| US019_StageGitHunk | Stage a git hunk | ui | must | Git |
| US006_UnstageGitHunk | Unstage a git hunk | ui | must | Git |
| US007_SwitchGitBranch | Switch git branch | ui | must | Git |
| US008_CreateGitBranch | Create a git branch | ui | should | Git |
| US020_StashUncommittedChanges | Stash uncommitted changes | ui | should | Git |
| US021_DiscardFileChangesInGitPanel | Discard file changes in git panel | ui | must | Git |
| US022_CommitStagedChanges | Commit staged changes | ui | must | Git |
| US023_ViewProjectWideDiff | View project-wide diff | ui | should | Git |
| US024_ViewGitCommitGraph | View git commit graph | ui | should | Git |
| US025_ReloadExtensions | Reload all extensions | ui | should | Extensions |
| US026_InstallDevExtension | Install a local dev extension | ui | should | Extensions |
| US027_CompileDevExtension | Compile a dev extension | system | should | Extensions |
| US028_RestartContextServer | Restart a context/MCP server | ui | should | Extensions |
| US029_ConnectToContextServerOverMcp | Connect to a context server over MCP | system | should | Extensions |
| US030_DeclareProcessExecCapability | Declare a process-exec capability | ui | must | Extensions |
| US031_DeclareDownloadFileCapability | Declare a download-file capability | ui | should | Extensions |
| US032_DeclareNpmInstallCapability | Declare an npm-install capability | ui | should | Extensions |
| US033_RejectUndeclaredExtensionCapability | Reject an undeclared capability request | system | must | Extensions |
| US034_OpenRecentProjectFromWelcomeScreen | Open a recent project from Welcome | ui | must | Workspace |
| US035_NavigateProjectPanelEntries | Navigate project panel entries | ui | must | Workspace |
| US036_CreateFileInProjectPanel | Create a file in the project panel | ui | must | Workspace |
| US037_DeleteWorktreeFromPicker | Delete a worktree | ui | should | Workspace |
| US038_ToggleMultiProjectSidebar | Toggle the multi-project sidebar | ui | must | Workspace |
| US039_SwitchActiveProjectInSidebar | Switch active project in sidebar | ui | must | Workspace |
| US040_HibernateIdleProject | Hibernate an idle project | system | should | Workspace |
| US041_ReactivateHibernatedProject | Reactivate a hibernated project | ui | must | Workspace |
| US042_InitializeDevContainerForProject | Initialize a dev container | ui | should | Workspace |
| US043_BuildDevContainerImage | Build a dev container image | system | should | Workspace |
| US044_RunDevContainerLifecycleScripts | Run a dev container's lifecycle scripts | system | should | Workspace |
| US045_RunCommandInIntegratedTerminal | Run a command in the integrated terminal | ui | must | Terminal |
| US046_ToggleTerminalPanel | Toggle the terminal panel | ui | must | Terminal |
| US047_RunConfiguredTask | Run a configured task | ui | must | Terminal |
| US048_SearchTerminalScrollback | Search terminal scrollback | ui | should | Terminal |
| US049_SwitchLanguageServerToolchain | Switch language server toolchain | ui | should | Language Intelligence |
| US050_SwitchBufferLanguage | Switch a buffer's language | ui | should | Language Intelligence |
| US051_RestartLanguageServersForBuffer | Restart language servers for a buffer | system | should | Language Intelligence |
| US052_NavigateTextWithVimMotions | Navigate text with Vim motions | ui | must | Vim |
| US053_EnterVimInsertMode | Enter Vim insert mode | ui | must | Vim |
| US054_SelectTextInVimVisualMode | Select text in Vim visual mode | ui | must | Vim |
| US055_RunVimExCommand | Run a Vim ex command | ui | should | Vim |
| US056_RepeatLastVimChange | Repeat the last Vim change | ui | should | Vim |
| US057_SelectVimTextObject | Select a Vim text object | ui | should | Vim |
| US058_EditSettingsJson | Edit a setting in settings.json | ui | must | Settings |
| US059_EditKeymapBinding | Edit a keymap binding | ui | must | Settings |
| US060_SwitchBaseKeymapPreset | Switch the base keymap preset | ui | should | Settings |
| US061_BackupSettingsBeforeMigration | Back up settings before a schema migration | system | must | Settings |
| US062_MigrateSettingsToCurrentSchema | Migrate settings to the current schema | system | must | Settings |
| US063_FindFileByFuzzyName | Find a file by fuzzy name | ui | must | Search |
| US064_SearchAcrossProject | Search across the whole project | ui | must | Search |
| US065_SearchWithinCurrentBuffer | Search within the current buffer | ui | must | Search |
| US066_SwitchBetweenOpenTabs | Switch between open tabs | ui | must | App Shell |
| US067_OpenDebugAdapterLogs | Open Debug Adapter Protocol logs | ui | could | Debugging |

---

## US001_NavigateCursorWithMotions: Navigate cursor with structural motions

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Editor Core

### User Story
As a developer, I want to move my cursor by structural units (word, line, page) so that I can navigate a file without reaching for the mouse.

### Acceptance Criteria
- [ ] Motion keybindings from `keymap.json` move the cursor without altering buffer content.
- [ ] Page-up/page-down motions keep the cursor within the visible viewport bounds after scrolling.

### Background Logic
- BL013_EditorCoreActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Cursor is mid-line in an open buffer | Developer presses `MoveToBeginningOfLine` | Cursor moves to column 0 of the current line, buffer text unchanged |

---

## US009_ExtendSelectionToNextMatch: Extend selection to next match

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Editor Core

### User Story
As a developer, I want to extend my selection to the next occurrence of the selected text so that I can multi-select and edit repeated identifiers quickly.

### Acceptance Criteria
- [ ] `SelectNext` adds a new selection at the next match without losing existing selections.
- [ ] `SelectPrevious` behaves symmetrically in the reverse direction.

### Background Logic
- BL013_EditorCoreActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A word is selected, and it recurs later in the buffer | Developer triggers `SelectNext` | A second cursor/selection is added at the next occurrence |

---

## US010_DeleteTextToLineBoundary: Delete text to line boundary

**Type**: ui
**Interaction**: destructive-action
**Priority**: must
**Feature Area**: Editor Core

### User Story
As a developer, I want to delete text from my cursor to the beginning of the line so that I can clear a partial line without selecting it manually.

### Acceptance Criteria
- [ ] `DeleteToBeginningOfLine` removes exactly the text between the line start and the cursor.
- [ ] The deletion is a single undoable operation.

### Background Logic
- BL013_EditorCoreActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Cursor sits after 10 characters of indentation+text on a line | Developer triggers `DeleteToBeginningOfLine` | Those 10 characters are removed and `Undo` restores them in one step |

---

## US011_ToggleSplitDiffView: Toggle split-diff view

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Editor Core

### User Story
As a developer, I want to toggle the diff style of a split editor so that I can compare panes in the layout that's easiest to read for the change at hand.

### Acceptance Criteria
- [ ] `ToggleSplitDiff` switches the `SplittableEditor`'s diff rendering between its two supported styles.
- [ ] The toggle persists for that editor instance until toggled again or the editor is closed.

### Background Logic
- BL014_ToggleSplitDiffAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A split editor is open comparing two buffers | Developer triggers `ToggleSplitDiff` | The diff presentation style flips to its alternate mode |

---

## US012_OpenBufferDiagnostics: Open buffer diagnostics

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Feature Area**: Diagnostics

### User Story
As a developer, I want to open a diagnostics view scoped to my current file so that I can review its errors/warnings without the noise of the whole project.

### Acceptance Criteria
- [ ] `DeployCurrentFile` opens a diagnostics editor showing only excerpts around diagnostics in the focused buffer.
- [ ] The view updates as new diagnostics arrive for that buffer.

### Background Logic
- BL011_DeployCurrentFileDiagnosticsAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | The focused buffer has 3 LSP diagnostics | Developer triggers `DeployCurrentFile` | A diagnostics pane opens showing exactly those 3 excerpts |

---

## US013_OpenProjectDiagnostics: Open project diagnostics

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Feature Area**: Diagnostics

### User Story
As a developer, I want to open a project-wide diagnostics view so that I can see every error/warning across all open language servers at once.

### Acceptance Criteria
- [ ] `Deploy` opens the project diagnostics view aggregating all worktrees' diagnostics.
- [ ] `ToggleWarnings` hides/shows warning-severity diagnostics without affecting errors.

### Background Logic
- BL012_ProjectDiagnosticsActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Multiple files across the project have diagnostics | Developer triggers `Deploy` | A single list aggregates diagnostics from every affected file |

---

## US014_ViewLanguageServerErrorStatus: View language server error status

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Diagnostics

### User Story
As a developer, I want to view the last language-server error from the status bar so that I know a language server has failed without digging through logs.

### Acceptance Criteria
- [ ] `ShowErrorMessage` surfaces the most recent LSP error text from the activity indicator.
- [ ] `DismissMessage` clears the surfaced error without affecting the underlying language server state.

### Background Logic
- BL001_ActivityIndicatorStatusActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A language server crashed and logged an error | Developer clicks the status-bar activity indicator | The last error message is shown in a status-bar popover |

---

## US015_StartDebugSession: Start a debug session

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Debugging

### User Story
As a developer, I want to start a debug session for my project so that I can run my program under a debugger with breakpoints active.

### Acceptance Criteria
- [ ] `Start` launches a `DebugSession` using the configured debug adapter for the active launch configuration.
- [ ] A running session becomes visible in the debugger UI (console, variables, breakpoint list panes).

### Background Logic
- BL005_DebuggerSessionControlActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A valid debug launch configuration exists for the project | Developer triggers `Start` | A `DebugSession` starts and the debugger panes populate |
| Error Case | The debug adapter binary is missing/misconfigured | Developer triggers `Start` | Session start fails with a diagnostic message; no session panes are shown as active |

---

## US016_StepThroughCodeWhileDebugging: Step through code while debugging

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Debugging

### User Story
As a developer, I want to step through my program's execution one line/call/instruction at a time so that I can trace exactly where a bug occurs.

### Acceptance Criteria
- [ ] `Continue`, `StepInto`, `StepOver`, `StepOut`, and `StepBack` each advance/rewind the active session by the documented unit and pause it again at the next stop point.
- [ ] The editor's current-line indicator and the variable list refresh to reflect the new stack frame after each step.

### Background Logic
- BL005_DebuggerSessionControlActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A debug session is paused at a breakpoint | Developer triggers `StepOver` | Execution advances past the current line and pauses again at the next statement in the same frame |

---

## US002_StopDebugSession: Stop a debug session

**Type**: ui
**Interaction**: destructive-action
**Priority**: must
**Feature Area**: Debugging

### User Story
As a developer, I want to stop a running debug session so that I can terminate the debuggee and free the debugger UI for a new run.

### Acceptance Criteria
- [ ] `Stop` terminates the debuggee process (or `Detach` leaves it running if the adapter supports detach).
- [ ] The debugger panes clear their session-scoped state (variables, call stack) once the session ends.

### Background Logic
- BL005_DebuggerSessionControlActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A debug session is running | Developer triggers `Stop` | The debuggee process terminates and the session is removed from the debugger UI |

---

## US003_ClearAllBreakpoints: Clear all breakpoints

**Type**: ui
**Interaction**: destructive-action
**Priority**: should
**Feature Area**: Debugging

### User Story
As a developer, I want to clear every breakpoint in the project so that I can start a clean debugging pass without hunting down stray breakpoints one by one.

### Acceptance Criteria
- [ ] `ClearAllBreakpoints` removes every breakpoint tracked by the `BreakpointStore`, across all files.
- [ ] A subsequent debug session run does not stop at any previously-set location.

### Background Logic
- BL005_DebuggerSessionControlActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | 5 breakpoints exist across 3 files | Developer triggers `ClearAllBreakpoints` | All 5 breakpoints are removed and the breakpoint list panel shows empty |

---

## US004_AddWatchExpression: Add a watch expression

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Debugging

### User Story
As a developer, I want to add an expression to the debugger's watch list so that its value is continuously evaluated as I step through code.

### Acceptance Criteria
- [ ] `WatchExpression` adds the currently selected/typed expression from the console to the watch list.
- [ ] The watched expression re-evaluates and updates on every subsequent stop/step.

### Background Logic
- BL008_ConsoleWatchExpressionAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Developer types `myVar.count` in the debug console | Developer triggers `WatchExpression` | `myVar.count` appears in the watch panel and updates on the next step |

---

## US005_InspectVariableInDebugPanel: Inspect a variable in the debug panel

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Debugging

### User Story
As a developer, I want to expand a variable in the debugger's variable list so that I can inspect nested fields of a struct/object while paused.

### Acceptance Criteria
- [ ] `ExpandSelectedEntry` reveals the child fields of the selected variable; `CollapseSelectedEntry` hides them again.
- [ ] `CopyVariableValue` places the currently displayed value text on the clipboard.

### Background Logic
- BL010_VariableListActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A paused session shows a struct-typed variable | Developer triggers `ExpandSelectedEntry` | The variable's fields are listed as child rows beneath it |

---

## US017_EditVariableValueWhileDebugging: Edit a variable's value while debugging

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Debugging

### User Story
As a developer, I want to edit a variable's value while paused in the debugger so that I can test a different runtime state without restarting the program.

### Acceptance Criteria
- [ ] `EditVariable` sends the debug adapter a set-variable request for the selected variable with the new value.
- [ ] The variable list reflects the updated value once the adapter confirms the write.

### Background Logic
- BL010_VariableListActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A paused session shows an integer variable `count = 3` | Developer edits it to `10` via `EditVariable` | The adapter accepts the write and the panel shows `count = 10` |
| Error Case | The debug adapter rejects the set-variable request (e.g. read-only binding) | Developer edits the value | The panel keeps the original value and surfaces the adapter's rejection |

---

## US018_AttachDebuggerToRemoteProcess: Attach debugger to a remote process

**Type**: ui
**Interaction**: system-action
**Priority**: should
**Feature Area**: Debugging

### User Story
As a developer, I want to attach the debugger to a process already running on my remote (SSH) project so that I can debug a long-lived service without restarting it.

### Acceptance Criteria
- [ ] Opening the "Attach to Process" modal for a remote project fetches the live process list from the remote host.
- [ ] Selecting a process starts a debug session attached to that PID on the remote host.

### Background Logic
- BL150_FetchRemoteProcessListForAttach

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A remote SSH project has a running target process | Developer opens Attach-to-Process and selects it | A debug session attaches to that remote PID |

---

## US019_StageGitHunk: Stage a git hunk

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Git

### User Story
As a developer, I want to stage a single git hunk from the editor gutter so that I can build a commit out of only part of a file's changes.

### Acceptance Criteria
- [ ] Triggering the stage action on a hunk adds only that hunk's lines to the git index.
- [ ] Other unstaged hunks in the same file remain unstaged.

### Background Logic
- BL021_GitHunkStagingActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A file has 2 unstaged hunks | Developer stages hunk 1 from the gutter | `git status` shows hunk 1 staged and hunk 2 still unstaged |

---

## US006_UnstageGitHunk: Unstage a git hunk

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Git

### User Story
As a developer, I want to unstage a single git hunk so that I can remove it from the next commit without discarding the underlying edit.

### Acceptance Criteria
- [ ] Triggering the unstage action on a staged hunk removes only that hunk from the index.
- [ ] The working-tree content of the hunk is left untouched.

### Background Logic
- BL021_GitHunkStagingActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A hunk is currently staged | Developer unstages it | The hunk returns to the "unstaged changes" section, file content unchanged |

---

## US007_SwitchGitBranch: Switch git branch

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Git

### User Story
As a developer, I want to switch to a different local git branch from the branch picker so that I can work on another line of development.

### Acceptance Criteria
- [ ] Selecting a branch in the picker checks it out in the current repository's worktree.
- [ ] The git panel/status-bar branch label updates to the newly checked-out branch.

### Background Logic
- BL023_BranchPickerActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Working tree is clean on branch `main`, `feature-x` exists | Developer selects `feature-x` in the branch picker | Repository checks out `feature-x`; status bar reflects it |
| Error Case | Working tree has uncommitted changes that conflict with the target branch | Developer selects a different branch | Checkout is refused/prompts the developer rather than silently discarding changes |

---

## US008_CreateGitBranch: Create a git branch

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Git

### User Story
As a developer, I want to create a new git branch from the branch picker so that I can start isolated work without leaving the editor.

### Acceptance Criteria
- [ ] Entering a new branch name and confirming creates and checks out that branch from the current HEAD.
- [ ] The new branch is immediately selectable in the branch picker on reopen.

### Background Logic
- BL023_BranchPickerActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Developer types a novel branch name in the picker's create flow | Developer confirms | A new branch is created at current HEAD and checked out |

---

## US020_StashUncommittedChanges: Stash uncommitted changes

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Git

### User Story
As a developer, I want to stash my uncommitted changes so that I can switch context to a clean working tree without losing my in-progress edits.

### Acceptance Criteria
- [ ] The stash action captures all uncommitted (staged + unstaged) changes into a new stash entry.
- [ ] The working tree returns to matching HEAD immediately after stashing.

### Background Logic
- BL024_CommitViewStashActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | 3 files have uncommitted edits | Developer triggers stash | A stash entry is created and the working tree matches HEAD |

---

## US021_DiscardFileChangesInGitPanel: Discard file changes in git panel

**Type**: ui
**Interaction**: destructive-action
**Priority**: must
**Feature Area**: Git

### User Story
As a developer, I want to discard a file's uncommitted changes from the git panel so that I can revert it to its last-committed state.

### Acceptance Criteria
- [ ] The discard action reverts the selected file's working-tree content to match HEAD (or the index, if staged).
- [ ] The discarded file no longer appears in the git panel's changed-files list.

### Background Logic
- BL025_GitPanelActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A file has unstaged edits | Developer triggers "Discard" on it from the git panel | The file's content reverts to HEAD and it disappears from the changed-files list |

---

## US022_CommitStagedChanges: Commit staged changes

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Git

### User Story
As a developer, I want to commit my staged changes with a message so that I can record a checkpoint in the repository's history.

### Acceptance Criteria
- [ ] Confirming a commit in the git panel with staged changes present creates a new commit on the current branch with the entered message.
- [ ] The staged-changes list is cleared once the commit succeeds.

### Background Logic
- BL163_CommitStagedChanges
- BL025_GitPanelActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | 2 hunks are staged and a commit message is entered | Developer confirms commit | A new commit is created containing exactly those 2 hunks |
| Error Case | Nothing is staged | Developer attempts to commit | Commit is blocked/no-ops rather than creating an empty commit silently |

---

## US023_ViewProjectWideDiff: View project-wide diff

**Type**: ui
**Interaction**: navigation
**Priority**: should
**Feature Area**: Git

### User Story
As a developer, I want to view a combined diff of every changed file in the project so that I can review my whole working-tree change set in one place before committing.

### Acceptance Criteria
- [ ] The Project Diff view lists every file with uncommitted changes and their hunks.
- [ ] Selecting a hunk in the Project Diff view jumps to that hunk's location in the corresponding buffer.

### Background Logic
- BL027_ProjectDiffActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | 4 files across the project have uncommitted changes | Developer opens Project Diff | All 4 files' diffs are listed in one view |

---

## US024_ViewGitCommitGraph: View git commit graph

**Type**: ui
**Interaction**: navigation
**Priority**: should
**Feature Area**: Git

### User Story
As a developer, I want to view the repository's commit graph so that I can understand branch/merge history visually.

### Acceptance Criteria
- [ ] Opening the Git Graph tab renders commits and their parent/child edges for the current repository.
- [ ] The graph reflects new commits after a refresh/on the panel regaining focus.

### Background Logic
- BL022_GitGraphActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Repository has 3 branches with a recent merge | Developer opens Git Graph | The merge point and both parent branches are rendered |

---

## US025_ReloadExtensions: Reload all extensions

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As a developer, I want to reload all installed extensions so that a newly installed or updated extension takes effect without restarting the editor.

### Acceptance Criteria
- [ ] The reload action tears down and re-initializes every loaded WASM extension instance.
- [ ] Extensions that fail to reload report an error without crashing the host process.

### Background Logic
- BL017_ReloadExtensionsAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | An extension was just updated on disk | Developer triggers "Reload Extensions" | The new extension version is loaded and active |

---

## US026_InstallDevExtension: Install a local dev extension

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As a developer, I want to install a locally-authored extension directory so that I can test it inside the editor before publishing.

### Acceptance Criteria
- [ ] The "Install Dev Extension" action loads the selected local directory as a dev extension.
- [ ] The dev extension appears active on the Extensions page, distinguished from published extensions.

### Background Logic
- BL018_InstallDevExtensionAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A valid local extension directory with `extension.toml` exists | Developer selects it via "Install Dev Extension" | The extension loads and is marked as a dev extension |

---

## US027_CompileDevExtension: Compile a dev extension

**Type**: system
**Interaction**: system-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As a developer, I want a locally-installed dev extension to rebuild automatically when I trigger a rebuild so that I can iterate on its Rust/WASM source without a manual build step.

### Acceptance Criteria
- [ ] Triggering rebuild of a dev extension recompiles its source to WASM and reloads it in place.
- [ ] A compile error is surfaced to the developer rather than silently keeping the stale build loaded.

### Background Logic
- BL157_CompileDevExtension

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Dev extension source was edited since last build | Developer triggers rebuild | The extension recompiles and the running instance reflects the new code |

---

## US028_RestartContextServer: Restart a context/MCP server

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As a developer, I want to restart a context server (MCP) from its status UI so that I can recover it after a crash or configuration change without restarting the editor.

### Acceptance Criteria
- [ ] The restart action tears down the existing context-server connection and re-establishes it.
- [ ] The context-server status UI reflects the new connection state once restarted.

### Background Logic
- BL054_ContextServerRestartAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A context server's connection has dropped | Developer triggers restart from the status UI | The connection re-establishes and the status UI shows it connected |

---

## US029_ConnectToContextServerOverMcp: Connect to a context server over MCP

**Type**: system
**Interaction**: system-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As a developer, I want an MCP client to be able to connect to zode's local context-server socket so that external MCP tooling can interact with my editor session.

### Acceptance Criteria
- [ ] A local Unix-socket MCP client connection is accepted by the context-server listener.
- [ ] Incoming JSON-RPC notifications on that connection are dispatched to the appropriate subscriber.

### Background Logic
- BL148_McpServeConnection
- BL125_ContextServerNotificationObserver

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | The context-server listener is active | An MCP client connects to its Unix socket | The connection is accepted and notifications begin dispatching |

---

## US030_DeclareProcessExecCapability: Declare a process-exec capability

**Type**: ui
**Interaction**: system-action
**Priority**: must
**Feature Area**: Extensions

### User Story
As an extension author, I want to declare a `ProcessExec` capability with a command+args match rule in my extension manifest so that my extension can spawn that specific external process at runtime.

### Acceptance Criteria
- [ ] A declared `ProcessExec` capability whose command+args rule matches a requested spawn is allowed.
- [ ] Wildcard and double-wildcard argument matching (per `allow_exec`'s documented rules) resolve as declared.

### Background Logic
- PERM001_ExtensionProcessExecCapability

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Manifest declares `ProcessExec` for `git` with wildcard args | Extension calls `allow_exec("git", ["status"])` | The call is permitted and the process spawns |

---

## US031_DeclareDownloadFileCapability: Declare a download-file capability

**Type**: ui
**Interaction**: system-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As an extension author, I want to declare a `DownloadFile` capability for a specific host in my manifest so that my extension can fetch assets from that host at runtime.

### Acceptance Criteria
- [ ] A declared `DownloadFile` capability entry matching the requested host permits the sandboxed fetch.
- [ ] A request to a host not covered by any declared entry is rejected before the fetch is attempted.

### Background Logic
- PERM002_ExtensionDownloadFileCapability

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Manifest declares `DownloadFile` for `github.com` | Extension requests a download from `github.com` | The fetch proceeds |

---

## US032_DeclareNpmInstallCapability: Declare an npm-install capability

**Type**: ui
**Interaction**: system-action
**Priority**: should
**Feature Area**: Extensions

### User Story
As an extension author, I want to declare an `NpmInstallPackage` capability for a specific package in my manifest so that my extension can install that npm dependency at runtime.

### Acceptance Criteria
- [ ] A declared `NpmInstallPackage` capability entry matching the requested package permits the install.
- [ ] A request for a package with no matching declared entry is rejected before install is attempted.

### Background Logic
- PERM003_ExtensionNpmInstallCapability

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Manifest declares `NpmInstallPackage` for `pyright` | Extension requests installing `pyright` | The install proceeds |

---

## US033_RejectUndeclaredExtensionCapability: Reject an undeclared capability request

**Type**: system
**Interaction**: system-action
**Priority**: must
**Feature Area**: Extensions

### User Story
As a developer, I want an extension's undeclared capability request to be denied at runtime so that a buggy or malicious extension cannot silently exceed what it declared in its manifest.

### Acceptance Criteria
- [ ] A `ProcessExec`/`DownloadFile`/`NpmInstallPackage` request with no matching declared capability entry errors out before the underlying operation runs.
- [ ] The denial does not crash the host process — the extension call site receives an error result.

### Background Logic
- PERM001_ExtensionProcessExecCapability
- PERM002_ExtensionDownloadFileCapability
- PERM003_ExtensionNpmInstallCapability

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Error Case | Manifest declares no `ProcessExec` capability | Extension calls `allow_exec("curl", [...])` | The call errors before any process is spawned |

---

## US034_OpenRecentProjectFromWelcomeScreen: Open a recent project from Welcome

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Feature Area**: Workspace

### User Story
As a developer, I want to click a recent-project entry on the welcome screen so that I can quickly reopen a project I worked on before.

### Acceptance Criteria
- [ ] Clicking a recent-project entry opens that project's worktree(s) in a workspace.
- [ ] The welcome screen entry reflects the project's last-known path even if the folder has since moved (surfacing an error if it no longer exists).

### Background Logic
- BL097_WelcomeScreenActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A previously-opened project appears in the recent list | Developer clicks its entry | The project opens in a new/current workspace |

---

## US035_NavigateProjectPanelEntries: Navigate project panel entries

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Feature Area**: Workspace

### User Story
As a developer, I want to navigate the project panel's file tree with the keyboard so that I can move between files without leaving the keyboard.

### Acceptance Criteria
- [ ] Keybindings move the selected entry up/down/into/out of folders in the tree.
- [ ] Confirming a selected file entry opens it in the active pane.

### Background Logic
- BL055_ProjectPanelActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Project panel is focused with a folder collapsed | Developer navigates to it and expands it | The folder's children become visible and selectable |

---

## US036_CreateFileInProjectPanel: Create a file in the project panel

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Workspace

### User Story
As a developer, I want to create a new file directly from the project panel so that I can add files to my project without leaving the editor.

### Acceptance Criteria
- [ ] The "new file" action creates an empty file entry at the selected location in the worktree.
- [ ] The new file is immediately opened for editing/renaming inline.

### Background Logic
- BL205_CreateWorktreeEntryOnDisk

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A folder is selected in the project panel | Developer triggers "New File" | A new empty file appears under that folder and is opened for naming |

---

## US037_DeleteWorktreeFromPicker: Delete a worktree

**Type**: ui
**Interaction**: destructive-action
**Priority**: should
**Feature Area**: Workspace

### User Story
As a developer, I want to delete a worktree from the worktree picker so that I can remove a folder I no longer want open in this project.

### Acceptance Criteria
- [ ] Deleting a worktree removes it from the current project's set of open worktrees.
- [ ] Deleting a worktree does not delete the underlying folder from disk.

### Background Logic
- BL029_DeleteWorktreeAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A project has 2 worktrees open | Developer deletes one from the picker | The project now shows only the remaining worktree; the deleted folder still exists on disk |

---

## US038_ToggleMultiProjectSidebar: Toggle the multi-project sidebar

**Type**: ui
**Interaction**: secondary-action
**Priority**: must
**Feature Area**: Workspace

### User Story
As a developer, I want to toggle the always-visible project rail so that I can reclaim screen space when I don't need to switch projects.

### Acceptance Criteria
- [ ] `ToggleWorkspaceSidebar` hides the sidebar when visible and shows it when hidden.
- [ ] The sidebar's visibility state persists across window restarts.

### Background Logic
- BL094_MultiWorkspaceSidebarActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Sidebar is visible | Developer triggers toggle | Sidebar hides and the editor pane reclaims its space |

---

## US039_SwitchActiveProjectInSidebar: Switch active project in sidebar

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Feature Area**: Workspace

### User Story
As a developer, I want to switch to the next/previous open project from the sidebar so that I can move between concurrently-open projects in the same window.

### Acceptance Criteria
- [ ] `NextProject`/`PreviousProject` cycles the window's active project among currently open projects.
- [ ] The workspace pane content updates to reflect the newly active project.

### Background Logic
- BL094_MultiWorkspaceSidebarActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Two projects are open in the same window | Developer triggers `NextProject` | The second project becomes active and its panes render |

---

## US040_HibernateIdleProject: Hibernate an idle project

**Type**: system
**Interaction**: system-action
**Priority**: should
**Feature Area**: Workspace

### User Story
As a developer, I want an inactive project's language servers, terminals, and other resources to be torn down automatically so that I can keep many projects open without them consuming resources indefinitely.

### Acceptance Criteria
- [ ] A project idle past its configured timer transitions `Active → Warm → Hibernated` and its resource layer (LSP, terminals, prettier, git store) is torn down or deferred.
- [ ] The `Workspace`/`Project` entities and their on-disk session record persist through hibernation so the project can be reopened without a full re-scan.

### Background Logic
- No dedicated BL### exists for this fork-specific feature; cited directly per the headless-IPE rule: `crates/project/src/project.rs:355` (`ProjectActivity::Hibernated`), `crates/project/src/project.rs:4740` (`set_activity`), `crates/project/src/lsp_store.rs:11612` (`LspStore::hibernate`), `crates/project/src/prettier_store.rs:118` (`PrettierStore::hibernate`)

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A project has been inactive past its idle timer | The timer fires | The project's activity moves to `Hibernated` and its LSP/terminal/prettier resources are torn down or deferred behind a barrier |

---

## US041_ReactivateHibernatedProject: Reactivate a hibernated project

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Workspace

### User Story
As a developer, I want to click a hibernated project in the sidebar so that it wakes back up and becomes usable again without a full re-open.

### Acceptance Criteria
- [ ] Selecting a hibernated project's sidebar entry triggers `wake_resources`, restoring its torn-down resources.
- [ ] The project's sidebar entry no longer shows the "Hibernated — will wake when opened" tooltip once reactivated.

### Background Logic
- No dedicated BL### exists for this fork-specific feature; cited directly: `crates/project/src/project.rs:4958` (`wake_resources`), `crates/sidebar/src/project_item.rs:70-79` (hibernated-entry UI)

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A project's sidebar entry shows as Hibernated | Developer clicks it | `wake_resources` runs and the project becomes fully interactive again |

---

## US042_InitializeDevContainerForProject: Initialize a dev container

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Workspace

### User Story
As a developer, I want to initialize a `.devcontainer` configuration for my project so that I can define a reproducible containerized dev environment.

### Acceptance Criteria
- [ ] `InitializeDevContainer` opens a modal that scaffolds a valid `devcontainer.json` for the workspace.
- [ ] The generated config passes `validate_devcontainer_contents()` (matched `workspaceMount`/`workspaceFolder`, valid Compose `service` if applicable).

### Background Logic
- BL003_InitializeDevContainerAction
- BL102_DevContainerJsonParsing

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Project has no existing `.devcontainer` | Developer runs `InitializeDevContainer` and completes the modal | A valid `devcontainer.json` is written to the project |

---

## US043_BuildDevContainerImage: Build a dev container image

**Type**: system
**Interaction**: system-action
**Priority**: should
**Feature Area**: Workspace

### User Story
As a developer, I want opening a project with a valid dev-container config to build that container's image/Compose stack automatically so that I don't have to run manual Docker build commands before my environment is ready.

### Acceptance Criteria
- [ ] Opening/attaching to a dev-container-configured project builds the image/Compose stack via the Docker (or Podman) CLI.
- [ ] Build failures surface to the developer rather than silently leaving a stale/missing image.

### Background Logic
- BL103_DevContainerManifestBuildAndRun
- BL104_DockerCliIntegration

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Project has a valid `devcontainer.json` | Developer opens/attaches the project | The container image/Compose stack builds successfully |

---

## US044_RunDevContainerLifecycleScripts: Run a dev container's lifecycle scripts

**Type**: system
**Interaction**: system-action
**Priority**: should
**Feature Area**: Workspace

### User Story
As a developer, I want the dev container's lifecycle scripts to run automatically once its image is built so that my editor session runs against a fully-initialized containerized environment without manual exec commands.

### Acceptance Criteria
- [ ] After a successful build, the editor runs/exec's into the container via the Docker (or Podman) CLI.
- [ ] Lifecycle scripts (`onCreateCommand`, `postStartCommand`, etc.) execute as part of the run/exec flow.

### Background Logic
- BL103_DevContainerManifestBuildAndRun
- BL104_DockerCliIntegration

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Dev container image built successfully, config has an `onCreateCommand` | Editor runs/execs into the container | `onCreateCommand` executes as part of the run flow |

---

## US045_RunCommandInIntegratedTerminal: Run a command in the integrated terminal

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Terminal

### User Story
As a developer, I want to run a shell command in an integrated terminal so that I can execute build/test commands without leaving the editor window.

### Acceptance Criteria
- [ ] Opening a terminal panel spawns a shell process in the project's working directory.
- [ ] Command output streams into the terminal pane as it is produced.

### Background Logic
- BL067_TerminalCoreActions
- BL113_ProjectTerminalShellSpawn

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A project is open with a valid shell configured | Developer opens a terminal and runs `ls` | The shell spawns in the project root and lists its files |

---

## US046_ToggleTerminalPanel: Toggle the terminal panel

**Type**: ui
**Interaction**: secondary-action
**Priority**: must
**Feature Area**: Terminal

### User Story
As a developer, I want to toggle the terminal panel's visibility so that I can quickly show or hide it while I work.

### Acceptance Criteria
- [ ] The toggle keybinding/command shows the terminal panel if hidden and hides it if visible.
- [ ] A running terminal session inside the panel keeps running while the panel is hidden.

### Background Logic
- BL068_TerminalPanelToggleActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Terminal panel is hidden with a session running | Developer triggers toggle | The panel becomes visible showing the still-running session's output |

---

## US047_RunConfiguredTask: Run a configured task

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Terminal

### User Story
As a developer, I want to run a task defined in `tasks.json` so that I can execute a project-specific build/test/lint command with its configured arguments.

### Acceptance Criteria
- [ ] Selecting a task resolves its command/args/cwd from `tasks.json` and spawns it inside a new or reused terminal.
- [ ] The task's exit status is surfaced once it completes.

### Background Logic
- BL114_ProjectTaskTerminalSpawn
- BL194_BuildTaskContextsOffThread

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | `tasks.json` defines a `cargo test` task | Developer runs it | A terminal spawns running `cargo test` in the project root and reports its exit code |

---

## US048_SearchTerminalScrollback: Search terminal scrollback

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Terminal

### User Story
As a developer, I want to search the scrollback of an integrated terminal so that I can find earlier output without scrolling manually.

### Acceptance Criteria
- [ ] Cmd-F inside a terminal pane opens a search bar that highlights matches in the scrollback buffer.
- [ ] Next/previous match navigation scrolls the terminal view to each match in order.

### Background Logic
- BL195_SearchTerminalScrollback

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Terminal scrollback contains the string "error" 3 times | Developer searches "error" | All 3 occurrences are highlighted and navigable |

---

## US049_SwitchLanguageServerToolchain: Switch language server toolchain

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Language Intelligence

### User Story
As a developer, I want to switch the active toolchain for a language from the status bar so that my language server analyzes code using the correct interpreter/SDK version.

### Acceptance Criteria
- [ ] Selecting a toolchain in the picker updates the language server's configured toolchain for that worktree.
- [ ] The language server restarts (or reconfigures) using the newly selected toolchain.

### Background Logic
- BL073_ToolchainSelectorActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Two Python interpreters are detected for the project | Developer selects the non-default one from the toolchain picker | The Python language server reconfigures to use the selected interpreter |

---

## US050_SwitchBufferLanguage: Switch a buffer's language

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Language Intelligence

### User Story
As a developer, I want to manually set a buffer's language from the status bar so that syntax highlighting and language-server features apply correctly to a file whose extension was misdetected.

### Acceptance Criteria
- [ ] Selecting a language in the picker re-associates the buffer with that language's grammar/LSP.
- [ ] Syntax highlighting re-renders immediately using the newly selected language's grammar.

### Background Logic
- BL038_LanguageSelectorToggleAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A `.txt` file actually contains Rust code | Developer sets its language to Rust via the status-bar selector | Rust syntax highlighting and the Rust language server activate for that buffer |

---

## US051_RestartLanguageServersForBuffer: Restart language servers for a buffer

**Type**: system
**Interaction**: system-action
**Priority**: should
**Feature Area**: Language Intelligence

### User Story
As a developer, I want to restart the language server(s) backing my open buffers so that I can recover from a hung/stale server without restarting the whole editor.

### Acceptance Criteria
- [ ] Triggering the restart action tears down and respawns the language server process(es) associated with the target buffers.
- [ ] Buffers regain diagnostics/completions once the respawned server re-initializes.

### Background Logic
- BL182_RestartLanguageServersForBuffers
- BL107_LanguageServerProcessLifecycle

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | The rust-analyzer process for a project has become unresponsive | Developer triggers "Restart Language Servers" | rust-analyzer respawns and diagnostics resume within the affected buffers |

---

## US052_NavigateTextWithVimMotions: Navigate text with Vim motions

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Vim

### User Story
As a developer, I want to move my cursor using Vim motion keys so that I can navigate text efficiently while Vim emulation is enabled.

### Acceptance Criteria
- [ ] Motion keys (`w`, `e`, `b`, `j`, `k`, etc.) move the cursor by the documented Vim unit (word/line) in normal mode.
- [ ] A count prefix (e.g. `3w`) repeats the motion that many times.

### Background Logic
- BL081_VimMotionActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Cursor is at the start of a line with 3 words | Developer presses `w` twice | Cursor lands at the start of the third word |

---

## US053_EnterVimInsertMode: Enter Vim insert mode

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Vim

### User Story
As a developer, I want to enter Vim insert mode at a specific cursor position (before/after/line-start/line-end) so that I can start typing exactly where I intend.

### Acceptance Criteria
- [ ] `i`/`a`/`I`/`A`/`o`/`O` each place the cursor per their documented Vim semantics and switch the mode to Insert.
- [ ] Text typed after entering insert mode is inserted at that cursor position, not overwriting existing text.

### Background Logic
- BL082_VimNormalModeInsertActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Cursor is mid-line in Vim normal mode | Developer presses `A` | Cursor jumps to end of line and mode switches to Insert |

---

## US054_SelectTextInVimVisualMode: Select text in Vim visual mode

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Vim

### User Story
As a developer, I want to select text using Vim's visual mode (character/line/block) so that I can operate on a precise text range before deleting, yanking, or changing it.

### Acceptance Criteria
- [ ] `v`, `V`, and `Ctrl-V` enter character-wise, line-wise, and block-wise visual selection respectively.
- [ ] Motions performed while in visual mode extend the current selection instead of moving the cursor alone.

### Background Logic
- BL093_VimVisualModeActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Cursor is at the start of a 3-line block | Developer presses `V` then `jj` | All 3 lines are selected line-wise |

---

## US055_RunVimExCommand: Run a Vim ex command

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Vim

### User Story
As a developer, I want to type and execute a Vim `:`-command so that I can perform range-based or configuration operations (e.g. `:w`, `:%s/a/b/g`) the same way I would in real Vim.

### Acceptance Criteria
- [ ] Pressing `:` opens the ex command line, and submitting a recognized command executes it against the buffer/range.
- [ ] An unrecognized ex command surfaces an error rather than silently doing nothing.

### Background Logic
- BL075_VimExCommandActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Buffer contains the word "foo" three times | Developer runs `:%s/foo/bar/g` | All three occurrences become "bar" |

---

## US056_RepeatLastVimChange: Repeat the last Vim change

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Vim

### User Story
As a developer, I want to repeat my last Vim change with `.` so that I can apply the same edit at a new cursor position without retyping it.

### Acceptance Criteria
- [ ] `.` re-executes the most recent change-producing command (insert, delete, etc.) at the current cursor position.
- [ ] Recording (`q`) and replaying (`@@`) a macro repeats the exact recorded key sequence.

### Background Logic
- BL085_VimRepeatMacroActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Developer just deleted a word with `dw` | Cursor moves elsewhere and `.` is pressed | The word under the new cursor position is deleted the same way |

---

## US057_SelectVimTextObject: Select a Vim text object

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Feature Area**: Vim

### User Story
As a developer, I want to select a Vim text object (word, quoted string, bracketed block) as the target of an operator so that I can act on a whole syntactic unit in one keystroke combo.

### Acceptance Criteria
- [ ] Operator+object combos (`diw`, `ci(`, `da"`, etc.) select exactly the documented text-object range for the operator to act on.
- [ ] Inner (`i`) vs. around (`a`) variants include/exclude the object's delimiters as documented.

### Background Logic
- BL089_VimTextObjectActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Cursor is inside `"hello"` | Developer runs `ci"` | The contents between the quotes are deleted and insert mode starts inside them |

---

## US058_EditSettingsJson: Edit a setting in settings.json

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Settings

### User Story
As a developer, I want to edit a value in the Settings UI/editor so that a configuration change takes effect live without restarting the editor.

### Acceptance Criteria
- [ ] Changing a setting field updates the underlying `settings.json` and the in-memory `SettingsStore`.
- [ ] Every registered `impl Settings` consumer observing that key re-renders/reconfigures on the change.

### Background Logic
- BL061_SettingsEditorActions
- BL138_SettingsStoreFileWatcher

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Developer changes `buffer_font_size` in the Settings editor | The change is saved | All open editors immediately render with the new font size |

---

## US059_EditKeymapBinding: Edit a keymap binding

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Settings

### User Story
As a developer, I want to edit a keybinding in the keymap editor so that I can rebind an action to a key combination I prefer.

### Acceptance Criteria
- [ ] Entering a new keystroke for an action in the keymap editor writes it to the user keymap file.
- [ ] The new binding takes effect immediately without restarting the editor.

### Background Logic
- BL036_KeymapEditorActions
- BL143_KeymapAndSettingsObserver

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Developer rebinds `Save` from `Cmd-S` to `Cmd-Shift-S` | The binding is saved | Pressing `Cmd-Shift-S` now triggers Save, and `Cmd-S` no longer does |

---

## US060_SwitchBaseKeymapPreset: Switch the base keymap preset

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Feature Area**: Settings

### User Story
As a developer, I want to switch the editor's base keymap preset (e.g. to a VS Code or Sublime-style layout) so that my muscle memory from another editor carries over.

### Acceptance Criteria
- [ ] Selecting a base keymap preset in the selector applies that preset's default bindings, layered under any custom user bindings.
- [ ] The change is reflected immediately across all open windows.

### Background Logic
- BL048_ToggleBaseKeymapSelectorAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Default base keymap is active | Developer selects a different base keymap preset | Keybindings from that preset become active immediately |

---

## US061_BackupSettingsBeforeMigration: Back up settings before a schema migration

**Type**: system
**Interaction**: system-action
**Priority**: must
**Feature Area**: Settings

### User Story
As a developer, I want a backup copy of my settings/keymap files written before any migration runs so that I can recover my prior configuration if the migration goes wrong.

### Acceptance Criteria
- [ ] Clicking "Backup and Update" in the migration dialog writes a backup copy of the current settings/keymap files before anything else happens.
- [ ] The backup file is written to a recoverable location distinct from the live settings file.

### Background Logic
- BL207_RunKeymapOrSettingsMigration

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Settings file uses a deprecated schema field | Developer clicks "Backup and Update" | A backup file is written before migration begins |

---

## US062_MigrateSettingsToCurrentSchema: Migrate settings to the current schema

**Type**: system
**Interaction**: system-action
**Priority**: must
**Feature Area**: Settings

### User Story
As a developer, I want my settings/keymap files migrated to the current schema after they're backed up so that an editor update doesn't silently break or lose my configuration.

### Acceptance Criteria
- [ ] After the backup completes, the migration applies to the live settings/keymap files.
- [ ] The migrated file parses successfully under the current schema after the operation completes.

### Background Logic
- BL207_RunKeymapOrSettingsMigration

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Settings file backed up, uses a deprecated schema field | Migration applies to the live file | The live settings file parses successfully under the current schema |

---

## US063_FindFileByFuzzyName: Find a file by fuzzy name

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Search

### User Story
As a developer, I want to fuzzy-search for a file by (partial) name so that I can open it without navigating the project tree manually.

### Acceptance Criteria
- [ ] The file finder modal ranks and lists project files matching the typed fuzzy query.
- [ ] Confirming the highlighted result opens that file in the active pane.

### Background Logic
- BL020_FileFinderActions
- BL159_CheckRecentHistoryPathExists

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Project contains `src/main.rs` | Developer types "mnrs" in the file finder | `src/main.rs` appears as a top ranked match and opens on confirm |

---

## US064_SearchAcrossProject: Search across the whole project

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Search

### User Story
As a developer, I want to search for a text pattern across the entire project so that I can find every usage of a symbol or string regardless of which file it's in.

### Acceptance Criteria
- [ ] Submitting a project search query returns matches grouped by file, with per-match context lines.
- [ ] Search results can be opened as a multi-buffer for reviewing/editing matches in place.

### Background Logic
- BL059_ProjectSearchActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | The string "TODO" appears in 5 files | Developer searches "TODO" project-wide | All 5 files' matches are listed with surrounding context |

---

## US065_SearchWithinCurrentBuffer: Search within the current buffer

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Feature Area**: Search

### User Story
As a developer, I want to search for text within my currently open buffer so that I can jump between occurrences of a term in the file I'm editing.

### Acceptance Criteria
- [ ] Opening the in-buffer search bar and submitting a query highlights all matches in the active buffer.
- [ ] Next/previous navigation moves the cursor/scroll position between matches in order.

### Background Logic
- BL060_BufferSearchActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | Active buffer contains "foo" 4 times | Developer searches "foo" | All 4 occurrences highlight and are navigable via next/previous |

---

## US066_SwitchBetweenOpenTabs: Switch between open tabs

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Feature Area**: App Shell

### User Story
As a developer, I want to switch between recently-used open tabs via a quick-switcher modal so that I can jump back to a file I was just editing without hunting through the tab bar.

### Acceptance Criteria
- [ ] Holding the tab-switcher modifier and pressing the trigger key cycles through open tabs in most-recently-used order.
- [ ] Releasing the modifier confirms the highlighted tab and focuses it.

### Background Logic
- BL066_TabSwitcherActions

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | 3 tabs are open, tab C was focused most recently before tab A | Developer holds the switcher modifier and taps the trigger key once | Tab C becomes the highlighted/confirmed selection |

---

## US067_OpenDebugAdapterLogs: Open Debug Adapter Protocol logs

**Type**: ui
**Interaction**: navigation
**Priority**: could
**Feature Area**: Debugging

### User Story
As a developer, I want to open the Debug Adapter Protocol log viewer so that I can diagnose why a debug adapter isn't behaving as expected.

### Acceptance Criteria
- [ ] `OpenDebugAdapterLogs` opens a log viewer backed by a `LogStore` observing all active debug sessions.
- [ ] Logs for a given session remain viewable after that session ends, until the viewer is closed.

### Background Logic
- BL004_OpenDebugAdapterLogsAction

### Test Scenarios
| Scenario | Given | When | Then |
|---|---|---|---|
| Happy Path | A debug session is active | Developer triggers `OpenDebugAdapterLogs` | A log pane opens showing that session's DAP protocol traffic |

---

## Feature Area → US Map

| Feature Area | US Codes |
|---|---|
| Editor Core | US001, US009, US010, US011 |
| Diagnostics | US012, US013, US014 |
| Debugging | US015, US016, US002, US003, US004, US005, US017, US018, US067 |
| Git | US019, US006, US007, US008, US020, US021, US022, US023, US024 |
| Extensions | US025, US026, US027, US028, US029, US030, US031, US032, US033 |
| Workspace | US034, US035, US036, US037, US038, US039, US040, US041, US042, US043, US044 |
| Terminal | US045, US046, US047, US048 |
| Language Intelligence | US049, US050, US051 |
| Vim | US052, US053, US054, US055, US056, US057 |
| Settings | US058, US059, US060, US061, US062 |
| Search | US063, US064, US065 |
| App Shell | US066 |

## Cross-Reference Validation

- [x] All US### codes are unique
- [x] All acceptance criteria are testable
- [x] All technical notes/citations are complete (BL###, PERM###, or direct `file:line` for the two hibernation stories with no dedicated BL entry, re-verified this session with `grep`)
- [x] Feature mapping deferred to `feature-list.md` (this doc contains US without direct feature codes, per template convention)
- [x] All `ui`-typed US map to a Feature Area (no SCR### exists in this profile; system US excluded from mapping requirement)
- [x] All `system`-typed US have at least one BL### (or PERM###/direct-citation) mapped
- [x] No AI-agent, collaboration, or LiveKit references reintroduced (verified via `grep -i "agent\|collab\|livekit\|language_model"` against this file before finalizing)

## Limits

- **Not exhaustive over all 207 BL items.** This pass authored 67 US covering every functional domain named in
  the Wave 4 brief, prioritizing `must`/`should`-worthy, genuinely distinct user intents. Left uncovered by
  deliberate choice: the ~15 remaining Vim BL items (increment/decrement, rewrap, replace mode, digraphs, Helix
  mode, change-list navigation, register/mark deletion, shell-filter piping) — all structurally identical in
  shape to the 6 Vim US already written (one keybinding-driven single-verb action); a dedicated Vim-emulation
  pass should add them if full parity is required. Also uncovered: internal dev-only diagnostic tooling
  (BL016 ETW tracing, BL032 input-latency histogram, BL039 highlights-tree view, BL040 key-context view, BL043
  syntax-tree view, BL096 theme-preview, BL147 component-preview) — genuinely `could`-priority editor-authoring
  tools rather than end-user features, and the onboarding/window-chrome/CLI-install/journal/feedback/image-
  viewer/CSV-preview/SVG-preview/markdown-preview items (BL002, BL019, BL031, BL033, BL034, BL035, BL045, BL046,
  BL049, BL053, BL064, BL065, BL071, BL072, BL099, BL172) — real but lower-signal single-purpose actions that
  would follow the same one-BL-to-one-US pattern already demonstrated above without adding new judgment calls.
- **Remote development (SSH) is only lightly covered** (US018 attach-to-remote-process). The `remote`/
  `remote_connection`/`remote_server` subsystem's own user-facing surface (opening an SSH project, forwarding
  buffer edits/breakpoints to a headless remote host — BL177, BL185, BL188, BL189) was not separately storied;
  it is mostly system-action plumbing behind the "open project"/"toggle breakpoint" stories already written,
  and a dedicated remote-dev feature spec would benefit from its own pass.
- **No corroborating cross-check against a second independent source was performed for the Feature Area
  groupings** beyond `system-overview.md`/`architecture.md`; those two documents and `behavior-logic.md`/
  `permissions-matrix.md` were treated as the authoritative in-repo sources for this wave, consistent with the
  rebuild-spec pipeline's design (BL/PERM artifacts are themselves the researched ground truth for this pass,
  not a claim requiring external validation).
