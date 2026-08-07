# F016_Search: Technical Spec
**Priority**: P0
**Type**: ui
**Generated**: 2026-08-07

## Overview

Search covers the three "find" surfaces of the editor: fuzzy file finding (`FileFinder`,
`crates/file_finder/src/file_finder.rs`), whole-project text/regex search rendered as a
multi-buffer (`ProjectSearchView`, `crates/search/src/project_search.rs`), and in-buffer
find/replace (`BufferSearchBar`, `crates/search/src/buffer_search.rs`). All three are opened via
keybinding or the command palette; the command palette itself (`crates/command_palette/src/command_palette.rs`)
is included here because its post-confirm invocation logging (BL146) is the one DB-writing event
in this feature's scope. Project search results are not a bespoke view — they are rendered as a
`MODEL009_MultiBuffer` of anchored excerpts around each match, the same aggregation mechanism
diagnostics and diff views use.

## Polymorphic Behavior

N/A — no discriminator fields in Key Entities. `MultiBuffer` (MODEL009) declares
`**Discriminator Fields**: None (delegates to Buffer's `capability` discriminator)` in
`data-model.md`, and `Buffer.capability`/`Buffer.parse_status` (DISC-007/DISC-008) are not listed
as Key Entities for this feature — search never reads or branches on either field
(`crates/search/src/project_search.rs`, `crates/search/src/buffer_search.rs`,
`crates/file_finder/src/file_finder.rs` contain no reference to `capability` or `parse_status`).
The in-code `SearchQuery::Text`/`SearchQuery::Regex` enum (`crates/project/src/search.rs:61-80`)
and file-finder's `Match::History`/`Match::Search`/`Match::CreateNew` enum
(`crates/file_finder/src/file_finder.rs:453-460`) are real two/three-way behavioral forks, but
neither is a `data-model.md` entity — they are documented as ALG-001 and edge-case rows below
instead, per the contract's DISC-vs-plain-branch boundary (DISC is scoped to `data-model.md`
entities only).

## Cross-Cutting Logic

### Requirements

| Code | Description | Endpoint/Handler | Verifiable |
|------|-------------|-------------------|------------|
| FR-001 | Fuzzy-match the typed query against all files in visible worktrees, ranked, capped at 100 results | `FileFinderDelegate::spawn_search` | yes |
| FR-002 | Fall back to regex search automatically when a case-insensitive text query contains non-ASCII characters | `SearchQuery::text` | yes |
| FR-003 | Cap whole-project search at 5,000 matched files / 10,000 total ranges and flag when the cap is hit | `project::project_search::Search` | yes |
| FR-004 | Log every confirmed command-palette invocation (command name + query text) to a capped SQLite table | `CommandPalette::confirm`, `CommandPaletteDB::write_command_invocation_internal` | yes |

**Source:** `crates/file_finder/src/file_finder.rs:865-921`, `crates/project/src/search.rs:93-119`, `crates/project/src/project_search.rs:154-155,566-572`, `crates/command_palette/src/persistence.rs:74-123`

### Business Rules

_(See itemized entries below.)_

### BR-001_FileFinderResultCap
**Linked FR:** FR-001
**Source:** `crates/file_finder/src/file_finder.rs:900-910`
**Applies to:** `FileFinderDelegate::spawn_search`
**Rule:** `fuzzy_nucleo::match_path_sets` is invoked with a hard-coded limit of 100 results per search; results beyond that rank are never computed/returned, so the picker list never exceeds 100 fuzzy matches regardless of project size.

**Pseudocode:**
```text
spawn_search(query):
  candidate_sets = one PathMatchCandidateSet per visible worktree
  matches = fuzzy_nucleo::match_path_sets(candidate_sets, query, relative_to, ignore_case, limit=100, cancel_flag)
  update picker.matches with ranked results (<=100)
```

---

### BR-002_NonAsciiCaseInsensitiveFallbackToRegex
**Linked FR:** FR-002
**Source:** `crates/project/src/search.rs:93-119`
**Applies to:** `SearchQuery::text`
**Rule:** `AhoCorasickBuilder::ascii_case_insensitive` cannot do case-insensitive matching on non-ASCII text, so `SearchQuery::text` detects `!case_sensitive && !query.is_ascii()` and transparently re-dispatches to `SearchQuery::regex` with the literal query escaped (`regex::escape`), preserving the other flags (whole_word, include_ignored) but forcing `multiline=false`.

**Pseudocode:**
```text
text(query, case_sensitive, ...):
  if not case_sensitive and query has non-ascii chars:
    return regex(escape(query), whole_word, case_sensitive, include_ignored,
                 multiline=false, files_to_include, files_to_exclude,
                 match_full_paths=false, buffers)
  else: build AhoCorasick matcher directly
```

---

### BR-003_ProjectSearchResultCapWithFlag
**Linked FR:** FR-003
**Source:** `crates/project/src/project_search.rs:154-155,566-572`, `crates/search/src/project_search.rs:428-486`
**Applies to:** `project::project_search::Search::search`, `ProjectSearchView`
**Rule:** A project-wide search stops accumulating once it has matched more than 5,000 files (`MAX_SEARCH_RESULT_FILES`) or 10,000 total ranges (`MAX_SEARCH_RESULT_RANGES`), sending a `SearchResult::LimitReached` marker instead of further buffer results. The view sets `limit_reached = true` and renders the match counter as `"{index}/{count}+"` (trailing `+`) instead of the exact count.

**Pseudocode:**
```text
on each search result batch:
  if matched_buffers > 5000 or matches > 10000:
    send LimitReached; stop yielding further results
on view update:
  limit_reached |= any LimitReached received this batch
  match_text = limit_reached ? "{idx}/{count}+" : "{idx}/{count}"
```

---

### BR-004_BufferSearchInvalidRegexBlocksUpdate
**Linked FR:** —
**Source:** `crates/search/src/buffer_search.rs:1584-1631`
**Applies to:** `BufferSearchBar::update_matches`
**Rule:** When the regex-mode query fails to compile (`SearchQuery::regex` returns `Err`), the bar records the error string in `self.query_error`, clears any existing highlighted matches on the active searchable item, and returns without running a search — it never falls back to a text search or crashes. The same fallback happens for a malformed text-mode query (also fallible, since it can internally re-dispatch to regex per BR-002).

**Pseudocode:**
```text
update_matches():
  if query is empty: clear matches; return
  query_result = regex_mode ? SearchQuery::regex(...) : SearchQuery::text(...)
  match query_result:
    Ok(q) -> proceed to search
    Err(e) -> self.query_error = e.to_string(); clear_active_searchable_item_matches(); return
```

---

### BR-005_CommandInvocationLogCappedAt1000
**Linked FR:** FR-004
**Source:** `crates/command_palette/src/persistence.rs:118-123`
**Applies to:** `CommandPaletteDB::write_command_invocation_internal`
**Rule:** Every confirmed command palette selection inserts one row into `command_invocations` (command_name, user_query, default `last_invoked = unixepoch()`), then immediately deletes the oldest row(s) whenever more than 1,000 rows exist in total, keeping the table capped near 1,000 entries. Both statements run in the same query, and errors from either are logged rather than surfaced (`crates/command_palette/src/command_palette.rs` calls this from a detached background task per BL146).

**Pseudocode:**
```text
write_command_invocation(command_name, user_query):
  INSERT INTO command_invocations (command_name, user_query) VALUES (?, ?)
  DELETE FROM command_invocations WHERE id IN (SELECT MIN(id) FROM command_invocations HAVING COUNT(1) > 1000)
  # errors: log_err(), not surfaced to user
```

### Decision Logic

N/A — no user-facing decision logic beyond DISC-### Polymorphic Behavior. The nearest candidates
— `Match::History`/`Match::Search`/`Match::CreateNew` selection in the file finder, and
`SearchQuery::Text`/`Regex` dispatch — are each single-field/type-driven branches with no
multi-predicate render, no interaction-driven reveal, and no in-feature flow/step routing; they
are documented instead as BR-002/FR-002 (query-type dispatch) and edge-case rows (finder-match
kind) since they change *what data feeds the result*, not *where the user navigates or what panel
they see*.

### State Machines

None — no persisted or ≥3-state/≥2-transition UI state machine was found local to this feature.
`BufferSearchBar`/`ProjectSearchView`'s internal booleans (`dismissed`, `no_results`,
`limit_reached`, `query_error: Option<String>`) are simple flags below the SM threshold in the
contract (fewer than 3 states, or a single boolean transition), so they stay implicit in the BR
rules above rather than becoming SM-### blocks.

### Algorithms

_(See itemized entries below.)_

### ALG-001_BufferTextSearchStream
**Linked FR:** FR-002
**Source:** `crates/project/src/search.rs:427-479`
**Input:** `SearchQuery` (Text or Regex variant) + a `BufferSnapshot` + optional byte-range subrange
**Output:** `Vec<Range<usize>>` of byte-offset match ranges within the buffer
**File Schema**: N/A — not a file-exchange type
**Complexity:** O(n) over buffer length, streamed with a cooperative yield every 20,000 matches (`YIELD_INTERVAL`)
**Description:** Runs an `AhoCorasick` (Text variant) or compiled `Regex` (Regex variant) scan over the buffer's rope, optionally restricted to a subrange (used for search-within-selection). For whole-word mode, each candidate match is boundary-checked against the buffer's `CharKind` classifier (word/non-word) on both edges before being accepted.

**Pseudocode:**
```text
search(query, buffer, subrange):
  if query text is empty: return []
  rope = subrange ? buffer.slice(subrange) : buffer.full_rope()
  for each match in query.engine.stream_find_iter(rope):
    if whole_word and (start boundary is mid-word or end boundary is mid-word): skip
    else: matches.push(match.range)
    every 20000 matches: yield_now()
  return matches
```

### External Integrations

None — search runs entirely against in-process buffer/worktree state; command palette logging
writes to a local SQLite file (`CommandPaletteDB`), which is covered as a DB write in `## DB
Impact per Event`, not an external service integration.

### Verification

- **SC-001** — A fuzzy file-finder query against a project with >100 matching files never returns more than 100 ranked results (covers FR-001, BR-001).
- **SC-002** — A case-insensitive text query containing a non-ASCII character still matches correctly, proving the automatic regex fallback ran (covers FR-002, BR-002).
- **SC-003** — A project search matching more than 5,000 files shows a `+`-suffixed match count and `limit_reached = true` (covers FR-003, BR-003).
- **SC-004** — Confirming 1,001 distinct commands over time leaves `command_invocations` at ~1,000 rows, never growing unbounded (covers FR-004, BR-005).

---

**Client behavior:** see
[`behavior-logic.md`](../../generated/behavior-logic.md) (client-side patterns — debounce, optimistic UI, polling, upload, realtime),
[`permissions.md`](../../system/permissions.md) (feature flags / experiments / env / locale gates),
[`screen-flow.md`](../../generated/screen-flow.md) (guards / deep-link state restoration / unsaved-changes protection).

## User Stories

### US063_FindFileByFuzzyName — Find a file by fuzzy name (Priority: P1/must)

**What happens:** The developer opens the file finder modal, types a partial/fuzzy filename, and the picker ranks and lists matching project files (plus, if the query resolves to no existing file, a "create new file" candidate). Confirming the highlighted result opens that file in the active pane.
**Why this priority:** Marked `must` — this is the primary "open a file" path for most developers, faster than navigating the project tree.
**Independent Test:** Open the file finder, type a 3-4 character fuzzy fragment of a known file's name, confirm the top-ranked result is that file, confirm it opens on Enter.

**Acceptance Scenarios:**

1. **Given** `src/main.rs` exists in the project, **When** the developer types "mnrs" in the file finder, **Then** `src/main.rs` appears as a top-ranked match.
2. **Given** the highlighted result is `src/main.rs`, **When** the developer confirms, **Then** the file opens in the active pane (or split, if confirmed with the secondary modifier).

**Requirements fulfilled:**
- **FR-001** Fuzzy-match the typed query against all files in visible worktrees, ranked, capped at 100 results — `FileFinderDelegate::spawn_search`
  **Source:** `crates/file_finder/src/file_finder.rs:865-921`

**Rules enforced:** BR-001 (see Cross-Cutting Logic)

**State transitions:** N/A — see Cross-Cutting Logic § State Machines.

**Verification:**
- **SC-001** (see Cross-Cutting Logic, Verification)
- **SC-005** — Typing a query with no existing-file match surfaces a `Match::CreateNew` candidate at the bottom of the list, and confirming it creates/opens that path (covers BL159, `crates/file_finder/src/file_finder.rs:991-1002`).

---

### US064_SearchAcrossProject — Search across the whole project (Priority: P1/must)

**What happens:** The developer opens project search, submits a text or regex query (optionally scoped by include/exclude glob filters), and matches stream in grouped by file with surrounding context lines, rendered as excerpts in a `MultiBuffer` the developer can edit in place; `ReplaceAll`/`ReplaceNext` apply the query's replacement text across matches.
**Why this priority:** Marked `must` — finding every usage of a symbol/string across the codebase is a baseline navigation need.
**Independent Test:** Introduce a known string in 5 files, run project search for it, confirm all 5 files' matches appear with context and the result can be opened for in-place editing.

**Acceptance Scenarios:**

1. **Given** "TODO" appears in 5 files, **When** the developer searches "TODO" project-wide, **Then** all 5 files' matches are listed with surrounding context lines.
2. **Given** results are showing, **When** the developer opens the results view, **Then** it behaves as an editable multi-buffer (edits to an excerpt affect the underlying file).
3. **Given** the query text fails to compile as a regex (regex mode on), **When** the developer submits it, **Then** the Query panel is marked with an inline error and no search runs.

**Requirements fulfilled:**
- **FR-002** Fall back to regex search automatically when a case-insensitive text query contains non-ASCII characters — `SearchQuery::text`
  **Source:** `crates/project/src/search.rs:93-119`
- **FR-003** Cap whole-project search at 5,000 matched files / 10,000 total ranges and flag when the cap is hit — `project::project_search::Search`
  **Source:** `crates/project/src/project_search.rs:154-155,566-572`

**Rules enforced:** BR-002 (see Cross-Cutting Logic), BR-003 (see Cross-Cutting Logic)

**State transitions:** N/A — see Cross-Cutting Logic § State Machines.

**Verification:**
- **SC-002** (see Cross-Cutting Logic, Verification)
- **SC-003** (see Cross-Cutting Logic, Verification)
- **SC-006** — Submitting an invalid regex in the project search Query field marks that field with `panels_with_errors[Query]` and does not dispatch a search (covers `crates/search/src/project_search.rs:1414-1445`).

---

### US065_SearchWithinCurrentBuffer — Search within the current buffer (Priority: P1/must)

**What happens:** The developer opens the in-buffer search bar (`FocusSearch`), types a query, and all matches in the active buffer highlight; `SelectNextMatch`/`SelectPreviousMatch` cycle the active match, `ToggleWholeWord`/`ToggleCaseSensitive`/`ToggleRegex`/`ToggleSelection` refine the query, and `ToggleReplace` + `ReplaceNext`/`ReplaceAll` perform in-place replacement.
**Why this priority:** Marked `must` — jumping between occurrences of a term in the open file is one of the most frequent editor actions.
**Independent Test:** Open a buffer containing "foo" 4 times, search "foo", confirm all 4 highlight and Next/Previous cycles through them in document order.

**Acceptance Scenarios:**

1. **Given** the active buffer contains "foo" 4 times, **When** the developer searches "foo", **Then** all 4 occurrences highlight and are navigable via Next/Previous.
2. **Given** a match is selected, **When** the developer triggers `ReplaceNext` with replacement text set, **Then** only that match is replaced and the next match becomes active.
3. **Given** the developer types an unbalanced/invalid regex with `ToggleRegex` on, **When** the query updates, **Then** the search bar shows an inline error and clears any stale highlights rather than crashing.

**Requirements fulfilled:**
- **FR-002** (see Cross-Cutting Logic) — regex/text dispatch applies identically to buffer search via `BufferSearchBar::update_matches`
  **Source:** `crates/search/src/buffer_search.rs:1584-1631`

**Rules enforced:** BR-004 (see Cross-Cutting Logic)

**State transitions:** N/A — see Cross-Cutting Logic § State Machines.

**Verification:**
- **SC-007** — An invalid regex query in the buffer search bar sets `query_error` and clears active matches without a panic (covers BR-004).

---

### Background Feature: Command Palette Invocation Logging (BL146)

**What happens:** After a command palette selection is confirmed and its action dispatched, a detached background task writes the command name and current query text to `command_invocations` for future fuzzy-ranking of frequently-used commands, then prunes the table back toward 1,000 rows.
**Why this priority:** Background/telemetry-adjacent logging that improves future command-palette ranking; not directly user-facing beyond its ranking effect over time.
**Independent Test:** Confirm a command in the palette, then query `command_invocations` (or re-open the palette and observe ranking) to confirm the row was written and old rows are pruned once >1,000 accumulate.

**Requirements fulfilled:**
- **FR-004** (see Cross-Cutting Logic) — `CommandPalette::confirm`, `CommandPaletteDB::write_command_invocation_internal`
  **Source:** `crates/command_palette/src/command_palette.rs` (confirm handler spawns the write), `crates/command_palette/src/persistence.rs:74-123`

**Rules enforced:** BR-005 (see Cross-Cutting Logic)

**Verification:**
- **SC-004** (see Cross-Cutting Logic, Verification)

### Edge Cases

| Scenario | Behavior |
|----------|----------|
| Fuzzy file-finder query matches no existing file and doesn't end in a path separator | A `Match::CreateNew` candidate is appended for that path, always sorted last (BR-001, `crates/file_finder/src/file_finder.rs:627-630,991-1002`) |
| Project search query fails to compile as a valid regex | The Query input panel is flagged in `panels_with_errors`, no search dispatches, and the error clears once the query becomes valid (`crates/search/src/project_search.rs:1414-1445`) |
| Project search matches more than 5,000 files or 10,000 ranges | Search stops accumulating further results and the UI shows a `+`-suffixed, possibly-undercount match total (BR-003) |
| Buffer search query is empty | All existing highlighted matches are cleared and no search runs (`crates/search/src/buffer_search.rs:1580-1583`) |
| A recent-history candidate's on-disk path no longer exists | `FileFinderDelegate::new`'s background `fs.is_file` check (BL159) drops the stale entry before it's offered as a match candidate |
| Command palette invocation logging write fails (DB error) | The error is logged, not surfaced to the user; the confirmed action itself still executes normally |

## Key Entities

| Entity | Table | Key Columns | Purpose |
|--------|-------|-------------|---------|
| MultiBuffer (MODEL009) | N/A — in-memory struct, not a DB table | snapshot, buffers, singleton, capability | Backs the rendered project-search results view: one excerpt per matched range, anchored into a single addressable editable buffer |
| SearchQuery (in-memory) | N/A — in-memory struct, not a DB table | Text{search, whole_word, case_sensitive}, Regex{regex, multiline, ...} | The compiled matcher (AhoCorasick or Regex) driving both buffer-local and project-wide search execution |
| command_invocations | `command_invocations` | id, command_name, user_query, last_invoked | The one persisted table this feature writes — logs confirmed command-palette selections for future ranking, capped at ~1,000 rows |

**Note:** `generic-source` profile (Rust/GPUI desktop app) — `MultiBuffer` and `SearchQuery` are
runtime structs, not database tables; `command_invocations` is the feature's only actual SQLite
table. See `## DB Impact per Event` below.

## Artifact References

| Artifact | File | Codes Used | Reviewed |
|----------|------|------------|----------|
| System Overview | [system-overview.md](../../system-overview.md) | — | [x] |
| Feature List | [feature-list.md](../../feature-list.md) | F016 | [x] |
| Entities | [entities.md](../../../../../docs/generated/entities.md) | MODEL009 | [x] |
| User Stories | [user-stories.md](../../user-stories.md) | US063, US064, US065 | [x] |
| Behavior Logic | [behavior-logic.md](../../behavior-logic.md) | BL020, BL059, BL060, BL146, BL159 | [x] |
| Business Rules | [business-rules.md](../../../../../docs/system/business-rules.md) | — | [x] |

**Rule:** Every code listed in Codes Used exists in its source artifact; `generic-source` profile
has no `route-list.md`/`screen-list.md`, so no `ROUTE###`/`SCR###` rows are included per
session-context instruction.

## Assumptions

- The 100-result fuzzy-finder cap (BR-001) and the 5,000-file/10,000-range project-search cap (BR-003) are assumed to be fixed constants, not user-configurable via settings — no `FileFinderSettings`/`ProjectSearchSettings` field overriding either constant was found in the crates read.
- `SearchHistory`/`SearchHistoryCursor` (query/include/exclude history navigable via `NextHistoryQuery`/`PreviousHistoryQuery`) is assumed to be in-memory/session-scoped only, since no persistence call (SQLite write, settings-file write) was found writing search history to disk.
- Command-palette invocation logging (BL146) is assumed to run for every confirmed command uniformly — no allowlist/denylist of "sensitive" commands was found excluding specific actions from being logged.

## Source Code References

| Order | Symbol | Path | Purpose |
|-------|--------|------|---------|
| 1 | `SearchQuery` | `crates/project/src/search.rs:61-479` | Core matcher construction (Text/Regex) + async per-buffer execution engine (ALG-001) |
| 2 | `Search` (project-wide) | `crates/project/src/project_search.rs:154-572` | Project-wide search orchestration, result-count caps (BR-003) |
| 3 | `ProjectSearchView` / `ProjectSearchBar` | `crates/search/src/project_search.rs:230-500,750-2200` | UI: query/include/exclude inputs, results MultiBuffer, replace-all, error panels |
| 4 | `BufferSearchBar` | `crates/search/src/buffer_search.rs:63-1650` | In-buffer find/replace bar: query handling, option toggles, next/prev match, replace |
| 5 | `FileFinderDelegate` | `crates/file_finder/src/file_finder.rs:390-1450` | Fuzzy file-finder picker delegate: search dispatch, history matching, confirm/open |
| 6 | `CommandPalette` / `CommandPaletteDB` | `crates/command_palette/src/command_palette.rs`, `crates/command_palette/src/persistence.rs:58-141` | Command palette confirm handler + capped invocation-log SQLite table |

## Unresolved Questions

1. **Fuzzy-finder 100-result cap tunability**: not confirmed whether any settings surface can raise/lower `fuzzy_nucleo::match_path_sets`'s hard-coded `100` limit, or whether it is intentionally fixed.
2. **Project-search cap user feedback beyond the `+` suffix**: the UI shows `{count}+` when the cap is hit, but this spec did not confirm whether any explicit toast/banner also explains *why* results stopped accumulating.
3. **Command-invocation log consumption**: `list_commands_used`/`list_recent_queries` (persistence.rs) clearly feed future command-palette ranking, but the exact ranking-weight formula that combines this history with static fuzzy-match score was not traced in this pass.

## Source Walkthrough

1. **File:** `crates/project/src/search.rs:61-119` — why start here: defines `SearchQuery`, the shared matcher type both project-wide and buffer-local search build and execute against.
2. **File:** `crates/search/src/search.rs:1-61` — next: the shared `actions!` (FocusSearch, ToggleRegex, etc.) and `SearchOptions` bitflags every search surface toggles.
3. **File:** `crates/file_finder/src/file_finder.rs:865-1002` — next: the fuzzy file-finder's search dispatch and CreateNew fallback, the simplest of the three surfaces.
4. **File:** `crates/search/src/project_search.rs:390-500` — next: `ProjectSearch::search`, which wires a `SearchQuery` into the project-wide async result stream and its `MultiBuffer` excerpt rendering.
5. **File:** `crates/search/src/buffer_search.rs:1205-1340` — last: `BufferSearchBar::search`/`update_matches`, the in-buffer counterpart, showing where BR-004's error handling lives.

### Call Hierarchy

```text
FocusSearch/Deploy/SearchInNew action
  -> BufferSearchBar::deploy | ProjectSearchBar::deploy | FileFinder toggle
    -> SearchQuery::text | SearchQuery::regex   (BR-002)
      -> project::search::Search::search (project-wide only, BR-003 caps)
        -> ProjectSearchView.excerpts (MultiBuffer, MODEL009)
      -> SearchQuery::search (per-buffer, ALG-001, buffer search only)
  -> CommandPalette::confirm -> CommandPaletteDB::write_command_invocation (BL146, DB write)
```

**Related files:** see `## Source Code References` above.

## DB Impact per Event

| Event/Endpoint | Table | Columns | Operation | Value Derivation | Source |
|----------------|-------|---------|-----------|-------------------|--------|
| Command palette confirm (Enter on a selected command) | `command_invocations` | command_name, user_query | INSERT | `command_name` from the invoked action's display name; `user_query` from the palette's current query editor text; `last_invoked` defaults to `unixepoch()` | `crates/command_palette/src/persistence.rs:118-123` |
| Command palette confirm (prune step, same transaction) | `command_invocations` | id | DELETE | Deletes the row(s) with the minimum `id` whenever the table's total row count exceeds 1,000 | `crates/command_palette/src/persistence.rs:121` |

Project search, buffer search, and the file finder itself perform no database writes — they only
read worktree snapshots and buffer content in memory (fuzzy-finder recent-history entries are
read from workspace navigation history, not written by this feature; see BL159).
