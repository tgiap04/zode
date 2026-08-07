<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths — all references here are output targets or internal definitions -->
# Test Cases — F011_GitIntegration

**SIDECAR (v26.1.0):** this is a 5th, optional file alongside the 4 mandatory feature-spec files.
Its absence never blocks feature-spec promotion.

**Code Format**: `TC###` — 3-digit zero-padded, resets per feature (this file's own scope is the
reset boundary; unlike `JOB###`, which is file-global).

**Citation-source split**: `UT`/`IT` rows cite a `BR-###`/`SM-###`/`DEC-###`/`DISC-###` code, a
`file:line`, or an `edge-cases.md` row. `UAT` rows cite a `screens.md`/`business-context.md`
section — never a bare code.

**CSV export**: out of scope v1 — Markdown is the sole output.

---

## Test Cases

| Test-ID | Type (UT\|IT\|UAT) | Given | When | Then | Traces-to |
|---------|---------------------|-------|------|------|-----------|
| TC001 | UT | No entries staged and no tracked files changed | Developer confirms commit (not amending) | Commit is refused with "No changes to commit"; nothing is written | `BR-001` |
| TC002 | IT | Nothing staged, but tracked files have uncommitted edits | Developer confirms commit (not amending) | All non-created tracked files are auto-staged, then the commit proceeds | `BR-001` |
| TC003 | UT | Repository has unresolved (unstaged) merge conflicts | Developer confirms commit | Commit is blocked with "There are still conflicts. You must stage these before committing" | `BR-002` |
| TC004 | IT | No HEAD commit exists yet in the repository | Developer invokes Amend | Amend gesture does not start (no commit-message load, no git call) | `BR-003` |
| TC005 | IT | HEAD commit exists; amend not yet pending | Developer invokes Amend for the first time | Last commit message is loaded into the commit editor; no git write occurs yet (`idle → amend_pending`) | `SM-001` |
| TC006 | IT | Amend is pending (message already loaded) and the commit editor is still focused | Developer invokes Amend a second time | `git commit --amend` runs and `amend_pending` resets to idle (`amend_pending → idle`) | `SM-001` |
| TC007 | IT | A stash entry's SHA no longer matches its expected index (list changed underneath the user) | Developer applies, pops, or drops that stash entry from Commit View | Operation is aborted with an explicit error instead of acting on the wrong stash | `BR-004` |
| TC008 | IT | Working tree has uncommitted changes that conflict with the target branch | Developer selects a different branch in the Branch Picker | `git checkout` itself refuses the switch; error is surfaced via a "Failed to change branch" dialog | `BR-005` |
| TC009 | UT | A tracked file has unstaged edits (staged or not) | Developer triggers Discard on that entry | Entry is unstaged first if staged, then reverted via `checkout_files("HEAD", ...)` | `BR-006` |
| TC010 | UT | A newly-created (untracked) file has uncommitted content | Developer triggers Discard on that entry | A "Trash {filename}?" confirmation prompt appears; on confirm, the file is deleted via `project.delete_file`, never checked out | `BR-006` |
| TC011 | IT | Any git CLI/libgit2 call (stage, unstage, stash, commit, checkout, push) is in flight | The underlying process exits with a non-zero status | Failure is captured with stderr and shown as a dialog; no partial state change is applied and no automatic retry happens | `edge-cases.md § Any underlying git process ... exits with a non-zero status` |
| TC012 | UAT | Developer has a repository open with changed files | Developer opens the Git Panel | Changed files are shown split into staged and unstaged sections | `screens.md § User Journey step 1` |
| TC013 | UAT | A file has 2 unstaged hunks visible in the Git Panel | Developer stages one hunk | That hunk moves into the staged section; the other unstaged hunk in the same file remains untouched | `screens.md § User Journey step 2` |
| TC014 | UAT | Staged changes exist in the Git Panel | Developer writes a commit message in the Git Panel and confirms | The staged changes become a new commit and the staged list clears | `screens.md § User Journey step 3` |
| TC015 | UAT | Working tree is clean and a different branch exists | Developer opens the Branch Picker and selects that branch | The Git Panel and status bar update to reflect the newly checked-out branch | `screens.md § User Journey step 4` |
| TC016 | UAT | Multiple files across the project have uncommitted changes | Developer opens Project Diff before a larger commit | Every changed file's diff is listed together in one combined multibuffer view | `screens.md § User Journey step 5` |
| TC017 | UAT | Repository has recent branches and merges in its history | Developer opens Git Graph | Recent branches and merges are rendered as a visual commit graph | `screens.md § User Journey step 5` |
| TC018 | UAT | Developer needs to switch context without losing in-progress work | Developer stashes the uncommitted changes | The project is set aside cleanly (working tree returns to match HEAD) until the developer comes back to it | `business-context.md § What They Do step 3` |
| TC019 | UAT | An edit turns out to be unwanted | Developer discards it | The file returns to its last-saved state, or is removed entirely if it was never saved before | `business-context.md § What They Do step 4` |

---

## Coverage Notes

All `BR-###` codes (BR-001 through BR-006) and both `SM-001` transitions have a
tracing test case above — no deliberate omissions. `DEC-###`/`DISC-###` are N/A for this feature
per `technical-spec.md`'s Decision Logic and Polymorphic Behavior sections, so none exist to
cover. `INT-001_GitCliAndLibgit2DualPath` has no dedicated `BR-###`/`SM-###` code of its own; its
only test-case-shaped source is the `edge-cases.md` non-zero-exit row, covered by TC011.
