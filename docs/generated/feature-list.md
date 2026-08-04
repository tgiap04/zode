<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->
# Feature List

**Rewritten 2026-08-04** against the post-fork tree (187 packages / 178 crates). Removed 8 of the
original 23 features entirely — F007_ProjectSharing, F008_JoinSharedProject,
F018_CollaboratorRoleManagement, F019_VoiceVideoCall (all collaboration), F013_AiAgentChat,
F020_InlineEditPrediction, F021_LlmProviderSwitching (all AI), and F022_AutoUpdate — since the
crates that implemented them (`collab`, `call`, `channel`, `livekit_*`, `agent`, `language_model`,
`edit_prediction`, `auto_update`) no longer exist. The 15 surviving features had their BL###/
PERM### citations updated to match the renumbering in `behavior-logic.md`/`permissions-matrix.md`
after those files' own removals.

**Project**: Zode
**Analysis Scope**: Full monorepo — native GPUI desktop editor (`generic-source` profile, `screen_source:none`)

**Code Format**: All codes follow `F###_NameSlug` format (e.g., F001_ProjectFolderNavigation)
**User Story Code Format**: All US codes follow `US###_NameSlug` format (e.g., US001_OpenProjectFolder)
**Background Logic Code Format**: All BL codes follow `BL###_NameSlug` format (e.g., BL001_AutoUpdatePoller)

**Feature Types**:
- `ui` - Feature has a UI surface, no dedicated background-logic item drives it
- `background` - Feature only has background logic (BL###), no dedicated UI surface
- `mixed` - Feature has both a UI surface and dedicated background logic (BL###)

**Adaptation Note (no ScreenList upstream)**: Zode is a native desktop app; no `screen-list.md`/`SCR###`
registry exists for this project (`screen_source:none`, per `_session-context.md` and
`user-stories.md` Adaptation Note). Per this session's instruction, the **Related Screens** field
below is replaced with **Related UI Surfaces** naming the descriptive GPUI `Render`-implementing
surface (e.g. "Editor pane", "Project Panel") instead of a `SCR###` code. No SCR### codes appear
anywhere in this document.

**Related Data Models / Permissions note**: Reconciled against `data-model.md`/`entities.md` (14
entities, no MODEL### codes assigned — entity names cited directly) and `permissions-matrix.md`
(PERM001–PERM004, cited under Related Permissions on F002 and F006; the remaining features
genuinely have no associated PERM### and are marked N/A).

## Feature Hierarchy

**Note**: Features are sorted by priority from highest to lowest (P0 → P1 → P2 → P3).

| Code | Name | Type | Language | Workspace | Priority |
|------|------|------|----------|-----------|----------|
| F001_ProjectFolderNavigation | Project folder navigation | mixed | Rust | zode (crates/workspace, worktree, project_panel) | P0 |
| F002_BufferTextEditing | Buffer text editing | mixed | Rust | zode (crates/text, editor, language) | P0 |
| F003_LspDiagnostics | LSP diagnostics feedback | mixed | Rust | zode (crates/project — LspStore) | P0 |
| F004_LspCompletions | LSP completion suggestions | mixed | Rust | zode (crates/project — LspStore) | P0 |
| F005_LiveSettingsEditing | Live settings.json editing | mixed | Rust | zode (crates/settings) | P0 |
| F006_ExtensionInstallation | Third-party extension installation | mixed | Rust | zode (crates/extension_host, extension_api) | P0 |
| F009_IntegratedTerminal | Integrated terminal command execution | ui | Rust | zode (crates/terminal, terminal_view) | P0 |
| F010_GitDiffViewing | Git diff viewing | ui | Rust | zode (crates/git) | P0 |
| F011_GitHunkStaging | Git hunk staging | ui | Rust | zode (crates/git) | P0 |
| F012_GitCommit | Git commit of staged changes | ui | Rust | zode (crates/git) | P0 |
| F014_KeyboardActionDispatch | Keyboard shortcut action dispatch | background | Rust | zode (GPUI core, crates/settings keymap) | P0 |
| F015_MultiBufferSearchResults | Multi-buffer search results view | ui | Rust | zode (crates/multi_buffer) | P1 |
| F016_LspCodeActions | LSP code action application | mixed | Rust | zode (crates/project) | P1 |
| F017_EditorThemeCustomization | Editor theme customization | mixed | Rust | zode (crates/theme) | P1 |
| F023_GitHostingPermalink | Git hosting permalink resolution | mixed | Rust | zode (crates/git_hosting_providers) | P2 |

## Feature Details

### F001_ProjectFolderNavigation: Project folder navigation

**Type**: mixed
**Description**: Developer opens a project folder as a `Worktree` and browses its file tree in the Project Panel to locate and open files for editing. Input: folder pick / directory expand. Process: `Worktree` registration + incremental filesystem watch. Output: rendered file tree, opened `Buffer` on file selection.

**Workspace**: zode (crates/workspace, crates/worktree, crates/project_panel)
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
- BL003_WorkspaceEventEmitterSubscribe: `Workspace` emits worktree-added/entry-tree-updated events consumed by panel views
- BL007_StandaloneCliBinaries: `zed <path>` CLI entry point (`crates/cli`) is one way a project folder is opened

**Related Permissions**:
- N/A — opening a local project folder is unrestricted; no PERM### gate applies

---

### F002_BufferTextEditing: Buffer text editing

**Type**: mixed
**Description**: Developer edits an open file's text; keystrokes mutate the buffer's `Rope` when the buffer's `Capability` is `ReadWrite`, and are silently discarded when it is `Read`/`ReadOnly`. Input: keystrokes. Process: capability check → rope mutation → LSP `didChange` notify. Output: re-rendered `Editor` view, undo-stack entry.

**Workspace**: zode (crates/text, crates/editor, crates/language)
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
- BL004_LspRequestDispatch: debounced buffer edits forwarded to the language server as `textDocument/didChange`

**Related Permissions**:
- PERM004_BufferCapabilityGate: `Buffer`/`MultiBuffer` capability (`ReadWrite`/`Read`/`ReadOnly`) gates whether an edit is accepted

---

### F015_MultiBufferSearchResults: Multi-buffer search results view

**Type**: ui
**Description**: Developer opens project-wide search results as a single scrollable `MultiBuffer` to review/edit matches across files without opening each individually. Input: project search query. Process: aggregate matching excerpts across `TextBuffer`s. Output: one scrollable multi-buffer view; edits write back to originating files.

**Workspace**: zode (crates/multi_buffer)
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

**Workspace**: zode (crates/project — LspStore)
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
- BL004_LspRequestDispatch: `LspStore::handle_lsp_*` handlers dispatch and route LSP protocol messages

**Related Permissions**:
- N/A

---

### F004_LspCompletions: LSP completion suggestions

**Type**: mixed
**Description**: Developer receives completion suggestions from the attached language server while typing. Input: cursor position + buffer context. Process: `textDocument/completion` request off-thread via `LspStore`. Output: completion popover at cursor.

**Workspace**: zode (crates/project — LspStore)
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
- BL004_LspRequestDispatch: `LspStore::handle_lsp_*` handlers dispatch and route LSP protocol messages

**Related Permissions**:
- N/A

---

### F016_LspCodeActions: LSP code action application

**Type**: mixed
**Description**: Developer applies a language-server-suggested code action to fix an issue or refactor. Input: code-action menu trigger. Process: `textDocument/codeAction` request → selected `WorkspaceEdit` applied atomically. Output: buffer(s) updated as one undo-stack entry.

**Workspace**: zode (crates/project)
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
- BL004_LspRequestDispatch: `handle_apply_code_action` routes the accepted action back through the LSP dispatch table

**Related Permissions**:
- N/A

---

### F017_EditorThemeCustomization: Editor theme customization

**Type**: mixed
**Description**: Developer selects a different color theme; every `Render`-implementing surface re-renders immediately with the new palette and the choice persists across restarts. Input: theme-picker selection. Process: swap active `Theme`/`ThemeFamily` → persist to settings → notify observers. Output: re-rendered UI in the new theme.

**Workspace**: zode (crates/theme)
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
- BL008_SettingsStoreObserver: theme selection persisted to settings JSON, re-parsed, and observers re-render

**Related Permissions**:
- N/A

---

### F005_LiveSettingsEditing: Live settings.json editing

**Type**: mixed
**Description**: Developer edits `settings.json` and every dependent registrant applies the new value live, without a restart. Input: settings-buffer save. Process: `SettingsStore` re-parses JSON → notifies each `impl Settings for` registrant. Output: live behavior change across the app; malformed JSON keeps last-valid settings.

**Workspace**: zode (crates/settings)
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
- BL008_SettingsStoreObserver: `SettingsStore` re-parses on change and notifies registrant call sites

**Related Permissions**:
- N/A

---

### F006_ExtensionInstallation: Third-party extension installation

**Type**: mixed
**Description**: Developer installs a third-party WASM extension; its manifest capabilities become its sandboxed allowlist before any code runs, and undeclared host-capability calls are rejected. Input: "Install" action. Process: download + compile `wasm32-wasip2` module → load into `extension_host` with manifest allowlist. Output: extension appears installed and usable; denied calls fail without crashing the host.

**Workspace**: zode (crates/extension_host, crates/extension_api)
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
- BL002_ExtensionHostWasmDispatch: sandboxed WASM calls dispatched in the background, checked against the manifest allowlist, and routed back to the main thread

**Related Permissions**:
- PERM001_ExtensionProcessExecCapability: gates `ProcessExec` capability grants against the manifest's declared allowlist
- PERM002_ExtensionDownloadFileCapability: gates `DownloadFile` capability grants
- PERM003_ExtensionNpmInstallCapability: gates `NpmInstallPackage` capability grants

---

### F009_IntegratedTerminal: Integrated terminal command execution

**Type**: ui
**Description**: Developer runs a shell command in an integrated terminal scoped to the project's working directory. Input: terminal command input. Process: spawn shell process, stream stdout/stderr off the UI thread. Output: live command output in the terminal pane; process terminates cleanly on panel close.

**Workspace**: zode (crates/terminal, crates/terminal_view)
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

**Workspace**: zode (crates/git)
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
**Description**: Developer stages a specific git hunk to commit changes incrementally, leaving the rest of the file's changes unstaged. Input: "Stage Hunk" inline action. Process: apply the hunk-level change to the git index. Output: git index reflects only the staged hunk.

**Workspace**: zode (crates/git)
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
- N/A — the collaboration-era hard-deny on this operation for non-owning participants no longer applies; there are no other participants in this fork

---

### F012_GitCommit: Git commit of staged changes

**Type**: ui
**Description**: Developer commits staged changes with a message, recording a checkpoint in the project's git history. Input: commit message + "Commit" click. Process: run commit against the active `Repository`'s staged index; reject empty messages before invoking git. Output: new commit created, staged list clears, HEAD updates.

**Workspace**: zode (crates/git)
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

**Workspace**: zode (crates/git_hosting_providers)
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
- BL006_GitHostingProviderDetection: provider-detection module resolving the remote to a hosting provider

**Related Permissions**:
- N/A

---

### F014_KeyboardActionDispatch: Keyboard shortcut action dispatch

**Type**: background
**Description**: A keystroke is matched against the focused element's registered `actions!()` bindings and keymap context, then dispatched to the nearest handler up the focus/view hierarchy; unbound keystrokes pass through without triggering anything. Input: keystroke. Process: keymap-context match → action dispatch to `.on_action()` handler. Output: matched action executes (e.g. Save writes the buffer); unbound keystrokes are no-ops.

**Workspace**: zode (GPUI core, crates/settings keymap config)
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
- BL003_WorkspaceEventEmitterSubscribe: related entity-event plumbing that action handlers often trigger downstream

**Related Permissions**:
- N/A

---

## Summary

- **Total Features**: 15 (down from 23 — F007, F008, F013, F018, F019, F020, F021, F022 removed)
- **Total User Stories**: 19 (US001–US009, US014–US016, US021–US027; US010–US013 and US017–US020 removed along with their parent features)
- **Total Routes**: 0 (native desktop app; no HTTP routes in this codebase per system-overview.md)
- **Total Data Models**: 14 entities (per entities.md, down from 16 — Thread and Message removed)
- **Total Background Logic**: 8 distinct BL### codes referenced (BL001–BL008, renumbered — down from 13)
- **Total Permissions**: 4 formally coded (PERM001–PERM004, down from 8), cited under Related Permissions on F002 and F006; remaining features genuinely have no PERM### gate
- **Languages Detected**: Rust (core), WASM (guest extensions via wasm32-wasip2)

## Cross-Reference Validation

- [x] All F### codes are unique (F001–F006, F009–F012, F014–F017, F023 — non-contiguous, reflecting the 8 removed codes; codes are not renumbered so history stays traceable)
- [x] All F### codes are referenced in user-stories.md by their constituent US###
- [x] No SCR### codes appear (screen_source:none — descriptive UI-surface names used instead)
- [x] All user story references are valid (19 US### codes, each appears in exactly one F###)
- [x] All route references are N/A with reason (no HTTP routes in this codebase)
- [x] All data model references verified against entities.md's 14 entities
- [x] All background logic references are valid (BL001–BL008, matching the renumbered behavior-logic.md)
- [x] All permission references are valid PERM### codes (PERM001–PERM004, verified against permissions-matrix.md)
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
| US014_RunTerminalCommand | F009_IntegratedTerminal |
| US015_ViewGitDiff | F010_GitDiffViewing |
| US016_CommitStagedChanges | F012_GitCommit |
| US021_ReadOnlyBufferRejectsEdit | F002_BufferTextEditing |
| US022_ExtensionCapabilityDenied | F006_ExtensionInstallation |
| US023_SettingsChangeNotifiesObservers | F005_LiveSettingsEditing |
| US024_DispatchKeyboardAction | F014_KeyboardActionDispatch |
| US025_ResolveGitHostingPermalink | F023_GitHostingPermalink |
| US026_ReceiveLspCompletions | F004_LspCompletions |
| US027_StageGitHunk | F011_GitHunkStaging |

## Unresolved Questions

1. Data-model entities are reconciled against `entities.md` (14 entities); that artifact assigns no `MODEL###` codes (heading-identified only), so entity names are cited directly rather than via a code.
2. Permission references are reconciled against `permissions-matrix.md`'s PERM001–PERM004 registry — resolved, no longer open.
3. F001_ProjectFolderNavigation merges US001 (open folder) and US002 (browse tree) as one feature since they form a single continuous input→process→output flow (pick folder → watch/populate tree → open file); flagging in case a stricter single-US-per-feature policy is wanted instead.
