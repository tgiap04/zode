---
passed: true
issues: 0
warnings: 0
missing: 0
result: PASS
---
<!--
`passed`: true iff `issues === 0` (this is the TC.3 --test-cases semantic pass, not the FS.5 gate).
`issues`: count of critical findings.
`warnings`: count of warning findings.
`missing`: N/A for this sidecar pass — test-cases.md absence is never a MISSING verdict.
`result`: PASS iff `issues === 0`.
-->

# Review Report — TestCases Semantic Review (Wave TC.3, Batch 02)

**Reviewer**: Staff Engineer (automated)
**Date**: 2026-08-07
**Scope**: `test-cases.md` for F011_GitIntegration, F012_ExtensionSystem,
F013_WorkspaceAndProjectManagement, F014_VimEmulation, F015_SettingsAndKeymaps

---

## Summary

| Metric | Value |
|--------|-------|
| Files reviewed | 5 (test-cases.md sidecars) |
| Test-case rows reviewed | 165 (F011: 19, F012: 26, F013: 38, F014: 30, F015: 23) |
| Critical issues | 0 |
| Warnings | 0 |
| Result | **PASS** |

Deterministic checks (TC### regex/uniqueness, Type enum, Traces-to presence, citation-source-family
match, coverage-gap) already passed clean at TC.2 for all 5 files — marked `[deterministic-pass]`
below and not re-checked. This review applied TC-S1..TC-S6 semantic depth only.

---

## Critical Issues

_(none)_

---

## Warnings

_(none)_

---

## Passed Checks

✓ TC.deterministic (TC###/Type/Traces-to/citation-family/coverage-gap) @ F011_GitIntegration..F015_SettingsAndKeymaps (5/5) [deterministic-pass]
✓ TC-S1 (Given/When/Then matches cited BR/SM/DEC/DISC or file:line) @ F011_GitIntegration
✓ TC-S1 (Given/When/Then matches cited BR/SM/DEC/DISC or file:line) @ F012_ExtensionSystem
✓ TC-S1 (Given/When/Then matches cited BR/SM/DEC/DISC or file:line) @ F013_WorkspaceAndProjectManagement
✓ TC-S1 (Given/When/Then matches cited BR/SM/DEC/DISC or file:line) @ F014_VimEmulation
✓ TC-S1 (Given/When/Then matches cited BR/SM/DEC/DISC or file:line) @ F015_SettingsAndKeymaps
✓ TC-S2 (UAT rows cite a genuine screens.md/business-context.md section, not a laundered code) @ F011_GitIntegration
✓ TC-S2 (UAT rows cite a genuine screens.md/business-context.md section, not a laundered code) @ F012_ExtensionSystem
✓ TC-S2 (UAT rows cite a genuine screens.md/business-context.md section, not a laundered code) @ F013_WorkspaceAndProjectManagement
✓ TC-S2 (UAT rows cite a genuine screens.md/business-context.md section, not a laundered code) @ F014_VimEmulation
✓ TC-S2 (UAT rows cite a genuine screens.md/business-context.md section, not a laundered code) @ F015_SettingsAndKeymaps
✓ TC-S3 (no generic padding added to dodge a coverage-gap WARN) @ F011_GitIntegration..F015_SettingsAndKeymaps (5/5)
✓ TC-S4 (edge-cases.md reuse is faithful to the cited row's actual content) @ F011_GitIntegration
✓ TC-S4 (edge-cases.md reuse is faithful to the cited row's actual content) @ F012_ExtensionSystem
✓ TC-S4 (edge-cases.md reuse is faithful to the cited row's actual content) @ F013_WorkspaceAndProjectManagement
✓ TC-S4 (edge-cases.md reuse is faithful to the cited row's actual content) @ F014_VimEmulation
✓ TC-S4 (edge-cases.md reuse is faithful to the cited row's actual content) @ F015_SettingsAndKeymaps
✓ TC-S5 ([NO_TEST_CASE] Coverage Notes rationale is real, not a placeholder) @ F011_GitIntegration
✓ TC-S5 ([NO_TEST_CASE] Coverage Notes rationale is real, not a placeholder) @ F012_ExtensionSystem
✓ TC-S5 ([NO_TEST_CASE] Coverage Notes rationale is real, not a placeholder) @ F013_WorkspaceAndProjectManagement
✓ TC-S5 (no [NO_TEST_CASE] entries — Coverage Notes are N/A explanations, both real) @ F014_VimEmulation
✓ TC-S5 (no [NO_TEST_CASE] entries — Coverage Notes state full BR/DISC coverage, real) @ F015_SettingsAndKeymaps
✓ TC-S6 (Given/When/Then reshaped into concrete scenarios, not verbatim BR/SM/DEC/DISC prose) @ F011_GitIntegration..F015_SettingsAndKeymaps (5/5)
✓ Mid-session citation-format fixes (bare BR-### / added file:line for INT-only rows) verified correct @ F011_GitIntegration
✓ Mid-session citation-format fixes (bare BR-### / added file:line for INT-only rows) verified correct @ F012_ExtensionSystem
✓ Mid-session DISC-F014-01 corroborating file:line addition verified correct on all 8 rows @ F014_VimEmulation

---

## Depth Notes (evidence, not evidence-log padding)

- **F011_GitIntegration**: all 19 rows cross-checked against `technical-spec.md`'s BR-001..006,
  SM-001, INT-001 and against `edge-cases.md`/`screens.md`/`business-context.md`. TC004-TC006
  (amend two-stage gesture) precisely mirror `BR-003`'s pseudocode (first invoke loads message
  only, no git call; second invoke runs `commit --amend`). TC009/TC010 (`BR-006`) correctly
  distinguish the tracked-file `checkout_files` path from the untracked-file trash-confirm path.
  Coverage Notes' claim that `INT-001` has no dedicated BR/SM code of its own, covered instead via
  the edge-cases.md non-zero-exit row (TC011), checks out against the spec's own INT-001 section.
- **F012_ExtensionSystem**: TC001-TC003 (`BR-001`/`BR-002` dual-gate) match the pseudocode's two
  gates and "first statement, before any side effect" ordering exactly. TC004-TC006 (`DISC-013`
  wildcard predicates) match each capability struct's described match semantics. TC007-TC009
  (`BR-003`) correctly cover all three branches (uninstall-published-first, symlink-replace,
  real-dir-abort). TC017/TC018 (`file:line` citations for `INT-001`/`INT-002`, added mid-session)
  verified against `listener.rs:33-80` and `protocol.rs:118-124` as described in the tech-spec —
  the fix is accurate, not just mechanically well-formed.
- **F013_WorkspaceAndProjectManagement**: TC003-TC005 (`DISC-003`) reproduce the Active/Warm/
  Hibernated render/tooltip text verbatim-correctly from the table (not fabricated). TC030
  (edge-cases.md "label is a lifecycle intent, not a resource-state guarantee") matches the spec's
  own caveat about `LspStore::hibernate`'s detached task — consistent with the reviewer's own prior
  finding on this exact fork behavior. TC026/TC027 `file:line` citations verified against
  `multi_workspace.rs:1769-1850`/`:283-310` (fuse cap-at-one-victim, manual-wake immunity).
- **F014_VimEmulation**: all 8 `DISC-F014-01` rows (TC001-TC008) verified against the mode table —
  each Then outcome (e.g. TC003's `UndoReplace` restoring one char from `Vim.replacements`, TC006's
  block-mode multi-cursor mechanism) matches its table row precisely, not a paraphrase drift. The
  mid-session file:line addition (`crates/vim/src/state.rs:42-81`) is the exact source cited by the
  tech-spec's own DISC-F014-01 entry — a correct, non-fabricated corroboration, not just a
  well-formatted one. TC012/TC013 (`BR-004`) match the two distinct failure messages ("not set" vs.
  "contains empty anchors") precisely.
- **F015_SettingsAndKeymaps**: TC001-TC005 (`DISC-010`) match each `SettingsFile` layer's
  Render/Validation/Persistence cell exactly, including the `Server` layer's "not user-editable
  from this client" framing. TC012's `file:line` citation (`settings_store.rs:552-607`) matches
  `ALG-001`'s described one-at-a-time queue processing in full (load → mutate → canonicalize →
  atomic_write → set_user_settings → complete oneshot). TC011 (`BR-004` staff-flag precedence)
  correctly reflects that a per-flag override still wins over the staff branch.

## Edge Cases Turned Up

None beyond what the drafts already cite — no additional gaps found during the cross-reference
pass that the drafts' own Coverage Notes don't already address.

## Done Well

- All three mid-session hand-fixes (F011/F012 bare-code citations, F012 INT-only file:line
  additions, F014's DISC-F014-01 corroborating citations) were verified against source, not merely
  taken on faith — all three are substantively correct, not just correctly formatted.
- Coverage Notes across all 5 files give real, specific rationale for every omission (contract
  scope, DRY, indirect coverage) rather than placeholder text.
- No citation-source laundering found anywhere — every UAT row cites a genuine screens.md/
  business-context.md section; every UT/IT row cites a genuine BR/SM/DEC/DISC code, file:line, or
  edge-cases.md row.
- State-machine and dual-gate/precedence rows (F013 SM-001/SM-002, F014 SM-001, F015 BR-001/BR-004)
  consistently reproduce guard conditions and side effects with the same precision as the source
  tech-specs, not simplified/embellished versions of them.

## Actions In Order

None required — no critical or warning findings.

## Numbers

- Files reviewed: 5
- Rows reviewed: 165
- Deterministic-pass rules skipped (already TC.2-clean): TC###/Type/Traces-to/citation-family/coverage-gap × 5 files
- Semantic rules applied: TC-S1..TC-S6 × 5 files = 30 rule/fcode checks, all passed

## Still Unresolved

None.
