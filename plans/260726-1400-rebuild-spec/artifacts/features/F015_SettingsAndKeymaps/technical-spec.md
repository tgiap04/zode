# F015_SettingsAndKeymaps: Technical Spec
**Priority**: P0
**Type**: mixed
**Generated**: 2026-08-07

## Overview

This feature covers live editing of `settings.json` and `keymap.json` through their dedicated
GPUI editors, switching the base keymap preset, theme selection/appearance, and the
backup-then-migrate flow that protects a user's config across schema changes on update. It also
owns the staff feature-flag gate (`FeatureFlagStore`) — an app-wide configuration concern placed
here for lack of a better home. It spans `crates/settings` (the `SettingsStore` registry and
file-watch plumbing), `crates/settings_ui` (the settings editor window), `crates/keymap_editor`
(the keybinding editor and keystroke recorder), `crates/theme`/`crates/theme_selector` (theme
data model and picker), `crates/onboarding` (base-keymap picker), `crates/feature_flags`, and
`crates/zed/src/zed/migrate.rs` (the migration banner/writer).

## Polymorphic Behavior

### DISC-010 — SettingsFile (precedence enum)

| Value | Render | Validation | Persistence |
|-------|--------|------------|-------------|
| `Default` | Baseline shipped defaults shown as the lowest-precedence layer in Settings UI | Never user-editable directly | Bundled with the binary, not written by this feature |
| `Global` | Org/device-wide overrides layer | Parsed same as user settings; errors surfaced per-file in `file_errors` | Written to `global_settings.json` via `set_global_settings` |
| `User` | The file the Settings UI edits by default (`SettingsUiFile::User`) | Parsed via `SettingsStore::set_user_settings`; parse errors recorded, not fatal | Written to `settings.json` via `update_settings_file`/`update_settings_file_with_completion` |
| `Server` | Remote-dev server-pushed overrides | Not user-editable from this client | Not written by this feature (received, not authored, locally) |
| `Project((WorktreeId, RelPath))` | Per-directory `.zed/settings.json` layer, edited when `SettingsUiFile::Project` is selected in the Settings UI | Queued through `ProjectSettingsUpdateQueue` per worktree | Written to the project's `.zed/settings.json` via `update_project_setting_file` |

**Source:** `crates/settings/src/settings_store.rs:145` (struct), discriminator `Ord`/`SettingsFile`
enum referenced in `permissions-matrix.md`/`data-model.md` DISC-010; write paths verified at
`crates/settings_ui/src/settings_ui.rs:3862-3887`.

### DISC-011 — Theme.appearance

| Value | Render | Validation | Persistence |
|-------|--------|------------|-------------|
| `Light` | Theme selector and system tie this appearance to light-mode contrast defaults; auto-selected when the OS reports a light appearance and the user has an appearance-tracking theme setting | No extra validation beyond enum parse | No DB write — theme selection is stored as a name string in `settings.json` (`theme` key), not this enum directly |
| `Dark` | Same mechanism for dark-mode contrast defaults | No extra validation beyond enum parse | Same — name string in `settings.json` |

**Source:** `crates/theme/src/theme.rs:54-59` (`Appearance` enum).

## Cross-Cutting Logic

### Requirements

| Code | Description | Endpoint/Handler | Verifiable |
|------|-------------|------------------|------------|
| FR-001 | Apply a live settings change to the in-memory `SettingsStore` and re-render every registered `impl Settings` consumer | `SettingsStore::set_user_settings`/`watch_settings_files` | yes |
| FR-002 | Watch the user/global settings files on disk and hot-reload on external edits | `SettingsStore::watch_settings_files` | yes |
| FR-003 | Watch the keymap file (and base-keymap/vim/helix settings, and OS keyboard layout) and hot-reload keybindings | `handle_keymap_file_changes` | yes |
| FR-004 | Back up then rewrite `settings.json`/`keymap.json` when a deprecated-schema field is detected | `write_settings_migration`/`write_keymap_migration` | yes |
| FR-005 | Resolve a feature flag's on/off (or variant) value per staff/override/server precedence, and re-render observers on change | `FeatureFlagStore::try_flag_value`, `FeatureFlagAppExt::observe_flag` | yes |

**Source:** `crates/settings/src/settings_store.rs:350-401` (FR-001/002), `crates/zed/src/zed.rs`
(`handle_keymap_file_changes`, FR-003), `crates/zed/src/zed/migrate.rs:278-326` (FR-004),
`crates/feature_flags/src/store.rs:155-176` (FR-005)

### Business Rules

_(See itemized entries below.)_

### BR-001_SettingsFilePrecedenceOrder
**Linked FR:** FR-001
**Source:** `crates/settings/src/settings_store.rs:432-441` (`merged_settings` field), `data-model.md` DISC-010
**Applies to:** `SettingsStore::get<T>` — every settings read in the app
**Rule:** Merged settings resolve with strict precedence Project > Server > User ≈ Global >
Default. A project-local `.zed/settings.json` value always beats a user-level one, but a user or
global override always beats the shipped default. This ordering is fixed by the `SettingsFile`
enum's `Ord` implementation, not user-configurable.

**Pseudocode:**
```text
fn merge_layers(default, global, user, server, project_for(worktree)):
    result = default
    result = apply_over(result, global)
    result = apply_over(result, user)
    result = apply_over(result, server)
    if project_for(worktree).is_some():
        result = apply_over(result, project_for(worktree))
    return result
```

### BR-002_NonUserKeybindingReplaceBecomesAdd
**Linked FR:** FR-003
**Source:** `crates/settings/src/keymap_file.rs:867-901` (`KeymapFile::update_keybinding`)
**Applies to:** `KeymapEditor::save`/`save_keybinding_update`
**Rule:** Editing a keybinding that did NOT originate from the user's own keymap file (e.g. a
base-keymap-preset or extension binding) is never rewritten in place. Instead the editor writes a
new user-level `Add` entry for the new keystroke and, if the keystrokes changed, an explicit
unbind (`suppression_unbind`) entry to suppress the original non-user binding — the underlying
non-user source file itself is never touched.

**Pseudocode:**
```text
fn update_keybinding(op):
    if op is Replace and target_source != User:
        if target.keystrokes != source.keystrokes:
            suppression_unbind = target
        op = Add { source, from: target }
    apply(op)  # only ever writes to the user keymap file
```

### BR-003_BackupWrittenBeforeLiveFileIsRewritten
**Linked FR:** FR-004
**Source:** `crates/zed/src/zed/migrate.rs:283-299` (`write_keymap_migration`), `:308-324`
(`write_settings_migration`)
**Applies to:** "Backup and Update" click handler in `MigrationBanner`
**Rule:** For both keymap and settings migration, the ordering is fixed: load current text →
compute migrated text via `migrate_keymap`/`migrate_settings` → if the live file exists, write the
**unmigrated** old text to the backup path (`keymap_backup.json`/`settings_backup.json`) first →
only then overwrite the live file with the migrated text. Each `fs.atomic_write` call is
individually errored via `.context(...)`, so a failure at the backup step aborts before the live
file is ever touched.

**Pseudocode:**
```text
fn write_X_migration(fs):
    old_text = load(fs)
    new_text = migrate(old_text) or return  # no-op if nothing to migrate
    if fs.is_file(live_path):
        fs.atomic_write(backup_path, old_text)?   # backup happens FIRST
        resolved = fs.canonicalize(live_path)?
        fs.atomic_write(resolved, new_text)?      # live file overwritten SECOND
    else:
        fs.atomic_write(live_path, new_text)?
```

### BR-004_StaffFlagDefaultUnlessOverridden
**Linked FR:** FR-005
**Source:** `crates/feature_flags/src/feature_flags.rs:114-132` (`FeatureFlag` trait defaults),
`crates/feature_flags/src/store.rs:155-176` (`try_flag_value`)
**Applies to:** every `impl FeatureFlag` type in `crates/feature_flags/src/flags.rs`
**Rule:** A flag resolves in strict precedence: `enabled_for_all()` (default `false`) always wins
if true; else a per-flag user override read from `FeatureFlagsSettings`; else "on" if the account
is staff (or a debug build) and `ZED_DISABLE_STAFF` is not set and `enabled_for_staff()` (default
`true`) holds; else a server-pushed flag value; else `None`/off.

**Pseudocode:**
```text
fn try_flag_value(flag):
    if flag.enabled_for_all(): return on_variant
    if override = settings.overrides.get(flag.NAME): return variant_from_key(override)
    if (debug_build or store.staff) and !ZED_DISABLE_STAFF and flag.enabled_for_staff():
        return on_variant
    if wire = store.server_flags.get(flag.NAME): return from_wire(wire)
    return None
```

### Decision Logic

N/A — no user-facing decision logic beyond DISC-010/DISC-011 Polymorphic Behavior. The
settings-file target selection (`SettingsUiFile::Project|User|Server`) and keybinding
replace-vs-add routing (BR-002) are single-field/config-driven branches already captured as BR
rules, not multi-predicate render/interaction/flow decisions.

### State Machines

None — no state machine in this feature crosses the ≥3-state/≥2-transition threshold; settings
apply, keymap reload, and migration are each linear, non-branching async operations captured as
BR/ALG entries instead.

### Algorithms

_(See itemized entries below.)_

### ALG-001_SettingsAtomicFileUpdate
**Linked FR:** FR-001
**Source:** `crates/settings/src/settings_store.rs:552-607` (`update_settings_file_inner`)
**Input:** current on-disk settings text + a caller-supplied `FnOnce(&mut SettingsContent, &App)` mutation
**Output:** new on-disk `settings.json` content + updated in-memory `SettingsStore`
**File Schema**: N/A — not a file-exchange type (single JSON config file, not a bulk import/export format)
**Complexity:** O(n) in settings-file size (parse + re-serialize)
**Description:** Serializes settings-file writes through a single unbounded channel
(`setting_file_updates_tx`) processed one at a time, so concurrent UI edits never race on the same
file. Each update loads the current text, applies the caller's mutation to a parsed
`SettingsContent`, resolves the settings path (canonicalizing it if it already exists so a symlink
target is written through), calls `fs.atomic_write`, then updates the in-memory store via
`set_user_settings` before completing the caller's oneshot receiver.

**Pseudocode:**
```text
fn update_settings_file_inner(fs, update):
    enqueue(async {
        old_text = load_settings(fs)
        new_text = update(old_text)
        path = settings_file()
        if fs.is_file(path): path = fs.canonicalize(path)
        fs.atomic_write(path, new_text)
        store.set_user_settings(new_text)
    })
```

### External Integrations

_(See itemized entries below.)_

### INT-001_ThemeAndKeymapFileWatch
**Linked FR:** FR-002, FR-003
**Source:** `crates/settings/src/settings_file.rs:165` (`watch_config_file`, BL137),
`crates/zed/src/main.rs` (`watch_themes`, BL206), `crates/zed/src/zed.rs`
(`handle_keymap_file_changes`, BL143)
**Type:** queue-worker / observer (local filesystem, not a network integration)
**Target:** OS filesystem watch (`Fs::watch`) on `settings.json`, `global_settings.json`,
`keymap.json`, and the active theme file
**Trigger:** Any write to a watched config file, whether from this feature's own writers or an
external editor
**Payload:** raw file content on each change batch
**Failure handling:** a parse failure records a `SettingsParseResult` error in
`SettingsStore::file_errors` rather than aborting the watch loop; the stream keeps running so a
subsequent valid write recovers automatically

**Pseudocode:**
```text
loop:
    batch = fs.watch(path).next()
    content = fs.load(path)
    result = parse_and_apply(content)
    if result.is_err(): file_errors[path] = result
    notify_observers()
```

### Verification

- **SC-001** — Changing a setting in the Settings UI updates `settings.json` on disk and every
  open editor observing that key re-renders without restart (covers FR-001, US058)
- **SC-002** — Rebinding a key in the keymap editor writes the new binding to `keymap.json` and
  the old binding stops firing immediately (covers FR-003, US059, BR-002)
- **SC-003** — Clicking "Backup and Update" always produces a readable backup file before the
  live file's content changes (covers FR-004, US061, US062, BR-003)
- **SC-012** — A feature flag's resolved value respects staff/override/server precedence order,
  and every registered observer re-renders when the resolved value changes (covers FR-005)

---

**Client behavior:** see
[`behavior-logic.md`](../../generated/behavior-logic.md) (client-side patterns — debounce, optimistic UI, polling, upload, realtime),
[`permissions.md`](../../system/permissions.md) (feature flags / experiments / env / locale gates),
[`screen-flow.md`](../../generated/screen-flow.md) (guards / deep-link state restoration / unsaved-changes protection).

## User Stories

### US058_EditSettingsJson — Edit a setting in settings.json (Priority: P1)

**What happens:** A developer changes a value in the Settings UI (or hand-edits `settings.json`
directly). If via the UI, `update_settings_file` queues an async update through
`SettingsStore::update_settings_file_inner`, which loads the current file, applies the mutation,
writes it back with `fs.atomic_write`, and immediately calls `set_user_settings` on the in-memory
store. If via a direct file edit, `BL138_SettingsStoreFileWatcher` picks up the filesystem event
and applies the same `set_user_settings` path. Either way every `impl Settings` consumer observing
that key re-renders on the next `cx.refresh_windows()`.
**Why this priority:** Live settings editing without restart is this feature's most-used
capability — nearly every other Zed feature reads its configuration through this path.
**Independent Test:** Change `buffer_font_size` in the Settings UI, confirm an already-open editor
immediately re-renders at the new size without restarting Zed.

**Acceptance Scenarios:**

1. **Given** the Settings UI is open on the `buffer_font_size` field, **When** the developer types
   a new value, **Then** `settings.json` is rewritten and the value change is visible in an open
   editor within one settings-store refresh cycle.
2. **Given** `settings.json` is hand-edited externally while Zed is running, **When** the file is
   saved, **Then** the change is detected and applied the same way as a UI edit.

**Requirements fulfilled:**
- **FR-001** Apply a live settings change to `SettingsStore` and re-render consumers — via
  `SettingsStore::update_settings_file`/`update_settings_file_with_completion`
  **Source:** `crates/settings/src/settings_store.rs:552-635`
- **FR-002** Watch user/global settings files and hot-reload on external edits — via
  `SettingsStore::watch_settings_files`
  **Source:** `crates/settings/src/settings_store.rs:350-401`

**Rules enforced:** BR-001 (see Cross-Cutting Logic) — the new value is applied through the same
precedence merge as any other settings read.

**Verification:**
- **SC-001** (see Cross-Cutting Logic)

---

### US059_EditKeymapBinding — Edit a keymap binding (Priority: P1)

**What happens:** A developer opens `KeybindingEditorModal` from the keymap editor, records a new
keystroke via `KeystrokeInput` (BL037), and confirms. `save`/`save_keybinding_update` loads the
current keymap file text, builds a `KeybindUpdateOperation` (Replace or Add depending on whether
the target binding is user-owned, per BR-002), calls `KeymapFile::update_keybinding` to compute
the new file text, then `fs.write`s it to `keymap.json`. `handle_keymap_file_changes` (BL143)
picks up the resulting file-watch event and reloads bindings, so the new binding takes effect
immediately.
**Why this priority:** Rebinding keys is the primary reason developers open the keymap editor
rather than hand-editing JSON.
**Independent Test:** Rebind `Save` from `Cmd-S` to `Cmd-Shift-S`; confirm `Cmd-Shift-S` now
triggers Save and `Cmd-S` no longer does, without restarting Zed.

**Acceptance Scenarios:**

1. **Given** the keymap editor is open, **When** the developer records a new keystroke for an
   action and confirms, **Then** `keymap.json` is rewritten with the new binding.
2. **Given** the binding being edited originates from a non-user source (base keymap or
   extension), **When** the developer confirms the edit, **Then** a new user-level `Add` entry
   (plus an unbind entry if the keystrokes changed) is written, and the non-user source file is
   left untouched (BR-002).

**Requirements fulfilled:**
- **FR-003** Watch the keymap file and hot-reload keybindings — via `handle_keymap_file_changes`
  **Source:** `crates/zed/src/zed.rs` (`handle_keymap_file_changes`, symbol confirmed via
  `behavior-logic.md` BL143)

**Rules enforced:** BR-002 (see Cross-Cutting Logic) — non-user bindings are never edited in
place.

**Verification:**
- **SC-002** (see Cross-Cutting Logic)

---

### US060_SwitchBaseKeymapPreset — Switch the base keymap preset (Priority: P2)

**What happens:** A developer opens `BaseKeymapSelector` (command palette or onboarding flow) and
confirms a preset (e.g. VS Code, Sublime). `BaseKeymapSelectorDelegate::confirm` resolves the
`BaseKeymap` from the selection and calls `update_settings_file` to write
`setting.base_keymap = Some(base_keymap)` into `settings.json`. The resulting settings change is
picked up by `handle_keymap_file_changes` (BL143), which treats a base-keymap setting change the
same as a keymap file change and triggers a keybinding rebuild layered under any custom user
bindings.
**Why this priority:** Useful for onboarding developers migrating from another editor, but not
required for day-to-day settings/keymap editing — hence `should`, not `must`.
**Independent Test:** Switch the base keymap from Default to VS Code; confirm VS Code-style
bindings (e.g. `Ctrl-Shift-P`) become active immediately across all open windows.

**Acceptance Scenarios:**

1. **Given** the default base keymap is active, **When** the developer selects a different preset
   in `BaseKeymapSelector`, **Then** that preset's default bindings become active immediately,
   layered under any custom user bindings.

**Requirements fulfilled:**
- **FR-001** (see Cross-Cutting Logic) — base keymap change goes through the same settings-write
  path as any other setting
  **Source:** `crates/onboarding/src/base_keymap_picker.rs:174-199`

**Rules enforced:** BR-001 (see Cross-Cutting Logic) — the new `base_keymap` value participates in
the same precedence merge before `handle_keymap_file_changes` diffs it.

**Verification:**
- **SC-004** Selecting a base keymap preset applies its bindings across all open windows without
  restart (covers FR-001, FR-003)

---

### US061_BackupSettingsBeforeMigration — Back up settings before a schema migration (Priority: P1)

**What happens:** When `MigrationBanner::set_active_pane_item` detects the active editor targets
`settings.json`/`keymap.json` and `should_migrate_settings`/`should_migrate_keymap` reports a
deprecated field is present, the banner shows a message naming the exact backup filename
(`paths::settings_backup_file()`/`paths::keymap_backup_file()`). Clicking "Backup and Update"
spawns `write_settings_migration`/`write_keymap_migration`, which writes the **unmigrated**
current file content to the backup path via `fs.atomic_write` before anything else happens
(BR-003).
**Why this priority:** Protecting the user's prior configuration is a correctness/trust
requirement — an update must never silently destroy a working config.
**Independent Test:** With a settings file containing a deprecated field, click "Backup and
Update"; confirm `settings_backup.json` exists and matches the pre-migration content, written
before the live file changes.

**Acceptance Scenarios:**

1. **Given** `settings.json` uses a deprecated schema field, **When** the developer clicks "Backup
   and Update", **Then** `settings_backup.json` is written with the original (unmigrated) content
   before the live file is touched.

**Requirements fulfilled:**
- **FR-004** (see Cross-Cutting Logic) — via `write_settings_migration`/`write_keymap_migration`
  **Source:** `crates/zed/src/zed/migrate.rs:278-326`

**Rules enforced:** BR-003 (see Cross-Cutting Logic).

**Verification:**
- **SC-003** (see Cross-Cutting Logic)

---

### US062_MigrateSettingsToCurrentSchema — Migrate settings to the current schema (Priority: P1)

**What happens:** Immediately after the backup write in `write_settings_migration`/
`write_keymap_migration` succeeds, the same function canonicalizes the live file path and
`fs.atomic_write`s the migrated text (computed earlier via `migrate_settings`/`migrate_keymap`
from the `migrator` crate) to the live `settings.json`/`keymap.json`. If the live file did not
exist at all, the migrated text is written directly with no backup step (nothing to back up).
**Why this priority:** Without this step the backup alone doesn't fix anything — the live file
must actually end up parseable under the current schema so the editor keeps working after update.
**Independent Test:** With a backed-up, deprecated-schema settings file, run the migration;
confirm the live `settings.json` parses successfully under the current schema afterward.

**Acceptance Scenarios:**

1. **Given** `settings.json` has been backed up and uses a deprecated schema field, **When**
   migration applies to the live file, **Then** the live file parses successfully under the
   current schema.

**Requirements fulfilled:**
- **FR-004** (see US061) — via `write_settings_migration`/`write_keymap_migration`
  **Source:** `crates/zed/src/zed/migrate.rs:288-299, 313-324`

**Rules enforced:** BR-003 (see Cross-Cutting Logic) — migration write always follows the backup
write within the same function call, never independently.

**Verification:**
- **SC-003** (see Cross-Cutting Logic)

---

### Edge Cases

| Scenario | Behavior |
|----------|----------|
| Settings file has a parse error after an external hand-edit | Error recorded in `SettingsStore::file_errors` keyed by `SettingsFile`; watch loop keeps running rather than crashing (`crates/settings/src/settings_store.rs:443`) |
| User edits a non-user-owned keybinding's keystrokes | Treated as an `Add` (not `Replace`) plus a suppression unbind if keystrokes changed — original non-user source file is never rewritten (BR-002) |
| Backup write (`fs.atomic_write` to the backup path) fails | `.with_context(...)` wraps the error ("Failed to create settings backup in home directory"); the function returns early via `?` — the live file write in the same call never executes, so the live config is never corrupted by a partial migration |
| Live settings/keymap file does not exist yet at migration time | `fs.is_file` check is false, so no backup is written and the migrated text is written directly to the (new) live path (`crates/zed/src/zed/migrate.rs:295-299, 320-324`) |
| Staff account has `ZED_DISABLE_STAFF` set | Feature flags that default `enabled_for_staff() == true` resolve to off for that session, letting a developer verify the non-staff experience locally |

## Key Entities

| Entity | Table | Key Columns | Purpose |
|--------|-------|-------------|---------|
| SettingsStore (MODEL012) | in-memory only — not a DB table; backed by `settings.json`/`global_settings.json`/project `.zed/settings.json` files | `setting_values`, `user_settings`, `global_settings`, `local_settings`, `merged_settings`, `file_errors` | Central registry merging all settings layers; every `impl Settings` consumer reads through it |
| Theme / ThemeFamily (MODEL013) | in-memory only — not a DB table; backed by theme JSON files under the theme extension/bundled path | `id`, `name`, `appearance`, `styles` | Color/typography data model selected by the theme selector and applied app-wide |
| FeatureFlagStore | in-memory only — not a DB table | `staff`, `server_flags`, `server_flags_received` | Resolves each declared `FeatureFlag` type's on/off/variant value for the current session |

## Artifact References

| Artifact | File | Codes Used | Reviewed |
|----------|------|------------|----------|
| System Overview | [system-overview.md](../../system-overview.md) | — | [x] |
| Feature List | [feature-list.md](../../feature-list.md) | F015 | [x] |
| Entities | [entities.md](../../../../docs/generated/entities.md) | MODEL012, MODEL013 | [x] |
| Behavior Logic | [behavior-logic.md](../../../../docs/generated/behavior-logic.md) | BL040, BL061, BL036, BL048, BL207, BL037, BL070, BL096, BL206, BL063, BL139, BL137, BL138, BL143, BL169, BL193, BL101, BL127 | [x] |
| Permissions Matrix | [permissions-matrix.md](../../../../docs/generated/permissions-matrix.md) | PERM006 | [x] |
| Permissions (curated) | [permissions.md](../../../../docs/system/permissions.md) | PERM006 | [x] |
| User Stories | [user-stories.md](../../../../docs/generated/user-stories.md) | US058, US059, US060, US061, US062 | [x] |

**Note (generic-source profile):** no `route-list.md`/`screen-list.md` exist for this Rust/GPUI
codebase — `ROUTE###`/`SCR###` references are intentionally omitted rather than fabricated.

## Assumptions

- `BL101_EncryptedPasswordAtRest` (secrets-at-rest for SSH/git askpass credentials) is included in
  this feature's Related Background Logic per `feature-list.md`, but its actual call sites
  (`crates/askpass`) are credential-flow adjacent rather than settings/keymap editing proper; it
  is documented here only as "no better home in this partition" context and is not otherwise
  integrated into the User Stories above.
- `BL040_OpenKeyContextViewAction` (dev-tools key-context debug view) is a diagnostic aid for
  understanding keybinding predicates, not a keymap-editing action itself; included for
  completeness per `feature-list.md`'s Related Background Logic but not elaborated as its own
  User Story.
- Theme JSON hot-reload (BL206/BL070) is assumed to apply to both bundled and extension-contributed
  theme files uniformly; the exact reload trigger path for extension-contributed themes specifically
  was not traced line-by-line in this pass.

## Source Code References

| Order | Symbol | Path | Purpose |
|-------|--------|------|---------|
| 1 | `SettingsStore` | `crates/settings/src/settings_store.rs:145, 287-441` | Central settings registry; entity definition + layer precedence |
| 2 | `SettingsStore::watch_settings_files`/`update_settings_file_inner` | `crates/settings/src/settings_store.rs:350-401, 552-635` | Live settings apply + file-watch reload path |
| 3 | `SettingsWindow`/`update_settings_file` | `crates/settings_ui/src/settings_ui.rs:720-859, 3862-3887, 1906-1983` | Settings UI entry point, write dispatch by file target, search |
| 4 | `KeymapEditor`/`save_keybinding_update` | `crates/keymap_editor/src/keymap_editor.rs:432-541, 3600-3672` | Keymap editor UI + keybinding write path |
| 5 | `KeymapFile::update_keybinding` | `crates/settings/src/keymap_file.rs:867-905` | Replace-vs-add / suppression-unbind logic (BR-002) |
| 6 | `MigrationBanner`/`write_settings_migration`/`write_keymap_migration` | `crates/zed/src/zed/migrate.rs:25-137, 262-326` | Backup-then-migrate flow (BR-003) |
| 7 | `BaseKeymapSelectorDelegate::confirm` | `crates/onboarding/src/base_keymap_picker.rs:174-199` | Base keymap preset switch |
| 8 | `Theme`/`ThemeFamily`/`Appearance` | `crates/theme/src/theme.rs:54-59, 192-220` | Theme data model + appearance discriminator |
| 9 | `FeatureFlagStore`/`FeatureFlag` trait | `crates/feature_flags/src/feature_flags.rs:20-132`, `crates/feature_flags/src/store.rs:69-236` | Staff feature-flag resolution (PERM006, BR-004) |

## Unresolved Questions

1. **`handle_keymap_file_changes` exact line range**: the function's presence in
   `crates/zed/src/zed.rs` is confirmed via `behavior-logic.md` (BL143) and by the base-keymap
   picker's downstream effect, but its own precise line range was not independently re-verified
   with a fresh `grep`/`Read` pass in this session — treat the citation above as file-level, not
   line-verified.
2. **Extension-contributed theme reload path**: whether `BL206_WatchThemeFileChanges` also covers
   theme files loaded lazily from installed extensions (vs. only the bundled/active theme file) was
   not confirmed by reading `crates/theme_extension` in this pass.

## Source Walkthrough

1. **File:** `crates/settings/src/settings_store.rs:145, 287-441` — start here: the
   `SettingsStore` entity and its layered-merge model that every other file in this walkthrough
   ultimately reads through or writes into.
2. **File:** `crates/settings_ui/src/settings_ui.rs:3862-3887` — next: `update_settings_file`, the
   single dispatch point the Settings UI uses to route a field edit to `SettingsStore` (user file)
   or the project settings queue.
3. **File:** `crates/keymap_editor/src/keymap_editor.rs:3600-3672` — next: `save_keybinding_update`,
   the keymap editor's equivalent write path, contrasting with settings (JSON merge-patch vs.
   keybinding-array Replace/Add logic).
4. **File:** `crates/settings/src/keymap_file.rs:867-905` — next: `KeymapFile::update_keybinding`,
   the core replace-vs-add decision (BR-002) that the keymap editor write path delegates to.
5. **File:** `crates/zed/src/zed/migrate.rs:262-326` — last: the backup-then-migrate functions
   that tie settings and keymap files together under one safety guarantee (BR-003) when the app
   detects a deprecated schema field.

### Call Hierarchy

```text
Settings UI field edit
  -> settings_ui::update_settings_file (routes by SettingsUiFile)
       -> SettingsStore::update_settings_file / update_settings_file_with_completion
            -> update_settings_file_inner -> fs.atomic_write(settings.json) -> set_user_settings

Keymap editor binding edit
  -> KeymapEditor::save -> save_keybinding_update
       -> KeymapFile::update_keybinding (BR-002: Replace-if-user else Add+unbind)
            -> fs.write(keymap.json)
                 -> handle_keymap_file_changes (file-watch reload, BL143)

"Backup and Update" click (MigrationBanner)
  -> write_settings_migration / write_keymap_migration
       -> migrate_settings / migrate_keymap (compute new text)
       -> fs.atomic_write(backup_path, old_text)   [BR-003: backup FIRST]
       -> fs.atomic_write(live_path, new_text)     [migration SECOND]
```

**Related files:** see `## Source Code References` above.

## DB Impact per Event

N/A — read-only feature with respect to any database. Every write in this feature (settings
edits, keymap edits, base-keymap preset changes, theme selection, and the backup-then-migrate
flow) goes through the `fs::Fs` trait's `atomic_write`/`write` methods against plain JSON files on
disk (`settings.json`, `global_settings.json`, `keymap.json`, their `*_backup.json` siblings, and
project `.zed/settings.json` files) — confirmed at `crates/settings/src/settings_store.rs:575-586`,
`crates/keymap_editor/src/keymap_editor.rs:3658-3663`, and
`crates/zed/src/zed/migrate.rs:285-323`. None of these paths touch Zed's SQLite `db` crate.
