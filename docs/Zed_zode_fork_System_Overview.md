# Zed_zode_fork — System Overview

| | |
|---|---|
| **Client** | Internal / Sun\* engineering team |
| **System** | zode (a fork of the Zed code editor) |
| **Version** | Documentation snapshot as of the source commit reflected in `docs/` |
| **Date** | 2026-08-07 |
| **Prepared by** | `/tkm:rebuild-spec --overview` (automated synthesis from the promoted `docs/` spec set) |

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Purpose & Business Value](#2-purpose--business-value)
3. [Actors & Roles](#3-actors--roles)
4. [Business Flows & Lifecycle](#4-business-flows--lifecycle)
5. [Detailed Function List](#5-detailed-function-list)
6. [Screen List](#6-screen-list)
7. [External Integrations](#7-external-integrations)
8. [Configuration & Optional Features](#8-configuration--optional-features)
9. [Technical Architecture & Infrastructure](#9-technical-architecture--infrastructure)
10. [Data Model (Summary)](#10-data-model-summary)
11. [Open Questions & Known Issues](#11-open-questions--known-issues)

---

## 1. System Overview

Zode is a native, high-performance code editor that runs directly on the user's computer — Mac,
Linux, or Windows — rather than in a browser or through a hosted service. It began as a fork of
an existing editor, and the team behind it made a clear, deliberate choice about what kind of
product it should be: a fast, focused tool for one developer working on their own machine, rather
than a platform for many people to edit code together in real time.

To achieve this focus, the real-time multiplayer/collaboration features and the built-in AI
writing-assistant features that the original editor offered have been removed. In their place,
the product keeps one capability for working beyond a single machine: connecting over SSH to a
project that lives on a different computer (for example, a personal remote server) and editing it
as though it were local. There is no shared, vendor-operated backend service that all users
connect through — each installation is self-contained.

Recent work on this product has focused on making it comfortable to work across several projects
in one window at once: an always-visible strip for quickly switching between open projects, a
refreshed window title bar with a built-in search bar, and an automatic system that quietly
"hibernates" projects the user isn't currently looking at, so having several projects open doesn't
cost more in memory and battery than actually needs to.

## 2. Purpose & Business Value

Zode is a native desktop application for writing and editing code — a lightweight, single-developer tool built for speed and focus rather than for the kind of shared, always-connected workspace that some code editors try to be. It runs directly on the user's machine (Mac, Linux, or Windows) with the responsiveness of a native application rather than a browser-based tool, and it always starts from what's on the user's own machine and files.

**A deliberate choice: one developer, one machine, no shared backend.** Zode began as a fork of an editor called Zed, and it makes an explicit product decision: it removes the built-in real-time multiplayer/collaboration features (multiple people editing together live) and the built-in AI writing-assistant subsystem that the original product offered. In their place, the product keeps a single capability for working beyond one's own machine: the ability to connect over SSH to a project that lives on a different computer (for example, a remote server) and edit it as if it were local. This is a deliberate trade — rather than running a shared collaboration service that many developers connect to at once, zode is built to be a fast, private, single-user tool that can still reach out to remote environments a user personally controls, without depending on any always-on backend service or third-party AI service.

**The value this delivers:**
- **Performance and responsiveness** — because the tool is a native application built specifically for text editing, it can render and respond to keystrokes with very low latency, even on large files and projects.
- **Focus and simplicity** — by not carrying multiplayer or AI-agent complexity, the product surface stays centered on the core job: opening a project, editing code, running tools against it (like language checking, debugging, and version control), and moving between projects efficiently.
- **Extensibility without added risk** — the editor can be extended with community or in-house add-ons (for new languages, themes, or tooling), and these run in a sandboxed, isolated environment so a faulty or misbehaving add-on cannot crash or compromise the main application.
- **Built-in version control and debugging** — everyday developer workflows (viewing changes, staging and committing work, stepping through a debugger) are handled natively inside the editor rather than requiring separate tools.
- **Safe handling of untrusted work** — when a user opens a folder they haven't worked with before, zode does not automatically run tooling against it; it asks for explicit trust first, protecting the user from accidentally running code-adjacent tools against something unfamiliar or unsafe.
- **Efficient multi-project work** — a user can keep several projects open at once without needing to worry about the ones they aren't currently using consuming unnecessary computer resources (more on this in the Business Flows & Lifecycle section below).

## 3. Actors & Roles

Zode has **no user-role or account system** — it is used by a single developer on their own
machine, with no login, no multi-tenant accounts, and no collaboration server to grant roles
against. What might look like "permissions" in other products are, here, three unrelated,
independent gates rather than a role hierarchy:

| Gate | What it controls |
|---|---|
| **Extension trust** | What a third-party add-on is allowed to do on the user's machine (e.g. run an external program, download a file, install a package) — declared in advance, never granted implicitly. |
| **File edit capability** | Whether an open file can be edited, is temporarily view-only, or structurally cannot be edited at all. |
| **Project/folder trust** | Whether a folder the user has opened is trusted enough to run tooling (like language servers) against it — new folders are untrusted by default until the user explicitly approves them. |

A smaller, separate mechanism — an internal "staff" feature-flag system — controls which
in-progress features the internal engineering team can see ahead of a wider release. This is a
rollout mechanism, not a customer-facing permission.

## 4. Business Flows & Lifecycle

Only one business-level flow currently exists in the documented system: the lifecycle of a project that goes idle and later comes back into use. (This is a desktop application, not a web application, so it has no "screens" connected by navigation routes in the way a website would — there is no screen-flow document for this reason, and none should be expected. No other end-to-end business flows have been documented at this time; this section should not be padded to imply otherwise.)

**Idle projects quietly scale themselves down, then spring back to life.**

When a user works across multiple open projects at once — for example, switching between two or three codebases in the same window — only the project the user is actually looking at needs to stay fully active. Zode recognizes when a project has been sitting in the background, unused for a while, and automatically scales back the work it's doing on the user's behalf: it pauses the background process that watches files for changes, and it trims the memory used by that project's terminal history to something more efficient (while remembering the original size so nothing is lost, just condensed). This happens automatically, without the user needing to close anything or manage resources themselves.

Two safeguards keep this automatic scaling-down from ever getting in the way of real work:
- If a project has an active debugging session running, or has a file mid-autosave, zode holds off on scaling it down and tries again shortly after — it will not interrupt or discard work in progress.
- The visual indicators the user relies on (like file change indicators in the sidebar) aren't simply removed when a project goes idle — they're shown in a dimmed state so the user can still see that information existed, even while it's temporarily unverified.

**Coming back is seamless.** The moment a user clicks back into a project that had scaled itself down, zode immediately restores it: background file-watching resumes (prioritizing the files the user actually has open), the terminal's history returns to its normal size, and the project's version-control status is refreshed so everything the user sees is current again. From the user's point of view, returning to a project they stepped away from should feel instant and complete — nothing needs to be manually reactivated.

This lifecycle is a supporting efficiency feature — it exists so that keeping several projects open costs the user as little in system resources as possible — rather than a feature the user interacts with directly.

## 5. Detailed Function List

*Note on structure: zode is a single-user native desktop application with no accounts, roles, or collaboration/multiplayer backend — the "who can do what" role split used for multi-tenant products doesn't apply here. Functions below are grouped instead by natural usage category: Everyday Editing, Code Intelligence & Quality, Project & Workspace Management, Version Control, Extensibility, and Behind the Scenes.*

### Everyday Editing

| Function | Why It Matters | What the User Does |
|---|---|---|
| Editor Core | The text-editing surface every other panel (search results, diagnostics, diffs) reuses to display text — the single most-used part of the app. | Navigate by word/line/page, select and extend selections to matching text, delete to a line boundary, compare two file versions in a combined or side-by-side view, switch text encoding or line-ending style. Cursor position and selections are quietly remembered between sessions. |
| Vim Emulation | Lets developers who already know Vim's keyboard-driven editing keep their muscle memory instead of relearning shortcuts, on the same shared codebase as everyone else. | Turn on Vim mode; navigate, select, and edit entirely via keyboard commands; run short command-line instructions (e.g. find-and-replace); repeat or record/replay edit sequences; target whole words, phrases, or bracketed text with one command. The current mode is always shown at the bottom of the window. |
| Search | Three purpose-built ways to find things fast — a file by name, a term across the whole project, or the next match in the open file — instead of one tool trying to do everything. | Type a partial filename to jump to it (or create it if it doesn't exist); search a term across the entire project and edit results directly in place; search and replace within the currently open file. |

### Code Intelligence & Quality

| Function | Why It Matters | What the User Does |
|---|---|---|
| Language Intelligence | The plumbing behind every "smart" editing feature — error squiggles, autocomplete, jump-to-definition, auto-formatting — by starting and managing the background language-analysis helpers. Without it the editor is just a plain text box. | Opening a file auto-starts the right language helper; pick a specific toolchain/interpreter version when multiple are installed; manually correct a misdetected file's language; restart a stuck helper without closing the editor; formatting-on-save runs through the same mechanism. |
| Diagnostics | Turns errors/warnings that language helpers compute into two focused views (current file vs. whole project) instead of hunting through logs, and stays honest about stale data while a background project is waking up. | View diagnostics for the current file or the whole project; filter out warning-level noise; see a status-bar alert if a language helper has crashed; copy system specs for a bug report. |
| Debugging | Lets a developer run their program under a debugger, pause it, and inspect or change its state to find bugs faster than reading logs alone. | Start/stop/step through a debug session; set and clear breakpoints; watch expressions and inspect or edit variables while paused; attach to an already-running remote process; view raw debug-protocol logs when a debugger integration misbehaves. |

### Project & Workspace Management

| Function | Why It Matters | What the User Does |
|---|---|---|
| Workspace & Project Management | Lets a developer keep several projects open in one window at once (this fork's signature capability) while automatically hibernating the ones not in use, saving memory and battery. Also covers everyday navigation and reproducible team setups. | Reopen a recent project from the welcome screen; switch between open projects via an always-visible project rail; browse and edit the file tree; approve a new folder before any tooling runs against it (security gate); scaffold a shared containerized dev environment for a team. |
| Settings & Keymaps | Lets developers customize the editor (fonts, keybindings, themes) with changes taking effect instantly, and protects their configuration from being silently broken when the settings format changes during an update. | Edit settings and see changes apply immediately with no restart; record new keybindings; switch to a familiar keymap preset from another editor; accept a backup-then-migrate prompt when the settings format changes. |

### Version Control

| Function | Why It Matters | What the User Does |
|---|---|---|
| Git Integration | Brings git operations directly into the editor instead of forcing a switch to a separate terminal or client, so a developer can stage, review, and commit without losing their place in the code. | View changed files, stage or unstage a whole file or just part of it, stash work-in-progress, discard unwanted edits, write and confirm commits, review every changed file together, switch branches, and view a visual commit history graph. |

### Extensibility

| Function | Why It Matters | What the User Does |
|---|---|---|
| Extension System | Lets the community add languages, tools, and integrations without the core team building everything in — while making sure an extension can never do more than it explicitly declared. | Install extensions from the Extensions page or accept a suggestion banner for an unsupported file type; author and iteratively rebuild a local extension; declare exactly which external commands/downloads/installs an extension is allowed to use; restart a stalled external tool connection. |

### Behind the Scenes

| Function | Why It Matters | What the User Does |
|---|---|---|
| Terminal | Keeps command-line work (builds, tests, git) inside the editor window alongside the code it operates on, instead of breaking focus by switching to a separate app. | Open an integrated terminal (starts in the project's own folder); show/hide the panel without interrupting a running command; run a project-configured task and see a pass/fail summary; search terminal scrollback for earlier output. |

## 6. Screen List

A formal screen or route inventory was not generated for zode — it is a native desktop application built on GPUI's panel/view model, not a routed web application, so there is no URL-addressable "screen list" to enumerate in the usual sense. What follows is a brief, honest description of the main areas of the interface, inferred from each feature's own screen documentation rather than presented as a formal Screen/Route/Type table.

- **Editor pane** — the core text-editing surface, including split-diff comparison views and status-bar encoding/line-ending indicators.
- **Terminal panel** — a dockable panel hosting one or more live terminal tabs, plus an in-terminal search bar.
- **Diagnostics views** — a current-buffer diagnostics tab, an aggregated project-wide diagnostics tab, and status-bar/file-tree diagnostic indicators.
- **Debugger panel** — console, breakpoint list, variable inspector, and memory view, plus a "New Process" modal, an "Attach to Process" modal, and a debug-adapter log viewer.
- **Git panel and related views** — the Git panel itself, a branch picker, commit view, stash picker, a project-wide diff view, a visual commit graph, and a worktree picker.
- **Extensions page** — the installed/available extensions list, a language-extension suggestion banner, and a context-server connection status view.
- **Welcome screen and project navigation** — the welcome screen's recent-projects list, an always-visible project rail, an expandable sidebar panel, the project panel (file tree), a recent-projects picker, a quick tab switcher, a dev-container setup wizard, and a folder-trust security prompt.
- **Settings & keymaps** — a dedicated Settings window, a keymap editor with a keybinding-recording modal, a base-keymap preset picker, a theme picker, and a settings-migration banner.
- **Search surfaces** — the fuzzy file finder, the whole-project search view, and the in-editor buffer search bar.
- **Vim mode indicators** — the normal editor pane with Vim-mode behavior, a mode indicator in the status bar, and an ex-command input line.

The best entry point for a reader new to the interface is the **editor pane** — it is the one surface every other view above ultimately renders text through or opens alongside.

## 7. External Integrations

zode connects to the outside world through a small, deliberate set of integrations — all of them local-first, none of them dependent on a cloud service the vendor operates.

| Integration | What it gives the user |
|---|---|
| **Language servers** | Per-language intelligence — autocomplete, error/warning highlighting, "go to definition," and code actions — supplied by whichever language tooling is installed for the project (e.g. a Rust or TypeScript language service). zode runs its own client to talk to these servers; nothing is sent to a vendor-hosted service. |
| **Debug adapters** | Debugging support (breakpoints, stepping through code, inspecting variables) via the industry-standard Debug Adapter Protocol, kept as a separate layer from the language-server integration above. |
| **Git** | Built-in version control: viewing file changes/diffs, branch and commit history, staging, and commit creation, surfaced directly in the editor's Git panel and title bar. |
| **Extensions** | A sandboxed plugin system that lets third-party add-ons contribute themes, additional language support, and other language servers or debug adapters — each extension runs in an isolated sandbox and can only do what it has explicitly declared it needs to do (see Configuration & Optional Features, below). |
| **Remote (SSH) development** | The ability to open and edit a project that lives on a different machine, connecting over SSH and running the project/language/debugging tooling on that remote host while the familiar editor UI stays local. This is a single user working on their own remote machine — it is not a shared multi-user collaboration session, and zode has no always-on collaboration server. |

There is no web-style API surface in this product — zode is a desktop application, not a service with HTTP endpoints — so integrations are limited to the categories above rather than a catalog of API routes.

## 8. Configuration & Optional Features

zode's configurable behavior is best understood as a small set of trust and capability gates rather than user roles or an admin console — there is no multi-account permission system, since the product is used by one person on their own machine.

| Configurable behavior | What it controls |
|---|---|
| **Extension permissions** | When an extension wants to run an external program, download a file from a network location, or install an npm package, it must have declared exactly that capability in advance. Anything not explicitly declared is blocked automatically — extensions cannot silently gain new abilities at runtime. |
| **Editable vs. read-only content** | Any open file can be in a fully editable state, a temporarily view-only state, or a state that structurally cannot be edited at all (for example, certain remote or preview content). This determines whether typing into a file is accepted or has no effect. |
| **Project/folder trust** | Opening a new folder for the first time treats it as untrusted by default, which holds back potentially risky actions (like starting a language server) until the user explicitly grants trust via a security prompt. Trust can be granted per folder, inherited from a trusted parent folder, or — via an opt-in setting — turned off entirely so all folders are auto-trusted. Once granted, trust is remembered so the same folder isn't re-prompted later. |
| **Staff/experimental feature flags** | Some in-progress features are gated behind an internal "staff" flag so they can be tested before a wider rollout, with an environment override available to test the "not yet available" experience locally. This is an internal rollout mechanism, not an end-user-facing setting. |
| **Project activity state** | Each open project can be actively focused, idle-but-still-live, or hibernated (fully idle, with background resources like language servers and terminals torn down or deferred to save memory) — this happens automatically based on how long a project has sat unfocused, and lets a user keep many projects open in one window without the resource cost of running them all simultaneously. |
| **Editor display modes** | An editing surface can appear as a full-featured editor (with gutter, line numbers, minimap, etc.), a compact single-line input, or an auto-growing multi-line input — used consistently across the main editor and smaller inline inputs like rename/search fields. |

## 9. Technical Architecture & Infrastructure

zode is a **native desktop application** — it installs and runs directly on the user's computer rather than in a browser or as a hosted web service.

- **Language:** Built in **Rust**, chosen for its combination of performance and memory safety, which matters for an application that must stay responsive while indexing large codebases and running many background processes (language servers, git operations, terminals).
- **UI framework:** Powered by **GPUI**, a custom, in-house-built, GPU-accelerated user interface toolkit built specifically for this editor rather than adopting an existing off-the-shelf UI framework. This gives the editor tight control over rendering performance and a consistent interaction model across every part of the application.
- **Platform reach:** Designed to run natively on **macOS, Linux, and Windows**, with an experimental browser-based (WebAssembly) rendering path also in progress.
- **Deployment model:** There is no server-side component that the vendor operates and that all users share — each installation is a self-contained, single-user instance. The one exception is the optional remote-development feature, where a user can connect their own desktop app to their own remote machine over SSH; this is a private, user-controlled connection, not a shared multi-tenant backend.
- **Local storage:** Application and session state (such as open projects and window layout) is stored locally on the user's machine using an embedded SQLite database — no data is stored in a cloud database by default.

This architecture reflects a deliberate "local-first" design principle: the core editing, project management, and language-intelligence experience works entirely offline, with network access reserved for optional features (installing extensions, checking for updates, and remote development).

## 10. Data Model (Summary)

zode's core concepts describe the state of an editing session rather than records in a business database, since it is a desktop tool and not a data-driven web application. The main business nouns are:

**Workspace & window organization**
- A **Workspace** represents one open project inside one window slot — it holds the panels, tabs, and layout the user sees, along with the single project it's currently showing.
- A window can hold more than one **Workspace** at once (this fork's "multi-project" capability); the container that manages which one is actively shown, and which others are kept running quietly in the background, decides when an inactive project is put to sleep ("hibernated") to save resources.
- A **Pane** is a tab strip within a workspace — the container that holds a stack of open tabs (an editor, a terminal, a diagnostics list, etc.), letting a user split their screen and organize multiple things side by side.

**Projects and files**
- A **Project** is the central concept tying together everything related to one codebase the user has open — its files, its git status, its running language servers and debuggers, its terminals, and its tasks.
- A **Worktree** represents one folder/directory tree that has been opened as part of a project; a project can span more than one worktree at once.
- An **Entry** is a single file or folder inside a worktree — tracking things like whether it's hidden, ignored by git, or a symlink.

**Editing content**
- A **Buffer** is the in-memory representation of one open file's content, including its text, detected programming language, and any errors/warnings reported against it.
- Buffers can be combined into a single editable view — for example, to show search results or a diff spanning multiple files — which is how the editor supports things like multi-file search-and-replace.
- An **Editor** is the visual component the user actually types into and sees — cursors, selections, scrolling, and suggestions — always built on top of one of the buffer views above.

**Supporting concepts**
- **Settings** describes the layered configuration system — defaults, user preferences, and project-specific overrides — that governs how the editor behaves and looks.
- A **Theme** is a named color and typography scheme; themes are grouped into families of related visual styles (e.g. light and dark variants of the same design).
- A **Repository** represents one Git repository detected within a project, including its branches, commit history, and file-level change status.
- The **Project Panel** is the file-tree sidebar that lets a user browse, rename, and organize the files and folders inside their open project.
- A **Terminal** is one embedded command-line session running inside a project, which — like projects themselves — can have its history trimmed and later restored when a project goes idle and wakes back up.
- A **Language Server** represents one running language-intelligence process (for a specific programming language) that a project is currently talking to.
- An **Extension** is a packaged, sandboxed add-on that can contribute themes, language support, or other integrations to the editor, along with a declared list of exactly what external actions it's allowed to take.

## 11. Open Questions & Known Issues

The engineering review that produced this document surfaced a number of open items — places where behavior is not fully confirmed, or where a product decision is still pending. None of these are known to be broken; they are flagged so the team can decide whether they need attention before or shortly after launch.

### Terminal
- It's unclear which part of the application actually uses the terminal's "display-only" mode (likely for showing output piped from an integrated AI/agent tool) — no confirmed usage was found during review.
- Exactly how and where terminal session data is saved to the local database wasn't fully traced.

### Language Intelligence (code completion, error checking)
- What happens when Vue file support can't find its companion TypeScript service isn't fully verified — the assumed behavior is inferred, not directly observed.
- Whether notebook/Jupyter-style code execution support should live under "Language Intelligence" or deserves its own separate feature area is an open categorization question.
- A per-user setting for programming-language toolchains exists in the database, but exactly when it gets written (versus a similar, more general table) wasn't fully traced.

### Editor Core
- The exact set of internal states background code-parsing can be in wasn't independently re-verified in this review.
- A minor inconsistency was found between two internal reference documents over how many distinct "editing modes" the editor supports — flagged for a documentation fix, not a functional bug.
- How the initial list of autocomplete suggestions gets assembled, before it's narrowed down as the user types, wasn't traced.
- The editor's architecture document wasn't reviewed in full during this pass, so any structural design details it covers aren't reflected here.

### Diagnostics (errors and warnings)
- No hard guarantee was confirmed that outdated error/warning markers always clear themselves after a project is re-scanned — a language service that fails to restart could theoretically leave stale markers on screen indefinitely.
- The "include warnings" toggle applies globally across the whole application rather than per project or per window — worth confirming with the team whether that still makes sense now that multiple projects can be open side by side.
- It's unconfirmed whether some severity levels (for example, muting low-priority "info" or "hint" messages) can be changed from anywhere in the interface besides manually editing a settings file.

### Debugging
- It's unclear whether the "watch expression" feature checks that the connected debugger actually supports it before trying, or simply attempts it every time and relies on a graceful failure.
- Whether attaching the debugger to a remote process is protected by any permission check beyond the underlying secure connection wasn't verified.
- How reliably breakpoint-on-exception settings are saved and restored between sessions wasn't fully traced — the code documents this as "best effort," implying it may not always persist.

### Git Integration
- When resolving a stash conflict (a common Git operation) produces an error, it's unclear whether the user sees a specific, helpful explanation or just a generic error message — this needs a hands-on check.
- Push/pull/fetch operations (syncing with a remote repository) exist in the underlying code but aren't tied to any documented user-facing requirement for this feature area — open question whether that's an intentional scope boundary or something that was missed when features were catalogued.

### Extension System
- There's no visible in-app screen for a user to grant or revoke an installed extension's permissions after the fact (unlike the comparable "trust this folder" prompt used elsewhere) — unclear whether this is intentional or a genuine gap.
- The specific button or menu that triggers a "restart" action for a connected extension service wasn't located during this review.
- A list of "suppressed extensions" is referenced in the code, but its purpose and how it relates to permission checks wasn't investigated.

### Workspace and Project Management (multi-project support, hibernation)
- The exact default time thresholds before an inactive project gets automatically "hibernated" (paused to save memory and system resources) weren't confirmed — only that such a setting exists.
- Two unrelated concepts in the underlying code both happen to use the word "worktree" for different things — unclear whether that's a deliberate naming choice or simply an accident of how the feature evolved over time.
- Currently there's only one global on/off setting controlling whether background projects stay loaded; whether a future version is meant to offer this as a per-project setting is unconfirmed.

### Vim Emulation (modal, keyboard-driven editing)
- It's not confirmed whether the alternative "Helix" editing mode fully matches the real Helix editor's behavior, or whether it's a partial reproduction.
- It's unclear what happens if a user starts recording a macro into a slot that already holds a saved one — whether the old one is overwritten or the new recording is appended to it.
- If a bookmark/mark fails to save to the database, the failure is logged internally but never shown to the user — unclear whether that's a deliberate "fail quietly" design choice or an unaddressed gap.

### Settings and Keymaps
- One internal function tied to reloading the keyboard-shortcuts file was confirmed to exist, but its exact behavior wasn't independently re-checked in this pass.
- It's unclear whether custom themes installed through extensions reload automatically when their file changes, the same way the application's built-in themes do.

### Search
- It's unclear whether the cap on fuzzy file-search results (fixed at 100 matches) can be changed through settings, or whether it's a hard limit.
- When a project-wide search hits its results cap, the interface shows a "+" after the result count, but it's unclear whether anything else (such as a banner or tooltip) explains to the user why results stopped accumulating.
- Command-usage history clearly influences how the command palette ranks suggestions over time, but the exact weighting formula behind that ranking wasn't traced.

### Broader Product Questions (unconfirmed — inferred, not verified)

The following come from an experimental, lower-confidence internal document (`design-intent.md`) that reconstructs *why* certain choices were made, rather than *what* was built. Unlike the items above, these are inferred from indirect evidence (commit messages and code comments) rather than confirmed facts, and should be treated as discussion points rather than settled findings:

- No formal record of past architecture decisions exists for this project; the reasoning behind earlier design choices is pieced together from commit messages and code comments rather than a stable decision log. Future changes may not carry the same level of documented rationale.
- The business reason for removing collaboration, account/login, AI, and cloud-sync features from the original editor (to create this streamlined, standalone version) is not recorded anywhere found in the project — if there's a licensing, positioning, or maintenance rationale, it isn't written down.
- Why the extension system was built on a sandboxed execution technology (WASM) specifically, rather than an alternative isolation approach, isn't documented.
- Why the local database layer uses a custom-built component rather than an established, off-the-shelf one isn't documented.
