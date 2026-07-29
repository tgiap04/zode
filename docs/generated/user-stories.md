<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# User Stories

> [!CAUTION]
> **STALE — do not treat this document as a description of the current code.**
> It was generated on 2026-07-26 against the pre-fork tree of 240 packages /
> 232 crates. The hard fork has since removed 54 crates and gutted several
> more; the workspace is now 186 packages / 178 crates.
>
> Anything here describing accounts, sign-in, collaboration, calls, channels,
> AI agents, LLM providers, edit prediction, auto-update or crash reporting is
> **fiction** — that code no longer exists. Feature codes F007, F008, F013,
> F019, F020, F021 and F022 in particular no longer have an implementation.
>
> Regeneration is deliberately deferred until the fork is green and verified
> (`/tkm:rebuild-spec` after phase 11). Running it against a half-cut tree
> would just produce a second stale document.


**Project**: Zed (zode)
**Generated**: 2026-07-26
**Analysis Scope**: Full monorepo — native GPUI desktop editor (`generic-source` profile, `screen_source:none`)

**Code Format**: All US codes follow `US###_NameSlug` format (e.g., US001_OpenProject).

**US Types**:
- `ui` - User-facing stories (developer interacting with an editor surface)
- `system` - System stories: hook, event, observer, bg-job, trigger, etc. (mapped to BL### only)

## Adaptation Note (no ScreenList upstream)

Zed is a native desktop app, not a routed web app — no `screen-list.md`/`screen-flow.md` artifact
exists upstream (per `_session-context.md` and `behavior-logic.md`, `screen_source:none`). Per the
template's own allowance ("`ui`-typed stories may map to `SCR###`... system US do not"), and because
no SCR### codes exist to map to, the **Screens** field below names the GPUI `Render`-implementing
UI surface descriptively (e.g. "Editor pane", "Project Panel", "Agent panel") instead of a SCR###
code. This mirrors system-overview.md's stated analogue: "GPUI `Render`-implementing entities
(`Editor`, `Workspace`, dock panels)" stand in for screens. No `[IPE_ZERO]` screen-map table is
produced since there is no ScreenList to cross-validate against.

**Roles used**: `developer` (the local editor user / project owner), `collaborator` (a remote
participant joined to a shared project, further qualified by ChannelRole where relevant: Admin /
Member / Talker / Guest), `extension author` (third-party code author, non-interactive role).

## User Story Index

| Code | Title | Type | Priority | Screens |
|------|-------|------|----------|---------|
| US001_OpenProjectFolder | Open a project folder | ui | must | Workspace |
| US002_BrowseWorktreeFiles | Browse worktree files in the project panel | ui | must | Project Panel |
| US003_EditBufferText | Edit an open file's text | ui | must | Editor pane |
| US004_OpenMultiBufferSearchResults | Open search results as a multi-buffer | ui | should | Editor pane / Search results |
| US005_ReceiveLspDiagnostics | Receive LSP diagnostics while typing | ui | must | Editor pane |
| US006_ApplyLspCodeAction | Apply an LSP code action | ui | should | Editor pane |
| US007_ChangeEditorTheme | Change the editor's color theme | ui | should | Settings / Theme picker |
| US008_EditSettingsJson | Edit a setting in `settings.json` and see it take effect live | ui | must | Settings editor |
| US009_InstallExtension | Install a third-party extension | ui | should | Extensions panel |
| US010_ShareProjectForCollaboration | Share a project so others can collaborate | ui | must | Workspace / Collab panel |
| US011_JoinSharedProject | Join a shared project as a collaborator | ui | must | Workspace / Collab panel |
| US012_ChangeCollaboratorRole | Change a collaborator's role in a shared project | ui | should | Collab panel |
| US013_StartVoiceCall | Start a voice/video call with collaborators | ui | should | Call panel |
| US014_RunTerminalCommand | Run a shell command in the integrated terminal | ui | must | Terminal panel |
| US015_ViewGitDiff | View a file's git diff | ui | must | Git panel / Editor gutter |
| US016_CommitStagedChanges | Commit staged changes | ui | must | Git panel |
| US017_ChatWithAiAgent | Chat with the AI agent in a thread | ui | must | Agent panel |
| US018_AcceptInlineEditPrediction | Accept an inline AI edit prediction | ui | should | Editor pane |
| US019_SwitchLlmProvider | Switch the agent's active LLM provider | ui | should | Agent panel / Settings |
| US020_ReceiveAutoUpdateNotification | Receive and apply an application auto-update | system | should | — |
| US021_ReadOnlyBufferRejectsEdit | Read-only buffer silently rejects an edit attempt | system | must | Editor pane |
| US022_ExtensionCapabilityDenied | Extension's undeclared capability request is denied | system | must | — |
| US023_SettingsChangeNotifiesObservers | Settings-store change notifies all registered observers | system | must | — |
| US024_DispatchKeyboardAction | Keyboard shortcut dispatches a registered action | system | must | — |
| US025_ResolveGitHostingPermalink | Resolve a permalink for the current git remote | ui | could | Editor pane / Git panel |
| US026_ReceiveLspCompletions | Receive LSP completion suggestions while typing | ui | must | Editor pane |
| US027_StageGitHunk | Stage a git hunk | ui | must | Git panel / Editor gutter |

## Interaction Inventory

> No SCR###/ScreenList upstream exists for this project (native desktop app, `screen_source:none`);
> rows below use the descriptive UI-surface names from the Index above in place of `{SCR###_Name}`,
> per the Adaptation Note.

| Screen | Element | Type | Action | Endpoint |
|--------|---------|------|--------|---------|
| Workspace | "Open Folder" menu item / dialog | primary-action | Opens a native file picker, adds the chosen path as a `Worktree` in the `Project` | N/A |
| Project Panel | File/directory entry | navigation | Opens the file's buffer in the active pane, or expands/collapses a directory `Entry` | N/A |
| Editor pane | Text buffer | primary-action | Keystrokes mutate the `TextBuffer`/`Buffer` rope; `Editor` view re-renders | N/A |
| Editor pane | Search results multi-buffer | secondary-action | Aggregates matching excerpts from multiple buffers into one scrollable `MultiBuffer` | N/A |
| Editor pane | Inline diagnostic/completion popover | system-action | LSP `textDocument/publishDiagnostics` / `textDocument/completion` response rendered inline | N/A |
| Editor pane | Code action lightbulb | secondary-action | Requests `textDocument/codeAction`, applies chosen action's `WorkspaceEdit` | N/A |
| Theme picker | Theme list item | primary-action | Swaps the active `Theme`/`ThemeFamily`, re-renders all `Render`-implementing entities | N/A |
| Settings editor | `settings.json` text buffer | primary-action | On save, `SettingsStore` re-parses JSON and notifies `impl Settings for` registrants | N/A |
| Extensions panel | "Install" button | primary-action | Downloads, verifies, and loads a WASM extension via `extension_host` | N/A |
| Workspace | "Share Project" action | primary-action | Registers the current `Project` with `collab` server, generates an invite/join link | N/A |
| Collab panel | "Join" action | primary-action | Client connects to `collab` server over RPC, requests project state as the assigned `ChannelRole` | N/A |
| Collab panel | Role dropdown per participant | destructive-action | Admin-only; changes another participant's `ChannelRole` (Admin/Member/Talker/Guest/Banned) | N/A |
| Call panel | "Start Call" / "Join Call" button | primary-action | Establishes a LiveKit WebRTC session for voice/video with room participants | N/A |
| Terminal panel | Command input | primary-action | Spawns a shell process, streams stdout/stderr into the `Terminal` entity | N/A |
| Git panel | "Stage Hunk" inline action | primary-action | Applies a hunk-level stage operation via `GitStore`/`Repository` | N/A |
| Git panel | "Commit" button | primary-action | Runs `git commit` against staged changes in the active `Repository` | N/A |
| Agent panel | Message input + "Send" | primary-action | Appends a `Message` to the active `Thread`, streams the model response, dispatches queued tool calls | N/A |
| Editor pane | Inline edit-prediction ghost text | secondary-action | Tab/Accept action applies the AI-suggested edit to the buffer | N/A |
| Agent panel | Provider selector | secondary-action | Switches which `language_model` provider crate services the active `Thread` | N/A |
| — (system) | Auto-update background loop | system-action | Periodically polls update server; on new release, downloads and prompts a restart | N/A |
| Editor pane | Edit keystroke on a read-only buffer | system-action | Edit is silently discarded — `Capability` check fails before the rope mutation applies | N/A |
| — (system) | Extension host capability call | system-action | `extension_host` checks the call against the manifest allowlist; undeclared calls are rejected pre-execution | N/A |
| — (system) | `SettingsStore` re-parse | system-action | Fires observer callbacks on every `impl Settings for` registrant after a settings-JSON change | N/A |
| — (system) | Keybinding trigger | system-action | GPUI matches the keystroke to a registered `actions!()` action and dispatches to the focused handler | N/A |
| Git panel | "Copy Permalink" action | secondary-action | Resolves the current git remote via `GitHostingProvider` to build a GitHub/GitLab/Bitbucket permalink URL | N/A |

---

## US001_OpenProjectFolder: Open a project folder

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: M

### User Story

As a developer, I want to open a project folder so that I can browse and edit its files.

### Acceptance Criteria

- [ ] Criterion 1: Selecting a folder via the native file picker adds it as a `Worktree` under the current `Project`.
- [ ] Criterion 2: The `Worktree`'s file tree is populated in the Project Panel without a full manual re-scan (incremental filesystem watching).
- [ ] Criterion 3: Opening a folder that is already open focuses the existing `Workspace` window instead of creating a duplicate.

### Technical Notes

- **Endpoint**: N/A (native OS file dialog, no HTTP route)
- **Data Required**: `Project`, `Worktree`, `Entry` (data-model.md)
- **Dependencies**: OS-level file-picker integration; `crates/fs` filesystem watcher

### Screens

- Workspace: main application window

### Background Logic

- BL005_WorkspaceEventEmitterSubscribe: `Workspace` emits an event when a worktree is added, other panels subscribe to refresh.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | No project is open | Developer selects a folder via "Open Folder" | The folder appears as a root `Worktree` with its files listed in the Project Panel |
| Error Case | Selected path was deleted/unmounted between pick and open | Developer confirms the (now-invalid) path | Workspace surfaces an error state for that worktree instead of silently showing an empty tree |

---

## US002_BrowseWorktreeFiles: Browse worktree files in the project panel

**Type**: ui
**Interaction**: navigation
**Priority**: must
**Estimate**: S

### User Story

As a developer, I want to browse my project's file tree so that I can locate and open the file I need to edit.

### Acceptance Criteria

- [ ] Criterion 1: Expanding a directory `Entry` lazily lists its children without blocking the UI thread.
- [ ] Criterion 2: Clicking a file `Entry` opens (or focuses, if already open) its `Buffer` in the active editor pane.
- [ ] Criterion 3: External filesystem changes (files added/removed/renamed) are reflected in the tree without a manual refresh.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `Worktree`, `Entry` (data-model.md)
- **Dependencies**: `crates/fs` filesystem watcher; `crates/worktree`

### Screens

- Project Panel: file tree dock

### Background Logic

- BL005_WorkspaceEventEmitterSubscribe: entry-tree updates propagate to the panel view via entity events.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A worktree with nested directories is open | Developer expands a directory | Its child entries render, sorted per the panel's configured order |
| Error Case | A watched file is deleted on disk while the panel is open | Filesystem watcher detects the deletion | The corresponding `Entry` is removed from the tree without a crash or stale reference |

---

## US003_EditBufferText: Edit an open file's text

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: L

### User Story

As a developer, I want to edit an open file's text so that I can make changes to my code.

### Acceptance Criteria

- [ ] Criterion 1: Keystrokes mutate the underlying `Rope`/`TextBuffer` and the `Editor` view re-renders the change immediately.
- [ ] Criterion 2: Edits are only accepted when the buffer's `Capability` is `ReadWrite`; otherwise see US021.
- [ ] Criterion 3: Undo/redo restores prior buffer states using the buffer's operation history.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `TextBuffer`, `Buffer`, `Editor` (data-model.md)
- **Dependencies**: `crates/text` (rope/sum_tree), `crates/language`, `crates/editor`

### Screens

- Editor pane: active buffer view

### Background Logic

- BL006_LspRequestDispatch: buffer edits are debounced and forwarded to the language server as `textDocument/didChange`.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A read-write buffer is focused | Developer types a character | The rope updates, the view re-renders, and the LSP is notified of the change |
| Error Case | Buffer's capability is Read | Developer attempts to type | See US021_ReadOnlyBufferRejectsEdit — edit is discarded |

---

## US004_OpenMultiBufferSearchResults: Open search results as a multi-buffer

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Estimate**: M

### User Story

As a developer, I want to open project-wide search results as a single scrollable view so that I can review and edit matches across many files without opening each one individually.

### Acceptance Criteria

- [ ] Criterion 1: Running a project search aggregates matching excerpts from multiple `TextBuffer`s into one `MultiBuffer`.
- [ ] Criterion 2: Editing a match inside the multi-buffer writes back to the originating file's real buffer.
- [ ] Criterion 3: Excerpts update live if the underlying file changes while the multi-buffer is open.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `MultiBuffer`, `Buffer` (data-model.md)
- **Dependencies**: `crates/multi_buffer`, project-wide search index

### Screens

- Editor pane / Search results: multi-buffer excerpt view

### Background Logic

- N/A (direct user-triggered aggregation, no BL### applies)

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A project-wide text search returns matches in 3 files | Developer opens results as a multi-buffer | All 3 files' matching excerpts render in one scrollable pane |
| Error Case | One matched file is deleted after the search runs, before the multi-buffer is opened | Developer opens the results | The deleted file's excerpt is omitted or marked stale rather than causing a panel error |

---

## US005_ReceiveLspDiagnostics: Receive LSP diagnostics while typing

**Type**: ui
**Interaction**: system-action
**Priority**: must
**Estimate**: M

### User Story

As a developer, I want to see inline diagnostics from the language server so that I can catch errors as I type.

### Acceptance Criteria

- [ ] Criterion 1: On buffer change, `textDocument/didChange` is sent and subsequent `publishDiagnostics` notifications render inline squiggles/markers.
- [ ] Criterion 2: LSP requests run off the UI thread (`cx.background_spawn`) so typing never blocks on network/process latency.

### Technical Notes

- **Endpoint**: N/A (LSP over stdio/socket, not HTTP)
- **Data Required**: `Buffer`, `Editor` (data-model.md)
- **Dependencies**: `crates/project` (`LspStore`), configured language server binary

### Screens

- Editor pane: inline diagnostic markers

### Background Logic

- BL006_LspRequestDispatch: `LspStore::handle_lsp_*` handlers dispatch and route LSP protocol messages.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A supported language server is attached to the buffer | Developer types an incomplete expression | A syntax error underlines inline |
| Error Case | The language server process crashes mid-session | Developer continues typing | Editing continues uninterrupted; diagnostics gracefully stop until the server restarts |

---

## US026_ReceiveLspCompletions: Receive LSP completion suggestions while typing

**Type**: ui
**Interaction**: system-action
**Priority**: must
**Estimate**: M

### User Story

As a developer, I want completion suggestions from the language server so that I can write code faster.

### Acceptance Criteria

- [ ] Criterion 1: Requesting completions at the cursor shows a popover populated from `textDocument/completion`.
- [ ] Criterion 2: LSP requests run off the UI thread (`cx.background_spawn`) so typing never blocks on network/process latency.

### Technical Notes

- **Endpoint**: N/A (LSP over stdio/socket, not HTTP)
- **Data Required**: `Buffer`, `Editor` (data-model.md)
- **Dependencies**: `crates/project` (`LspStore`), configured language server binary

### Screens

- Editor pane: completion popover

### Background Logic

- BL006_LspRequestDispatch: `LspStore::handle_lsp_*` handlers dispatch and route LSP protocol messages.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A supported language server is attached to the buffer | Developer types an incomplete expression | A completion popover appears |
| Error Case | The language server process crashes mid-session | Developer continues typing | Editing continues uninterrupted; completions gracefully stop until the server restarts |

---

## US006_ApplyLspCodeAction: Apply an LSP code action

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Estimate**: M

### User Story

As a developer, I want to apply a suggested code action so that I can fix an issue or refactor code without typing the fix by hand.

### Acceptance Criteria

- [ ] Criterion 1: Triggering the code-action menu requests `textDocument/codeAction` and lists the server's suggestions.
- [ ] Criterion 2: Selecting an action applies its `WorkspaceEdit` atomically across all affected buffers.
- [ ] Criterion 3: The action is undoable as a single undo-stack entry.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `Buffer` (data-model.md)
- **Dependencies**: `crates/project` (`handle_apply_code_action`)

### Screens

- Editor pane: code-action lightbulb menu

### Background Logic

- BL006_LspRequestDispatch: `handle_apply_code_action` routes the accepted action back through the LSP dispatch table.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Server returns a single-file rename-symbol code action | Developer selects it | All references update atomically in one undoable edit |
| Error Case | The `WorkspaceEdit` touches a buffer that has unsaved conflicting edits | Developer applies the action | Action fails cleanly with no partial application, rather than silently corrupting the buffer |

---

## US007_ChangeEditorTheme: Change the editor's color theme

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Estimate**: S

### User Story

As a developer, I want to change the editor's color theme so that the interface matches my visual preference.

### Acceptance Criteria

- [ ] Criterion 1: Selecting a theme from the picker swaps the active `Theme`/`ThemeFamily` immediately, no restart required.
- [ ] Criterion 2: All open `Render`-implementing views (editor, panels, terminal) re-render with the new theme's colors.
- [ ] Criterion 3: The choice persists across restarts via settings.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `Theme` / `ThemeFamily` (data-model.md)
- **Dependencies**: `crates/theme`

### Screens

- Settings / Theme picker

### Background Logic

- BL013_SettingsStoreObserver: theme selection is persisted to settings JSON, re-parsed, and observers re-render.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer opens theme picker | Selects a different theme | All UI surfaces immediately reflect the new theme's palette |
| Error Case | Selected theme references a missing/corrupt theme file | Developer selects it | Editor falls back to the previous theme rather than rendering with broken/undefined colors |

---

## US008_EditSettingsJson: Edit a setting in settings.json and see it take effect live

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: S

### User Story

As a developer, I want to edit a setting in `settings.json` so that the editor's behavior updates without restarting the app.

### Acceptance Criteria

- [ ] Criterion 1: Saving `settings.json` triggers `SettingsStore` to re-parse the file.
- [ ] Criterion 2: Every `impl Settings for FooSettings` registrant affected by the change is notified and applies the new value live.
- [ ] Criterion 3: A malformed JSON edit does not crash the app or silently discard the previous valid settings.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `SettingsStore` (data-model.md)
- **Dependencies**: `crates/settings`

### Screens

- Settings editor: `settings.json` buffer

### Background Logic

- BL013_SettingsStoreObserver: `SettingsStore` re-parses on change and notifies ~40 registrant call sites.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer changes `"tab_size": 2` to `4` | Saves the file | Currently open buffers immediately use 4-space tabs |
| Error Case | Developer introduces a JSON syntax error and saves | Save completes | `SettingsStore` keeps the last valid parsed settings and surfaces a parse error, rather than crashing |

---

## US009_InstallExtension: Install a third-party extension

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Estimate**: M

### User Story

As a developer, I want to install a third-party extension so that I can add language, theme, or tool support not built into the editor.

### Acceptance Criteria

- [ ] Criterion 1: Installing downloads and loads the extension's compiled `wasm32-wasip2` module into the sandboxed `extension_host` runtime.
- [ ] Criterion 2: The extension's declared manifest (`ExtensionManifest`) capabilities are recorded as its allowlist before any code runs.
- [ ] Criterion 3: Extension calls execute off the main thread (`cx.background_spawn`) so a slow/misbehaving extension does not freeze the UI.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `ExtensionManifest` (data-model.md)
- **Dependencies**: `crates/extension_host`, `crates/extension_api`

### Screens

- Extensions panel: install/manage list

### Background Logic

- BL004_ExtensionHostWasmDispatch: sandboxed WASM calls dispatched in the background and routed back to the main thread.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer selects a published extension | Clicks "Install" | Extension loads, its declared capabilities become active, and it appears as installed |
| Error Case | Extension's WASM module fails to compile/instantiate (corrupt build) | Install proceeds | Install fails with a surfaced error; host process does not crash |

---

## US010_ShareProjectForCollaboration: Share a project so others can collaborate

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: L

### User Story

As a developer, I want to share my open project so that a collaborator can join and pair-program with me in real time.

### Acceptance Criteria

- [ ] Criterion 1: Sharing registers the current `Project` with the `collab` server over the client↔server RPC connection.
- [ ] Criterion 2: The host retains full local control; sharing does not restrict the host's own edits.
- [ ] Criterion 3: An invite/room identifier is produced that a collaborator can use to join.

### Technical Notes

- **Endpoint**: N/A (custom binary RPC over `crates/proto`/`crates/rpc`, not HTTP)
- **Data Required**: `Project`, `Workspace` (data-model.md)
- **Dependencies**: `crates/collab` (server), `crates/client`, `crates/call`, `crates/channel`

### Screens

- Workspace / Collab panel: share action + invite UI

### Background Logic

- BL007_RpcProtoMessageRouting: share request/response routed as a typed proto message over the persistent RPC connection.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer is signed in and has a project open | Clicks "Share Project" | Project registers with `collab`, an invite becomes available, host's own editing is unaffected |
| Error Case | Network connection to `collab` server is unavailable | Developer clicks "Share Project" | Share fails with a clear connection error rather than silently appearing shared |

---

## US011_JoinSharedProject: Join a shared project as a collaborator

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: L

### User Story

As a collaborator, I want to join a shared project so that I can view and (if permitted) edit the host's files in real time.

### Acceptance Criteria

- [ ] Criterion 1: Joining connects to the `collab` server over RPC and assigns the joiner a `ChannelRole` (Admin/Member/Talker/Guest/Banned).
- [ ] Criterion 2: The joiner receives the shared project's file tree and open buffers reflecting their granted `Capability` (ReadWrite/Read/ReadOnly).
- [ ] Criterion 3: A `Banned` participant cannot see or access the shared project at all.

### Technical Notes

- **Endpoint**: N/A (custom binary RPC)
- **Data Required**: `Project`, `Worktree` (data-model.md); `ChannelRole`, `Capability` (permissions.md)
- **Dependencies**: `crates/collab`, `crates/client`, `crates/call`

### Screens

- Workspace / Collab panel: join flow

### Background Logic

- BL007_RpcProtoMessageRouting: join request/state-sync routed over RPC.
- BL008_InAppNotificationCenter: host is notified in-app that a collaborator has joined.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Collaborator has a valid invite and is assigned Member role | They join | Project files load with read-write access matching the Member role |
| Error Case | Collaborator's role is Banned | They attempt to join | Connection is rejected server-side; no project data is sent to the client |

---

## US012_ChangeCollaboratorRole: Change a collaborator's role in a shared project

**Type**: ui
**Interaction**: destructive-action
**Priority**: should
**Estimate**: S

### User Story

As a collaboration Admin, I want to change another participant's role so that I can control who can edit, chat, or is removed from the session.

### Acceptance Criteria

- [ ] Criterion 1: Only participants with the Admin `ChannelRole` can change another participant's role.
- [ ] Criterion 2: Downgrading a participant to Talker/Guest immediately revokes their edit `Capability` server-side (not just hidden in the UI).
- [ ] Criterion 3: A small set of destructive git-worktree operations remain hard-denied for any non-owning participant regardless of the role granted (permissions.md Special Conditions).

### Technical Notes

- **Endpoint**: N/A (custom binary RPC)
- **Data Required**: `ChannelRole` (permissions.md)
- **Dependencies**: `crates/collab` (server-side role enforcement)

### Screens

- Collab panel: participant role dropdown

### Background Logic

- BL007_RpcProtoMessageRouting: role-change message routed to and enforced by the `collab` server.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Admin downgrades a Member to Talker | Change is applied | That participant's subsequent edit requests are rejected server-side before reaching the host |
| Error Case | A non-Admin (e.g. Member) attempts the role-change action | Action is attempted | Server rejects the change; the requester's own role is unaffected |

---

## US013_StartVoiceCall: Start a voice/video call with collaborators

**Type**: ui
**Interaction**: primary-action
**Priority**: should
**Estimate**: M

### User Story

As a developer, I want to start a voice/video call with my collaborators so that we can talk while pair-programming.

### Acceptance Criteria

- [ ] Criterion 1: Starting a call establishes a LiveKit WebRTC session for the current collaboration room.
- [ ] Criterion 2: Talker-role participants can use voice + text chat; Guest-role participants get text chat only (no microphone).
- [ ] Criterion 3: Call audio/video quality degrades gracefully (not a hard disconnect) under poor network conditions where LiveKit supports it.

### Technical Notes

- **Endpoint**: N/A (WebRTC via LiveKit SDK, not a REST endpoint)
- **Data Required**: N/A (call state is transient, not a persisted entity in data-model.md)
- **Dependencies**: `crates/livekit_api`, `crates/livekit_client`, `crates/call`

### Screens

- Call panel: start/join call controls

### Background Logic

- BL010_LiveKitCallingIntegration: WebRTC session establishment via the LiveKit SDK binding.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer and a Member collaborator are in a shared room | Developer clicks "Start Call" | A LiveKit session starts and the Member can join with audio |
| Error Case | A Guest-role participant attempts to unmute their microphone | Mic toggle is attempted | Action is blocked/hidden — Guests are restricted to text chat only |

---

## US014_RunTerminalCommand: Run a shell command in the integrated terminal

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: M

### User Story

As a developer, I want to run a shell command in the integrated terminal so that I can build, test, or script my project without leaving the editor.

### Acceptance Criteria

- [ ] Criterion 1: Opening the terminal spawns a shell process scoped to the current project's working directory.
- [ ] Criterion 2: Command stdout/stderr streams into the `Terminal` entity's view without blocking the UI thread.
- [ ] Criterion 3: Closing the terminal panel terminates its shell process cleanly.

### Technical Notes

- **Endpoint**: N/A (local process spawn, not HTTP)
- **Data Required**: `Terminal` (data-model.md)
- **Dependencies**: `crates/terminal`, `crates/terminal_view`

### Screens

- Terminal panel: shell session view

### Background Logic

- N/A (direct process I/O, no BL### item covers a per-command trigger)

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Terminal panel is open in a project directory | Developer runs `cargo build` | Build output streams live into the terminal pane |
| Error Case | The configured shell binary is missing/misconfigured | Developer opens a terminal | Panel surfaces a clear spawn error instead of an unresponsive blank pane |

---

## US015_ViewGitDiff: View a file's git diff

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: S

### User Story

As a developer, I want to view a file's git diff so that I can see what changed against HEAD.

### Acceptance Criteria

- [ ] Criterion 1: The editor gutter/git panel shows added/removed/modified line ranges against the `Repository`'s HEAD.
- [ ] Criterion 2: The diff view updates live as the buffer is further edited.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `GitStore` / `Repository` (data-model.md)
- **Dependencies**: `crates/git`

### Screens

- Git panel / Editor gutter: diff markers

### Background Logic

- N/A (direct git-diff computation, no BL### item covers this specifically)

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A tracked file has an edited hunk | Developer opens the git panel/gutter for that file | Added/removed/modified line ranges render against `Repository` HEAD |
| Error Case | The file has no git-tracked changes | Developer opens the git panel/gutter | No diff markers render; panel shows a clean-file state |

---

## US027_StageGitHunk: Stage a git hunk

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: M

### User Story

As a developer, I want to stage a specific git hunk so that I can commit changes incrementally.

### Acceptance Criteria

- [ ] Criterion 1: Staging a hunk applies only that hunk's changes to the git index, leaving the rest of the file's changes unstaged.
- [ ] Criterion 2: A non-owning collaborator without write capability cannot stage a hunk.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `GitStore` / `Repository` (data-model.md)
- **Dependencies**: `crates/git`

### Screens

- Git panel / Editor gutter: stage action

### Background Logic

- N/A (direct git-index operation, no BL### item covers per-hunk staging)

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A tracked file has 2 separate edited hunks | Developer stages hunk 1 only | Git index reflects hunk 1 staged, hunk 2 remains unstaged in the working tree |
| Error Case | Developer is a non-owning collaborator attempting a destructive worktree git operation | Action is attempted | Hard-denied regardless of assigned collaboration role (permissions.md Special Conditions) |

---

## US016_CommitStagedChanges: Commit staged changes

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: S

### User Story

As a developer, I want to commit my staged changes so that I can record a checkpoint in the project's git history.

### Acceptance Criteria

- [ ] Criterion 1: Committing with a message runs against the active `Repository`'s staged index and produces a new commit.
- [ ] Criterion 2: The commit panel clears the staged-changes list on success and reflects the new HEAD.
- [ ] Criterion 3: An empty commit message is rejected before invoking git.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `GitStore` / `Repository` (data-model.md)
- **Dependencies**: `crates/git`

### Screens

- Git panel: commit message input + commit button

### Background Logic

- N/A (direct, user-triggered git operation)

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer has staged changes and enters a commit message | Clicks "Commit" | A new commit is created and the staged list clears |
| Error Case | Developer clicks "Commit" with no message | Commit is attempted | Action is blocked with a validation message; no empty-message commit is created |

---

## US017_ChatWithAiAgent: Chat with the AI agent in a thread

**Type**: ui
**Interaction**: primary-action
**Priority**: must
**Estimate**: L

### User Story

As a developer, I want to chat with the AI agent so that I can get help writing, explaining, or refactoring code without leaving the editor.

### Acceptance Criteria

- [ ] Criterion 1: Sending a message appends it to the active `Thread` and streams the configured LLM provider's response back into the panel.
- [ ] Criterion 2: If the response includes tool calls, the `Thread`'s loop dispatches them in order and surfaces each tool's result before continuing.
- [ ] Criterion 3: The conversation (`Message` history) persists across app restarts for that `Thread`.

### Technical Notes

- **Endpoint**: N/A (vendor LLM API called from `crates/anthropic`/`open_ai`/etc., not a Zed-hosted route)
- **Data Required**: `Thread`, `Message` (data-model.md)
- **Dependencies**: `crates/agent`, `crates/language_model`, configured vendor client crate

### Screens

- Agent panel: message input + streamed response view

### Background Logic

- BL003_AgentThreadToolCallLoop: `Thread` loop awaits streamed responses and dispatches queued tool calls.
- BL009_LlmProviderClients: the per-vendor client crate performs the actual model request.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A valid API key is configured for the active provider | Developer sends a message | Response streams in and any tool calls execute and report results in order |
| Error Case | Configured provider API key is invalid/expired | Developer sends a message | Thread surfaces an auth error from the vendor client instead of hanging indefinitely |

---

## US018_AcceptInlineEditPrediction: Accept an inline AI edit prediction

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Estimate**: S

### User Story

As a developer, I want to accept an inline AI-suggested edit so that I can apply likely next changes without typing them manually.

### Acceptance Criteria

- [ ] Criterion 1: While typing, a ghost-text prediction renders inline based on surrounding buffer context.
- [ ] Criterion 2: Accepting (e.g. Tab) applies the prediction to the buffer as a normal, undoable edit.
- [ ] Criterion 3: Continuing to type without accepting dismisses the prediction without side effects.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `Buffer`, `Editor` (data-model.md)
- **Dependencies**: `crates/edit_prediction`, `crates/edit_prediction_cli` (offline eval)

### Screens

- Editor pane: inline ghost-text prediction

### Background Logic

- BL009_LlmProviderClients: prediction requests may route through a configured provider client.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A prediction is showing inline | Developer presses Tab | Prediction text is inserted into the buffer as a single undoable edit |
| Error Case | Prediction service request times out | Developer continues typing | No prediction renders; typing is never blocked waiting on the request |

---

## US019_SwitchLlmProvider: Switch the agent's active LLM provider

**Type**: ui
**Interaction**: secondary-action
**Priority**: should
**Estimate**: S

### User Story

As a developer, I want to switch which LLM provider services my agent conversations so that I can use the model/vendor of my choice.

### Acceptance Criteria

- [ ] Criterion 1: Selecting a different provider in settings changes which vendor client crate services new agent requests.
- [ ] Criterion 2: In-flight requests on the previous provider are not corrupted by the switch (completed or cleanly cancelled).
- [ ] Criterion 3: The chosen provider persists across restarts via `SettingsStore`.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `SettingsStore` (data-model.md)
- **Dependencies**: `crates/language_model`, per-vendor client crate (`crates/anthropic`, `crates/open_ai`, `crates/bedrock`, etc.)

### Screens

- Agent panel / Settings: provider selector

### Background Logic

- BL013_SettingsStoreObserver: provider choice change is persisted and observed via settings.
- BL009_LlmProviderClients: subsequent requests route to the newly selected vendor client.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Developer switches from Anthropic to a local Ollama model | Sends a new agent message | Request routes through the Ollama client crate instead of Anthropic's |
| Error Case | Newly selected provider has no credentials configured | Developer sends a message | Agent surfaces a clear "missing credentials" error rather than silently falling back |

---

## US020_ReceiveAutoUpdateNotification: Receive and apply an application auto-update

**Type**: system
**Interaction**: system-action
**Priority**: should
**Estimate**: M

### User Story

As a developer, I want the editor to check for and apply updates automatically so that I stay on the latest release without manually downloading it.

### Acceptance Criteria

- [ ] Criterion 1: A background task polls the update server periodically from app start, re-arming itself after each check.
- [ ] Criterion 2: When a new release is found, the update is downloaded and the developer is prompted to restart to apply it.
- [ ] Criterion 3: A failed poll (network error) does not crash the app and the loop still re-arms for the next interval.

### Technical Notes

- **Endpoint**: N/A (Zed's own update server, not documented as a public route)
- **Data Required**: N/A (no persisted entity in data-model.md; ephemeral poller state)
- **Dependencies**: `crates/auto_update`, `crates/scheduler`

### Screens

- N/A (background system behavior; no dedicated UI surface beyond a restart prompt)

### Background Logic

- BL001_AutoUpdatePoller: the periodic update-check loop itself.
- BL002_SchedulerTrait: underlying timer abstraction the poller sleeps on between checks.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A newer release exists on the update server | The poller's scheduled check runs | Update downloads and a restart prompt appears |
| Error Case | Update server is unreachable | The poller's scheduled check runs | Poll fails silently to the user, logs the error, and re-arms for the next interval |

---

## US021_ReadOnlyBufferRejectsEdit: Read-only buffer silently rejects an edit attempt

**Type**: system
**Interaction**: system-action
**Priority**: must
**Estimate**: S

### User Story

As a developer viewing a read-only buffer, I want my edit attempts to be silently rejected so that I never accidentally corrupt a file or view I'm not meant to modify.

### Acceptance Criteria

- [ ] Criterion 1: A buffer/multi-buffer whose `Capability` is `Read` or `ReadOnly` discards keystroke-driven edit attempts before they reach the rope.
- [ ] Criterion 2: No error dialog interrupts the developer — the rejection is silent, per permissions.md's documented behavior.
- [ ] Criterion 3: Non-edit interactions (cursor movement, selection, copy) remain fully functional on a read-only buffer.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `TextBuffer`, `Buffer` (data-model.md); `Capability` (permissions.md)
- **Dependencies**: `crates/text`, `crates/editor`

### Screens

- Editor pane: read-only buffer

### Background Logic

- BL013_SettingsStoreObserver: N/A directly; capability check is buffer-local, not settings-driven — listed for completeness only if a settings-driven read-only mode exists (e.g. vim mode); otherwise no BL### applies.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | A buffer's capability is ReadOnly (e.g. a remote collaborator's file with no write access) | Developer types a character | Nothing changes in the buffer; no error is shown |
| Error Case | N/A — rejection itself is the correct/expected behavior, not an error path | — | — |

---

## US022_ExtensionCapabilityDenied: Extension's undeclared capability request is denied

**Type**: system
**Interaction**: system-action
**Priority**: must
**Estimate**: S

### User Story

As an extension author, I want my extension's process-exec capability request to be sandboxed so that untrusted extensions can't run arbitrary commands beyond what they declared.

### Acceptance Criteria

- [ ] Criterion 1: `extension_host` checks every host-capability call (process exec, network host, npm install) against the extension's declared manifest allowlist before executing it.
- [ ] Criterion 2: A call requesting a command/host/package not present in the manifest is rejected before it runs — no partial execution.
- [ ] Criterion 3: The denial does not crash the host editor process; only the requesting extension's call fails.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `ExtensionManifest` (data-model.md)
- **Dependencies**: `crates/extension_host`, `crates/extension_api`

### Screens

- N/A (host-process-internal enforcement; no dedicated UI surface)

### Background Logic

- BL004_ExtensionHostWasmDispatch: the WASM dispatch layer where the allowlist check occurs before executing a host call.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | Extension's manifest declares `curl` as an allowed command | Extension calls `curl` | Call is permitted and executes |
| Error Case | Extension attempts to run `rm -rf` (not declared in its manifest) | Extension issues the exec call | Call is rejected before execution; host process and other extensions are unaffected |

---

## US023_SettingsChangeNotifiesObservers: Settings-store change notifies all registered observers

**Type**: system
**Interaction**: system-action
**Priority**: must
**Estimate**: S

### User Story

As a developer, I want every part of the editor that depends on a changed setting to update immediately so that my configuration change takes effect consistently across the whole app.

### Acceptance Criteria

- [ ] Criterion 1: `SettingsStore` re-parses the full settings JSON whenever it changes (file edit, or programmatic update).
- [ ] Criterion 2: Every crate implementing `impl Settings for FooSettings` (~40 call sites) is notified and re-derives its typed settings struct.
- [ ] Criterion 3: A registrant whose relevant settings key did not change does not unnecessarily re-render.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `SettingsStore` (data-model.md)
- **Dependencies**: `crates/settings`

### Screens

- N/A (cross-cutting system behavior, not a single UI surface)

### Background Logic

- BL013_SettingsStoreObserver: the observer/notification mechanism itself.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | `settings.json` is edited to change `theme` and `tab_size` | Save triggers re-parse | Theme registrant and editor-settings registrant both update; unrelated registrants are unaffected |
| Error Case | Re-parse encounters an unknown settings key | Save triggers re-parse | Unknown key is ignored/warned on, known keys still apply — parse does not abort entirely |

---

## US024_DispatchKeyboardAction: Keyboard shortcut dispatches a registered action

**Type**: system
**Interaction**: system-action
**Priority**: must
**Estimate**: S

### User Story

As a developer, I want my keyboard shortcuts to reliably trigger the right editor action so that I can work efficiently using the keymap I've configured.

### Acceptance Criteria

- [ ] Criterion 1: A keystroke is matched against the currently focused element's registered `actions!()` bindings and keymap context.
- [ ] Criterion 2: The matched action is dispatched to the nearest `.on_action()` handler up the focus/view hierarchy.
- [ ] Criterion 3: An unbound keystroke passes through without triggering any action (no silent no-op error).

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: N/A (keymap/action registration is not a persisted data-model.md entity)
- **Dependencies**: GPUI action dispatch (`actions!()`, `.on_action()`), `crates/settings` keymap config

### Screens

- N/A (global input-dispatch mechanism, not a single UI surface)

### Background Logic

- BL005_WorkspaceEventEmitterSubscribe: related entity-event plumbing that action handlers often trigger downstream (e.g. an action emits a follow-up event).

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | `cmd-s` is bound to "Save" in the active keymap context | Developer presses `cmd-s` while the editor is focused | The Save action dispatches and the buffer's file is written |
| Error Case | Developer presses a keystroke with no binding in the current context | Keystroke is pressed | No action fires; the keystroke is otherwise ignored (or passed to default text input if applicable) |

---

## US025_ResolveGitHostingPermalink: Resolve a permalink for the current git remote

**Type**: ui
**Interaction**: secondary-action
**Priority**: could
**Estimate**: S

### User Story

As a developer, I want to generate a permalink to my current file/selection on GitHub/GitLab/Bitbucket so that I can share a precise reference with teammates.

### Acceptance Criteria

- [ ] Criterion 1: The action detects the current file's git remote and maps it to the correct `GitHostingProvider` implementation (GitHub/GitLab/Bitbucket).
- [ ] Criterion 2: The generated permalink pins to the current commit SHA (not a branch name) so the link remains stable.
- [ ] Criterion 3: If no remote is configured or the provider is unrecognized, the action is unavailable rather than producing a broken link.

### Technical Notes

- **Endpoint**: N/A
- **Data Required**: `GitStore` / `Repository` (data-model.md)
- **Dependencies**: `crates/git_hosting_providers`

### Screens

- Editor pane / Git panel: "Copy Permalink" action

### Background Logic

- BL011_GitHostingProviderDetection: the provider-detection module resolving the remote to a hosting provider.

### Test Scenarios

| Scenario | Given | When | Then |
|----------|-------|------|------|
| Happy Path | File's repo has a GitHub remote and is committed | Developer selects "Copy Permalink" | A commit-SHA-pinned GitHub URL is copied to the clipboard |
| Error Case | Repo has no remote configured | Developer selects "Copy Permalink" | Action is disabled/hidden rather than generating an invalid link |

---

## Cross-Reference Validation

- [x] All US### codes are unique
- [x] All acceptance criteria are testable
- [x] All technical notes are complete (or explicitly `N/A` with reason, e.g. no HTTP endpoint / no persisted entity)
- [ ] All US### codes are referenced in FeatureList.md — N/A this pass; no upstream FeatureList generated in this session
- [x] All `ui` US### mapped to a descriptive UI-surface screen name (no SCR###/ScreenList exists upstream for this `screen_source:none` project — see Adaptation Note)
- [x] All `system` US### have at least one BL### mapped (US020, US021, US022, US023, US024 — US021 notes a partial/indirect BL relation; US024 maps to the closest related BL### since keybinding dispatch itself has no dedicated BL### item)

## Unresolved Questions

1. No upstream `feature-list.md` exists in this session — US↔F### cross-references above are deferred until a FeatureList artifact is produced.
2. US021 (read-only buffer rejection) and US024 (keybinding dispatch) have no dedicated BL### item describing the exact mechanism; the closest related BL### items are cited, but a future BL pass could add dedicated items for "capability-gated edit rejection" and "action-dispatch resolution" if greater precision is needed.
3. Screens are named descriptively (Workspace, Project Panel, Editor pane, etc.) rather than as SCR### codes, since no `screen-list.md` exists for this native desktop app (`screen_source:none`). If a future pass introduces a screen inventory for GPUI `Render` entities, these US### should be re-mapped to formal SCR### codes.
