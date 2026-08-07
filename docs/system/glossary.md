# Glossary

## Color Theme
**Definition:** The set of colors and fonts that decide how the whole app looks — one "family" of related themes (e.g. a light and dark pair) groups several named variants.
**Technical alias:** Theme / ThemeFamily
**Used in:** F012, F015

## Combined Text View
**Definition:** A single scrollable view that stitches together snippets from one or more files — used for whole-project search results, diagnostics lists, and diff comparisons, so those results can be read and edited without opening each file separately.
**Technical alias:** MultiBuffer
**Used in:** F008, F009, F016

## Diagnostics
**Definition:** The errors and warnings a language helper finds in the code, shown either for just the file currently open or for the whole project at once.
**Technical alias:** Diagnostics (LanguageServerId-keyed DiagnosticSet on Buffer)
**Used in:** F008, F009

## Entity Handle
**Definition:** An internal bookkeeping mechanism the app uses to track pieces of its own state (like an open project or an open file) so they stay alive exactly as long as something still needs them, and are cleaned up automatically once nothing does. Not something a developer using the editor ever sees directly.
**Technical alias:** Entity<T> / WeakEntity<T>
**Used in:** (no feature names it directly — it is the storage mechanism underlying every other entity in this glossary)

## Extension
**Definition:** An add-on package that gives the editor a new language, tool, theme, or integration that isn't built in by default. Extensions must declare up front exactly what external commands, downloads, or installs they need, so they can never quietly do more than they said.
**Technical alias:** ExtensionManifest
**Used in:** F002, F012, F015

## File Tree Panel
**Definition:** The sidebar that shows a project's files and folders as a browsable tree, where a developer can select, rename, and create files.
**Technical alias:** ProjectPanel
**Used in:** F013

## Folder Trust
**Definition:** A one-time permission check the editor asks for before it runs any language tooling or certain git operations against a folder it hasn't seen before, so opening an unfamiliar folder can't silently run code on the developer's machine.
**Technical alias:** worktree trust
**Used in:** F002, F012, F013

## Git Repository
**Definition:** A project's version-control history and current change status (what's modified, which branch is checked out, any in-progress merge) as tracked by the editor's git panel.
**Technical alias:** GitStore / Repository
**Used in:** F011, F013

## Hibernation
**Definition:** The editor's way of quietly winding down a project's background work (language help, file watching, format-on-save) when it's been sitting unused in the background for a while, to save memory — and waking it back up the instant the developer switches back to it.
**Technical alias:** Project.activity (Active / Warm / Hibernated)
**Used in:** F009, F013

## Language Server
**Definition:** A background helper program that understands a particular programming language and powers features like error checking, autocomplete, and jump-to-definition; the editor starts one automatically for each language it detects in an open file.
**Technical alias:** LanguageServer
**Used in:** F002, F009

## Open File
**Definition:** A file currently loaded into the editor's memory, including its text, detected language, and any known errors — this is what a developer is actually looking at and typing into, as opposed to the unopened copy sitting on disk.
**Technical alias:** Buffer (crates/language)
**Used in:** F002, F008, F009

## Pane
**Definition:** A tab strip that can hold several open files, terminals, or panels side by side, and can be split into more panes so a developer can view several things at once.
**Technical alias:** Pane / Item
**Used in:** F001, F008, F013

## Project
**Definition:** One codebase folder (or set of linked folders) a developer has opened — the central hub that ties together its files, git status, terminals, and language helpers.
**Technical alias:** Project
**Used in:** F001, F002, F008, F009, F010, F011, F012, F013, F014, F015, F016

## Project Entry
**Definition:** A single file or folder listed inside a project's file tree.
**Technical alias:** Entry
**Used in:** F013

## Project Folder
**Definition:** One filesystem folder a developer has added to a project — the editor keeps a live, continuously-updated index of every file and subfolder inside it.
**Technical alias:** Worktree
**Used in:** F002, F011, F013

## Project Window
**Definition:** This fork's window-level container that lets several projects stay open at once in a single window, showing one at a time while automatically hibernating the rest in the background.
**Technical alias:** MultiWorkspace
**Used in:** F013

## Settings
**Definition:** A developer's saved preferences — font size, key bindings, color theme, and dozens of other options — which take effect instantly across every open file the moment they're changed.
**Technical alias:** SettingsStore
**Used in:** F015

## Terminal
**Definition:** A built-in command-line session running inside the editor window, so a developer can run builds, tests, or git commands without switching to a separate app.
**Technical alias:** Terminal
**Used in:** F001

## Text Editor
**Definition:** The actual text-editing surface where a developer moves the cursor, selects, and types — reused underneath nearly every panel in the app (search results, diagnostics, diffs) rather than each panel building its own.
**Technical alias:** Editor
**Used in:** F008, F014, F016

## Text Storage
**Definition:** The lowest-level layer that holds a file's raw text and its full undo/redo history, with no awareness of programming languages — an internal layer sitting underneath every Open File.
**Technical alias:** TextBuffer (crates/text)
**Used in:** (no feature names it directly — it is the internal layer beneath Open File)

## Workspace
**Definition:** One open project's window-content — the panes, side panels, and status bar a developer sees for that project. A single window can hold several of these when multiple projects are open at once.
**Technical alias:** Workspace
**Used in:** F013
