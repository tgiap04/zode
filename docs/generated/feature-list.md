<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Feature List

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

**Code Format**: All codes follow `F###_NameSlug` format (e.g., F001_ProjectFolderNavigation)
**User Story Code Format**: All US codes follow `US###_NameSlug` format (e.g., US001_OpenProjectFolder)
**Background Logic Code Format**: All BL codes follow `BL###_NameSlug` format (e.g., BL001_AutoUpdatePoller)

**Feature Types**:
- `ui` - Feature has a UI surface, no dedicated background-logic item drives it
- `background` - Feature only has background logic (BL###), no dedicated UI surface
- `mixed` - Feature has both a UI surface and dedicated background logic (BL###)

**Adaptation Note (no ScreenList upstream)**: Zed is a native desktop app; no `screen-list.md`/`SCR###`
registry exists for this project (`screen_source:none`, per `_session-context.md` and
`user-stories.md` Adaptation Note). Per this session's instruction, the **Related Screens** field
below is replaced with **Related UI Surfaces** naming the descriptive GPUI `Render`-implementing
surface (e.g. "Editor pane", "Project Panel") instead of a `SCR###` code. No SCR### codes appear
anywhere in this document.

**Related Data Models / Permissions note**: Reconciled against `data-model.md` (16 entities, no
MODEL### codes assigned — entity names cited directly) and `permissions-matrix.md` (PERM001–PERM008,
cited under Related Permissions on F002, F006, F007, F008, F011, F018; the remaining features
genuinely have no associated PERM### and are marked N/A).

## Feature Hierarchy

**Note**: Features are sorted by priority from highest to lowest (P0 → P1 → P2 → P3).

| Code | Name | Type | Language | Workspace | Priority |
|------|------|------|----------|-----------|----------|
| F001_ProjectFolderNavigation | Project folder navigation | mixed | Rust | zed (crates/workspace, worktree, project_panel) | P0 |
| F002_BufferTextEditing | Buffer text editing | mixed | Rust | zed (crates/text, editor, language) | P0 |
| F003_LspDiagnostics | LSP diagnostics feedback | mixed | Rust | zed (crates/project — LspStore) | P0 |
| F004_LspCompletions | LSP completion suggestions | mixed | Rust | zed (crates/project — LspStore) | P0 |
| F005_LiveSettingsEditing | Live settings.json editing | mixed | Rust | zed (crates/settings) | P0 |
| F006_ExtensionInstallation | Third-party extension installation | mixed | Rust | zed (crates/extension_host, extension_api) | P0 |
| F007_ProjectSharing | Project sharing for collaboration | mixed | Rust | zed (crates/collab, client, call, channel) | P0 |
| F008_JoinSharedProject | Join a shared project | mixed | Rust | zed (crates/collab, client, call) | P0 |
| F009_IntegratedTerminal | Integrated terminal command execution | ui | Rust | zed (crates/terminal, terminal_view) | P0 |
| F010_GitDiffViewing | Git diff viewing | ui | Rust | zed (crates/git) | P0 |
| F011_GitHunkStaging | Git hunk staging | ui | Rust | zed (crates/git) | P0 |
| F012_GitCommit | Git commit of staged changes | ui | Rust | zed (crates/git) | P0 |
| F013_AiAgentChat | AI agent chat thread | mixed | Rust | zed (crates/agent, language_model) | P0 |
| F014_KeyboardActionDispatch | Keyboard shortcut action dispatch | background | Rust | zed (GPUI core, crates/settings keymap) | P0 |
| F015_MultiBufferSearchResults | Multi-buffer search results view | ui | Rust | zed (crates/multi_buffer) | P1 |
| F016_LspCodeActions | LSP code action application | mixed | Rust | zed (crates/project) | P1 |
| F017_EditorThemeCustomization | Editor theme customization | mixed | Rust | zed (crates/theme) | P1 |
| F018_CollaboratorRoleManagement | Collaborator role management | mixed | Rust | zed (crates/collab) | P1 |
| F019_VoiceVideoCall | Voice/video call with collaborators | mixed | Rust | zed (crates/livekit_api, livekit_client, call) | P1 |
| F020_InlineEditPrediction | Inline AI edit prediction acceptance | mixed | Rust | zed (crates/edit_prediction) | P1 |
| F021_LlmProviderSwitching | LLM provider switching | mixed | Rust | zed (crates/language_model, per-vendor clients) | P1 |
| F022_AutoUpdate | Application auto-update | background | Rust | zed (crates/auto_update, scheduler) | P1 |
| F023_GitHostingPermalink | Git hosting permalink resolution | mixed | Rust | zed (crates/git_hosting_providers) | P2 |

## Feature Details

### F001_ProjectFolderNavigation: Project folder navigation

**Type**: mixed
**Description**: Developer opens a project folder as a `Worktree` and browses its file tree in the Project Panel to locate and open files for editing. Input: folder pick / directory expand. Process: `Worktree` registration + incremental filesystem watch. Output: rendered file tree, opened `Buffer` on file selection.

**Workspace**: zed (crates/workspace, crates/worktree, crates/project_panel)
**Languages**: Rust
**Components**: `Workspace`, `Worktree`, `Entry`, Project Panel view

**Related UI Surfaces**:
- Workspace: main application window (open-folder action)
- Project Panel: file tree dock

**Related User Stories**:
- US001_OpenProjectFolder: Open a project folder
- US002_BrowseWorktreeFiles: Browse worktree files in the project panel

**Related APIs/Routes**:
- N/A — native OS file dialog + filesystem watcher, no HTTP route

**Related Data Models**:
- Project, Worktree, Entry, ProjectPanel

**Related Background Logic**:
- BL005_WorkspaceEventEmitterSubscribe: `Workspace` emits worktree-added/entry-tree-updated events consumed by panel views
- BL012_StandaloneCliBinaries: `zed <path>` CLI entry point (`crates/cli`) is one way a project folder is opened

**Related Permissions**:
- N/A — opening a local project folder is unrestricted; no PERM### gate applies

---

### F002_BufferTextEditing: Buffer text editing

**Type**: mixed
**Description**: Developer edits an open file's text; keystrokes mutate the buffer's `Rope` when the buffer's `Capability` is `ReadWrite`, and are silently discarded when it is `Read`/`ReadOnly`. Input: keystrokes. Process: capability check → rope mutation → LSP `didChange` notify. Output: re-rendered `Editor` view, undo-stack entry.

**Workspace**: zed (crates/text, crates/editor, crates/language)
**Languages**: Rust
**Components**: `TextBuffer`, `Buffer`, `Editor`, `Capability` check

**Related UI Surfaces**:
- Editor pane: active buffer view (read-write and read-only states)

**Related User Stories**:
- US003_EditBufferText: Edit an open file's text
- US021_ReadOnlyBufferRejectsEdit: Read-only buffer silently rejects an edit attempt

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- TextBuffer, Buffer (see PERM004_BufferCapabilityGate for the `Capability` discriminator gating this feature)

**Related Background Logic**:
- BL006_LspRequestDispatch: debounced buffer edits forwarded to the language server as `textDocument/didChange`

**Related Permissions**:
- PERM004_BufferCapabilityGate: `Buffer`/`MultiBuffer` capability (`ReadWrite`/`Read`/`ReadOnly`) gates whether an edit is accepted

---

### F015_MultiBufferSearchResults: Multi-buffer search results view

**Type**: ui
**Description**: Developer opens project-wide search results as a single scrollable `MultiBuffer` to review/edit matches across files without opening each individually. Input: project search query. Process: aggregate matching excerpts across `TextBuffer`s. Output: one scrollable multi-buffer view; edits write back to originating files.

**Workspace**: zed (crates/multi_buffer)
**Languages**: Rust
**Components**: `MultiBuffer`, `Buffer`, search-results excerpt view

**Related UI Surfaces**:
- Editor pane / Search results: multi-buffer excerpt view

**Related User Stories**:
- US004_OpenMultiBufferSearchResults: Open search results as a multi-buffer

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- MultiBuffer, Buffer

**Related Background Logic**:
- N/A — direct user-triggered aggregation, no BL### applies per user-stories.md

**Related Permissions**:
- N/A

---

### F003_LspDiagnostics: LSP diagnostics feedback

**Type**: mixed
**Description**: Developer sees inline diagnostics from the attached language server while typing. Input: buffer change. Process: `textDocument/didChange` sent off-thread → `publishDiagnostics` received via `LspStore`. Output: inline error/warning markers in the editor.

**Workspace**: zed (crates/project — LspStore)
**Languages**: Rust
**Components**: `LspStore`, `Buffer`, `Editor` diagnostic markers

**Related UI Surfaces**:
- Editor pane: inline diagnostic markers

**Related User Stories**:
- US005_ReceiveLspDiagnostics: Receive LSP diagnostics while typing

**Related APIs/Routes**:
- N/A — LSP over stdio/socket, not HTTP

**Related Data Models**:
- Buffer, Editor

**Related Background Logic**:
- BL006_LspRequestDispatch: `LspStore::handle_lsp_*` handlers dispatch and route LSP protocol messages

**Related Permissions**:
- N/A

---

### F004_LspCompletions: LSP completion suggestions

**Type**: mixed
**Description**: Developer receives completion suggestions from the attached language server while typing. Input: cursor position + buffer context. Process: `textDocument/completion` request off-thread via `LspStore`. Output: completion popover at cursor.

**Workspace**: zed (crates/project — LspStore)
**Languages**: Rust
**Components**: `LspStore`, `Buffer`, completion popover

**Related UI Surfaces**:
- Editor pane: completion popover

**Related User Stories**:
- US026_ReceiveLspCompletions: Receive LSP completion suggestions while typing

**Related APIs/Routes**:
- N/A — LSP over stdio/socket, not HTTP

**Related Data Models**:
- Buffer, Editor

**Related Background Logic**:
- BL006_LspRequestDispatch: `LspStore::handle_lsp_*` handlers dispatch and route LSP protocol messages

**Related Permissions**:
- N/A

---

### F016_LspCodeActions: LSP code action application

**Type**: mixed
**Description**: Developer applies a language-server-suggested code action to fix an issue or refactor. Input: code-action menu trigger. Process: `textDocument/codeAction` request → selected `WorkspaceEdit` applied atomically. Output: buffer(s) updated as one undo-stack entry.

**Workspace**: zed (crates/project)
**Languages**: Rust
**Components**: code-action lightbulb menu, `WorkspaceEdit` applier

**Related UI Surfaces**:
- Editor pane: code-action lightbulb menu

**Related User Stories**:
- US006_ApplyLspCodeAction: Apply an LSP code action

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- Buffer

**Related Background Logic**:
- BL006_LspRequestDispatch: `handle_apply_code_action` routes the accepted action back through the LSP dispatch table

**Related Permissions**:
- N/A

---

### F017_EditorThemeCustomization: Editor theme customization

**Type**: mixed
**Description**: Developer selects a different color theme; every `Render`-implementing surface re-renders immediately with the new palette and the choice persists across restarts. Input: theme-picker selection. Process: swap active `Theme`/`ThemeFamily` → persist to settings → notify observers. Output: re-rendered UI in the new theme.

**Workspace**: zed (crates/theme)
**Languages**: Rust
**Components**: `Theme`, `ThemeFamily`, theme picker

**Related UI Surfaces**:
- Settings / Theme picker

**Related User Stories**:
- US007_ChangeEditorTheme: Change the editor's color theme

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- Theme, ThemeFamily

**Related Background Logic**:
- BL013_SettingsStoreObserver: theme selection persisted to settings JSON, re-parsed, and observers re-render

**Related Permissions**:
- N/A

---

### F005_LiveSettingsEditing: Live settings.json editing

**Type**: mixed
**Description**: Developer edits `settings.json` and every dependent registrant applies the new value live, without a restart. Input: settings-buffer save. Process: `SettingsStore` re-parses JSON → notifies each `impl Settings for` registrant. Output: live behavior change across the app; malformed JSON keeps last-valid settings.

**Workspace**: zed (crates/settings)
**Languages**: Rust
**Components**: `SettingsStore`, ~40 `impl Settings for` registrants

**Related UI Surfaces**:
- Settings editor: `settings.json` buffer

**Related User Stories**:
- US008_EditSettingsJson: Edit a setting in `settings.json` and see it take effect live
- US023_SettingsChangeNotifiesObservers: Settings-store change notifies all registered observers

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- SettingsStore

**Related Background Logic**:
- BL013_SettingsStoreObserver: `SettingsStore` re-parses on change and notifies registrant call sites

**Related Permissions**:
- N/A

---

### F006_ExtensionInstallation: Third-party extension installation

**Type**: mixed
**Description**: Developer installs a third-party WASM extension; its manifest capabilities become its sandboxed allowlist before any code runs, and undeclared host-capability calls are rejected. Input: "Install" action. Process: download + compile `wasm32-wasip2` module → load into `extension_host` with manifest allowlist. Output: extension appears installed and usable; denied calls fail without crashing the host.

**Workspace**: zed (crates/extension_host, crates/extension_api)
**Languages**: Rust, WASM (guest extensions)
**Components**: `ExtensionManifest`, extension_host sandbox runtime

**Related UI Surfaces**:
- Extensions panel: install/manage list

**Related User Stories**:
- US009_InstallExtension: Install a third-party extension
- US022_ExtensionCapabilityDenied: Extension's undeclared capability request is denied

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- ExtensionManifest

**Related Background Logic**:
- BL004_ExtensionHostWasmDispatch: sandboxed WASM calls dispatched in the background, checked against the manifest allowlist, and routed back to the main thread

**Related Permissions**:
- PERM001_ExtensionProcessExecCapability: gates `ProcessExec` capability grants against the manifest's declared allowlist
- PERM002_ExtensionDownloadFileCapability: gates `DownloadFile` capability grants
- PERM003_ExtensionNpmInstallCapability: gates `NpmInstallPackage` capability grants

---

### F007_ProjectSharing: Project sharing for collaboration

**Type**: mixed
**Description**: Developer (host) shares the open project so a collaborator can join and pair-program in real time, without restricting the host's own edits. Input: "Share Project" action. Process: register `Project` with `collab` server over RPC. Output: invite/room identifier produced for a collaborator to join.

**Workspace**: zed (crates/collab, crates/client, crates/call, crates/channel)
**Languages**: Rust
**Components**: `Project`, `Workspace`, collab RPC connection

**Related UI Surfaces**:
- Workspace / Collab panel: share action + invite UI

**Related User Stories**:
- US010_ShareProjectForCollaboration: Share a project so others can collaborate

**Related APIs/Routes**:
- N/A — custom binary RPC over crates/proto, crates/rpc, not HTTP

**Related Data Models**:
- Project, Workspace

**Related Background Logic**:
- BL007_RpcProtoMessageRouting: share request/response routed as a typed proto message over the persistent RPC connection

**Related Permissions**:
- PERM005_CollabMutatingProjectRequestGuard: gates which mutating RPC requests the host's server accepts once a project is shared

---

### F008_JoinSharedProject: Join a shared project

**Type**: mixed
**Description**: Collaborator joins a shared project, is assigned a `ChannelRole`, and receives the file tree/open buffers matching their granted `Capability`; a `Banned` participant is rejected server-side. Input: join/invite action. Process: connect to `collab` server over RPC → role assignment → state sync. Output: project state rendered with role-appropriate access, or a rejected connection.

**Workspace**: zed (crates/collab, crates/client, crates/call)
**Languages**: Rust
**Components**: `ChannelRole`, `Capability`, RPC join handshake

**Related UI Surfaces**:
- Workspace / Collab panel: join flow

**Related User Stories**:
- US011_JoinSharedProject: Join a shared project as a collaborator

**Related APIs/Routes**:
- N/A — custom binary RPC

**Related Data Models**:
- Project, Worktree

**Related Background Logic**:
- BL007_RpcProtoMessageRouting: join request/state-sync routed over RPC
- BL008_InAppNotificationCenter: host is notified in-app that a collaborator has joined

**Related Permissions**:
- PERM006_CollabReadOnlyProjectRequestGuard: gates which read-only RPC requests a joining collaborator (including Guest role) may issue
- PERM007_ChannelRoleAccessControl: `ChannelRole` (Admin/Member/Talker/Guest/Banned) assigned on join determines granted `Capability`

---

### F018_CollaboratorRoleManagement: Collaborator role management

**Type**: mixed
**Description**: Collaboration Admin changes another participant's role, immediately revoking/granting edit `Capability` server-side; a fixed set of destructive git-worktree operations remain hard-denied for any non-owning participant regardless of role. Input: role-dropdown selection. Process: Admin-only RPC role-change request enforced server-side. Output: participant's effective capability updates immediately.

**Workspace**: zed (crates/collab)
**Languages**: Rust
**Components**: `ChannelRole`, server-side role enforcement

**Related UI Surfaces**:
- Collab panel: participant role dropdown

**Related User Stories**:
- US012_ChangeCollaboratorRole: Change a collaborator's role in a shared project

**Related APIs/Routes**:
- N/A — custom binary RPC

**Related Data Models**:
- N/A (transient RPC state)

**Related Background Logic**:
- BL007_RpcProtoMessageRouting: role-change message routed to and enforced by the collab server

**Related Permissions**:
- PERM007_ChannelRoleAccessControl: role-change is gated to the Admin role; the resulting `ChannelRole` determines the collaborator's `can_edit_projects`/`can_read_projects`/`can_use_microphone`/`can_see_channel` grants
- PERM008_DisallowGuestRequestGuard: hard-denies a fixed set of destructive git-worktree operations to non-host roles regardless of any role change

---

### F019_VoiceVideoCall: Voice/video call with collaborators

**Type**: mixed
**Description**: Developer starts a voice/video call with collaborators in the current room; Talker-role participants get voice + text, Guest-role participants get text only. Input: "Start Call"/"Join Call". Process: establish LiveKit WebRTC session for the room. Output: live audio/video session; degrades gracefully under poor network rather than hard-disconnecting.

**Workspace**: zed (crates/livekit_api, crates/livekit_client, crates/call)
**Languages**: Rust
**Components**: LiveKit WebRTC session, Call panel

**Related UI Surfaces**:
- Call panel: start/join call controls

**Related User Stories**:
- US013_StartVoiceCall: Start a voice/video call with collaborators

**Related APIs/Routes**:
- N/A — WebRTC via LiveKit SDK, not a REST endpoint

**Related Data Models**:
- N/A (call state is transient, not a persisted entity)

**Related Background Logic**:
- BL010_LiveKitCallingIntegration: WebRTC session establishment via the LiveKit SDK binding

**Related Permissions**:
- PERM007_ChannelRoleAccessControl: `can_use_microphone` grant on the collaborator's `ChannelRole` gates whether they may unmute on the call

---

### F009_IntegratedTerminal: Integrated terminal command execution

**Type**: ui
**Description**: Developer runs a shell command in an integrated terminal scoped to the project's working directory. Input: terminal command input. Process: spawn shell process, stream stdout/stderr off the UI thread. Output: live command output in the terminal pane; process terminates cleanly on panel close.

**Workspace**: zed (crates/terminal, crates/terminal_view)
**Languages**: Rust
**Components**: `Terminal` entity, terminal panel view

**Related UI Surfaces**:
- Terminal panel: shell session view

**Related User Stories**:
- US014_RunTerminalCommand: Run a shell command in the integrated terminal

**Related APIs/Routes**:
- N/A — local process spawn, not HTTP

**Related Data Models**:
- Terminal

**Related Background Logic**:
- N/A — direct process I/O, no BL### item covers a per-command trigger per user-stories.md

**Related Permissions**:
- N/A

---

### F010_GitDiffViewing: Git diff viewing

**Type**: ui
**Description**: Developer views a file's git diff against HEAD, updating live as the buffer is further edited. Input: open git panel/gutter for a file. Process: compute added/removed/modified line ranges against `Repository` HEAD. Output: diff markers in the gutter/git panel.

**Workspace**: zed (crates/git)
**Languages**: Rust
**Components**: `GitStore`, `Repository`, gutter diff markers

**Related UI Surfaces**:
- Git panel / Editor gutter: diff markers

**Related User Stories**:
- US015_ViewGitDiff: View a file's git diff

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- GitStore, Repository

**Related Background Logic**:
- N/A — direct git-diff computation, no BL### item covers this specifically per user-stories.md

**Related Permissions**:
- N/A

---

### F011_GitHunkStaging: Git hunk staging

**Type**: ui
**Description**: Developer stages a specific git hunk to commit changes incrementally, leaving the rest of the file's changes unstaged. Input: "Stage Hunk" inline action. Process: apply the hunk-level change to the git index. Output: git index reflects only the staged hunk; non-owning collaborators without write capability are denied.

**Workspace**: zed (crates/git)
**Languages**: Rust
**Components**: `GitStore`, `Repository`, hunk-level index operation

**Related UI Surfaces**:
- Git panel / Editor gutter: stage action

**Related User Stories**:
- US027_StageGitHunk: Stage a git hunk

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- GitStore, Repository

**Related Background Logic**:
- N/A — direct git-index operation, no BL### item covers per-hunk staging per user-stories.md

**Related Permissions**:
- PERM008_DisallowGuestRequestGuard: hard-denies staging (and other destructive worktree git operations) to a non-owning collaborator regardless of assigned `ChannelRole`

---

### F012_GitCommit: Git commit of staged changes

**Type**: ui
**Description**: Developer commits staged changes with a message, recording a checkpoint in the project's git history. Input: commit message + "Commit" click. Process: run commit against the active `Repository`'s staged index; reject empty messages before invoking git. Output: new commit created, staged list clears, HEAD updates.

**Workspace**: zed (crates/git)
**Languages**: Rust
**Components**: `GitStore`, `Repository`, commit panel

**Related UI Surfaces**:
- Git panel: commit message input + commit button

**Related User Stories**:
- US016_CommitStagedChanges: Commit staged changes

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- GitStore, Repository

**Related Background Logic**:
- N/A — direct, user-triggered git operation per user-stories.md

**Related Permissions**:
- N/A

---

### F023_GitHostingPermalink: Git hosting permalink resolution

**Type**: mixed
**Description**: Developer generates a stable, commit-SHA-pinned permalink to the current file/selection on the detected git hosting provider (GitHub/GitLab/Bitbucket) to share with teammates. Input: "Copy Permalink" action. Process: detect git remote → map to `GitHostingProvider` → pin to commit SHA. Output: permalink URL copied to clipboard, or action disabled if no remote/unrecognized provider.

**Workspace**: zed (crates/git_hosting_providers)
**Languages**: Rust
**Components**: `GitHostingProvider` detection

**Related UI Surfaces**:
- Editor pane / Git panel: "Copy Permalink" action

**Related User Stories**:
- US025_ResolveGitHostingPermalink: Resolve a permalink for the current git remote

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- GitStore, Repository

**Related Background Logic**:
- BL011_GitHostingProviderDetection: provider-detection module resolving the remote to a hosting provider

**Related Permissions**:
- N/A

---

### F013_AiAgentChat: AI agent chat thread

**Type**: mixed
**Description**: Developer chats with the AI agent in a persistent thread; the model's tool calls dispatch in order and results surface before continuing. Input: message input + send. Process: append `Message` to `Thread` → stream vendor LLM response → dispatch queued tool calls. Output: streamed response in the panel; conversation persists across restarts.

**Workspace**: zed (crates/agent, crates/language_model, per-vendor client crates)
**Languages**: Rust
**Components**: `Thread`, `Message`, tool-call dispatch loop

**Related UI Surfaces**:
- Agent panel: message input + streamed response view

**Related User Stories**:
- US017_ChatWithAiAgent: Chat with the AI agent in a thread

**Related APIs/Routes**:
- N/A — vendor LLM API called from crates/anthropic, open_ai, etc., not a Zed-hosted route

**Related Data Models**:
- Thread, Message

**Related Background Logic**:
- BL003_AgentThreadToolCallLoop: `Thread` loop awaits streamed responses and dispatches queued tool calls
- BL009_LlmProviderClients: the per-vendor client crate performs the actual model request

**Related Permissions**:
- N/A

---

### F020_InlineEditPrediction: Inline AI edit prediction acceptance

**Type**: mixed
**Description**: Developer accepts an inline AI-suggested next edit while typing, applying it as a normal undoable buffer edit. Input: ghost-text prediction rendered inline + accept keystroke (e.g. Tab). Process: prediction request based on surrounding buffer context; accept applies edit, continuing to type dismisses it. Output: buffer updated (or unaffected) with no typing block on request latency.

**Workspace**: zed (crates/edit_prediction, crates/edit_prediction_cli)
**Languages**: Rust
**Components**: ghost-text prediction renderer

**Related UI Surfaces**:
- Editor pane: inline ghost-text prediction

**Related User Stories**:
- US018_AcceptInlineEditPrediction: Accept an inline AI edit prediction

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- Buffer, Editor

**Related Background Logic**:
- BL009_LlmProviderClients: prediction requests may route through a configured provider client

**Related Permissions**:
- N/A

---

### F021_LlmProviderSwitching: LLM provider switching

**Type**: mixed
**Description**: Developer switches which LLM provider services agent conversations; in-flight requests on the previous provider complete or cancel cleanly, and the choice persists across restarts. Input: provider-selector choice. Process: update `SettingsStore` → subsequent requests route to the newly selected vendor client crate. Output: new agent requests serviced by the selected provider.

**Workspace**: zed (crates/language_model, per-vendor client crates)
**Languages**: Rust
**Components**: provider selector, vendor client routing

**Related UI Surfaces**:
- Agent panel / Settings: provider selector

**Related User Stories**:
- US019_SwitchLlmProvider: Switch the agent's active LLM provider

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- SettingsStore

**Related Background Logic**:
- BL013_SettingsStoreObserver: provider choice change is persisted and observed via settings
- BL009_LlmProviderClients: subsequent requests route to the newly selected vendor client

**Related Permissions**:
- N/A

---

### F022_AutoUpdate: Application auto-update

**Type**: background
**Description**: The application periodically checks for and applies updates in the background so the developer stays current without a manual download. Input: none (time-driven). Process: scheduled poller checks the update server, re-arming after each check regardless of success/failure. Output: on a new release, update downloads and a restart prompt appears.

**Workspace**: zed (crates/auto_update, crates/scheduler)
**Languages**: Rust
**Components**: update poller, scheduler timer abstraction

**Related UI Surfaces**:
- N/A — background system behavior; no dedicated UI surface beyond a restart prompt

**Related User Stories**:
- US020_ReceiveAutoUpdateNotification: Receive and apply an application auto-update

**Related APIs/Routes**:
- N/A — Zed's own update server, not a documented public route

**Related Data Models**:
- N/A (ephemeral poller state, no persisted entity)

**Related Background Logic**:
- BL001_AutoUpdatePoller: the periodic update-check loop itself
- BL002_SchedulerTrait: underlying timer abstraction the poller sleeps on between checks

**Related Permissions**:
- N/A

---

### F014_KeyboardActionDispatch: Keyboard shortcut action dispatch

**Type**: background
**Description**: A keystroke is matched against the focused element's registered `actions!()` bindings and keymap context, then dispatched to the nearest handler up the focus/view hierarchy; unbound keystrokes pass through without triggering anything. Input: keystroke. Process: keymap-context match → action dispatch to `.on_action()` handler. Output: matched action executes (e.g. Save writes the buffer); unbound keystrokes are no-ops.

**Workspace**: zed (GPUI core, crates/settings keymap config)
**Languages**: Rust
**Components**: `actions!()` registrations, `.on_action()` handlers, keymap context

**Related UI Surfaces**:
- N/A — global input-dispatch mechanism, not a single UI surface

**Related User Stories**:
- US024_DispatchKeyboardAction: Keyboard shortcut dispatches a registered action

**Related APIs/Routes**:
- N/A

**Related Data Models**:
- N/A (keymap/action registration is not a persisted entity)

**Related Background Logic**:
- BL005_WorkspaceEventEmitterSubscribe: related entity-event plumbing that action handlers often trigger downstream

**Related Permissions**:
- N/A

---

## Summary

- **Total Features**: 23
- **Total UI Surfaces (descriptive, no SCR###)**: 18 named surfaces across 20 features (3 features are background-only: F022, F014; partial: F019 has one dedicated surface)
- **Total User Stories**: 27 (US001–US027, all referenced)
- **Total Routes**: 0 (native desktop app; no HTTP routes in this codebase per system-overview.md)
- **Total Data Models**: 18 distinct entity names cited, covering all 16 top-level headings in data-model.md (Project, Worktree, Entry, TextBuffer, Buffer, MultiBuffer, Editor, Theme, ThemeFamily, SettingsStore, ExtensionManifest, Workspace, GitStore, Repository, ProjectPanel, Terminal, Thread, Message — Theme/ThemeFamily and GitStore/Repository are each one heading in data-model.md, counted here as 2 names apiece) — data-model.md does not assign MODEL### codes (entities are heading-identified only), so entity names are cited directly rather than via a code. `Capability` and `ChannelRole` are NOT top-level entities (Capability is a discriminator field on Buffer/MultiBuffer; ChannelRole is a permissions-domain type documented in permissions-matrix.md) and have been re-cited accordingly rather than listed as entities.
- **Total Background Logic**: 13 distinct BL### codes referenced (BL001–BL013, all referenced across features)
- **Total Permissions**: 8 formally coded (PERM001–PERM008, verified against permissions-matrix.md), cited under Related Permissions on F002, F006, F007, F008, F011, F018, F019; remaining features genuinely have no PERM### gate
- **Languages Detected**: Rust (core), WASM (guest extensions via wasm32-wasip2)

## Cross-Reference Validation

- [x] All F### codes are unique (F001–F023, contiguous)
- [x] All F### codes are referenced in user-stories.md by their constituent US###
- [x] No SCR### codes appear (screen_source:none — descriptive UI-surface names used instead, per task instruction)
- [x] All user story references are valid (US001–US027, each appears in exactly one F###)
- [x] All route references are N/A with reason (no HTTP routes in this codebase)
- [x] All data model references verified against data-model.md's 16 entities (cited by name; data-model.md assigns no MODEL### codes)
- [x] All background logic references are valid (BL001–BL013, matching user-stories.md BL citations)
- [x] All permission references are valid PERM### codes (PERM001–PERM008, verified against permissions-matrix.md and cross-referenced from F002/F006/F007/F008/F011/F018)
- [x] Every US has a parent feature (F###) — verified below
- [x] Every background logic BL### cited maps to at least one feature (F###)

### US### → F### Coverage Matrix

| US### | F### |
|-------|------|
| US001_OpenProjectFolder | F001_ProjectFolderNavigation |
| US002_BrowseWorktreeFiles | F001_ProjectFolderNavigation |
| US003_EditBufferText | F002_BufferTextEditing |
| US004_OpenMultiBufferSearchResults | F015_MultiBufferSearchResults |
| US005_ReceiveLspDiagnostics | F003_LspDiagnostics |
| US006_ApplyLspCodeAction | F016_LspCodeActions |
| US007_ChangeEditorTheme | F017_EditorThemeCustomization |
| US008_EditSettingsJson | F005_LiveSettingsEditing |
| US009_InstallExtension | F006_ExtensionInstallation |
| US010_ShareProjectForCollaboration | F007_ProjectSharing |
| US011_JoinSharedProject | F008_JoinSharedProject |
| US012_ChangeCollaboratorRole | F018_CollaboratorRoleManagement |
| US013_StartVoiceCall | F019_VoiceVideoCall |
| US014_RunTerminalCommand | F009_IntegratedTerminal |
| US015_ViewGitDiff | F010_GitDiffViewing |
| US016_CommitStagedChanges | F012_GitCommit |
| US017_ChatWithAiAgent | F013_AiAgentChat |
| US018_AcceptInlineEditPrediction | F020_InlineEditPrediction |
| US019_SwitchLlmProvider | F021_LlmProviderSwitching |
| US020_ReceiveAutoUpdateNotification | F022_AutoUpdate |
| US021_ReadOnlyBufferRejectsEdit | F002_BufferTextEditing |
| US022_ExtensionCapabilityDenied | F006_ExtensionInstallation |
| US023_SettingsChangeNotifiesObservers | F005_LiveSettingsEditing |
| US024_DispatchKeyboardAction | F014_KeyboardActionDispatch |
| US025_ResolveGitHostingPermalink | F023_GitHostingPermalink |
| US026_ReceiveLspCompletions | F004_LspCompletions |
| US027_StageGitHunk | F011_GitHunkStaging |

## Unresolved Questions

1. Data-model entities are reconciled against `data-model.md` (16 entities); that artifact assigns no `MODEL###` codes (heading-identified only), so entity names are cited directly rather than via a code — no further action needed unless a future pass introduces MODEL### numbering.
2. Permission references are reconciled against `permissions-matrix.md`'s PERM001–PERM008 registry — resolved, no longer open.
3. F007_ProjectSharing and F008_JoinSharedProject were kept as two separate features (host-share vs. guest-join) rather than merged into one "Real-time collaboration session" feature, since each is a distinct single user action performed by a distinct role (developer vs. collaborator) — flagging this split for confirmation if a coarser grouping is preferred downstream.
4. F001_ProjectFolderNavigation merges US001 (open folder) and US002 (browse tree) as one feature since they form a single continuous input→process→output flow (pick folder → watch/populate tree → open file); flagging in case a stricter single-US-per-feature policy is wanted instead.
