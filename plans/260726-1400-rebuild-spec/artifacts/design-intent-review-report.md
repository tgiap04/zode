---
passed: true
issues: 0
warnings: 2
---
<!--
`issues`: count of critical issues (0 = all pass).
`warnings`: count of warning issues.
`passed`: true iff issues === 0.
-->

# Review Report — design-intent.md (`--design-intent` pass, D.3, EXPERIMENTAL)

**Reviewer**: reviewer (automated)
**Date**: 2026-08-07
**Scope**: `plans/260726-1400-rebuild-spec/artifacts/design-intent.md` (1 artifact)
**Checklist sections applied**: Universal + DesignIntent (DI-S1..DI-S6) only.
**Upstream state**: D.2 `validate_design_intent_density.py` already PASSED with 0 issues
(paragraph-level citation-or-`[INFERRED]` density, disclaimer fence-skip,
`.design-intent.completed` marker presence). Those checks are marked `[deterministic-pass]`
below and were not re-run; this review focuses on semantic depth only.

---

## Summary

| Metric | Value |
|--------|-------|
| Artifacts reviewed | 1 (design-intent.md) |
| Cross-refs read in full | `docs/system/architecture.md`, `docs/system/business-rules.md`, `docs/generated/permissions-matrix.md` (excerpt), `crates/sidebar/src/rail.rs`, `crates/workspace/src/multi_workspace.rs`, `crates/extension/src/extension_manifest.rs`, `crates/extension_host/src/capability_granter.rs`, `crates/project/src/trusted_worktrees.rs`, `crates/project/src/lsp_store.rs`, `crates/project/src/git_store.rs` |
| Critical issues | 0 |
| Warnings | 2 |
| Result | **PASS** |

---

## Critical Issues

_(none)_

---

## Warnings

### W1: NFR1 quote cited to the wrong line range — OPEN
- **Severity**: warning
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/design-intent.md:81-83` (citing `crates/workspace/src/multi_workspace.rs:283-285`)
- **Description**: The claim quotes `"polling must not cost anything noticeable"` and cites `multi_workspace.rs:283-285`. That line range is actually the doc comment on `MEMORY_FUSE_POLL_INTERVAL` ("FR3: how often the memory-pressure fuse polls system memory. NFR1 requires this to be infrequent and off the foreground thread..."), which paraphrases the same NFR1 requirement but does not contain the literal quoted string. The literal phrase actually appears ~14 lines earlier, in `SysinfoMemoryPressureReader::available_memory_percent`'s comment (lines 269-270: `// matches system_specs.rs's existing convention and NFR1's "polling must not cost anything noticeable"`). The underlying claim (NFR1 exists and drives this design) is real and verifiable in the same file — this is a citation-precision defect, not a fabrication, so it does not meet the DI-S1 "citation does not support its claim" critical bar, but it should be tightened.
- **Fix**: Change the citation to `crates/workspace/src/multi_workspace.rs:269-270`, or quote the FR3/NFR1 doc comment actually at 280-283 instead of the SysinfoMemoryPressureReader inline comment.

### W2: Worktree-trust section is quote-heavy with a thin added "why" — OPEN
- **Severity**: warning
- **Location**: `plans/260726-1400-rebuild-spec/artifacts/design-intent.md:150-162`
- **Description**: DI-S4 requires added "why," not restatement. The `business-rules.md` § Workspace/Worktree Trust quote reproduced here runs to nearly the full source paragraph (the untrusted-by-default behavior, the hierarchical grant rule, and the persistence-across-restarts rule), and the section's own added rationale is a single sentence ("a single explicit trade-off... minimize prompt fatigue... while still gating the two riskiest operation classes"). Compare to the neighboring hibernation and GPUI-concurrency sections, which each add several sentences of genuine trade-off analysis beyond their source quote. This section stays just on the right side of DI-S4 (there is a real, non-generic "why" — prompt-fatigue vs. risk-gating — that is not stated verbatim in business-rules.md), so it is not a DRY violation, but the why-to-quote ratio is the thinnest of the six Architecture Choices entries and is worth tightening on a future pass.
- **Fix**: Trim the quoted span to the sentence that actually motivates the hierarchical/persistent design (drop the restated "trust must be granted through a prompt" and "global opt-out" mechanics, which are pure business-rules.md restatement and add no rationale), and expand the trade-off sentence with what a per-action model would have cost (repeated prompts on every LSP spawn / git op).

---

## Passed Checks

✓ Universal.artifact_exists_non_empty @ design-intent.md
✓ Universal.no_placeholder_text @ design-intent.md
✓ Universal.required_sections_present_in_order @ design-intent.md
✓ Universal.every_issue_has_location @ design-intent-review-report.md
✓ Universal.paragraph_citation_or_inferred_density @ design-intent.md [deterministic-pass]
✓ Universal.disclaimer_fence_skip @ design-intent.md [deterministic-pass]
✓ Universal.design-intent-completed-marker-present @ design-intent.md [deterministic-pass]
✓ DI-S1 @ design-intent.md (SSH-remoting claim vs architecture.md § System Architecture — verbatim quote match)
✓ DI-S1 @ design-intent.md (hibernation claim vs business-rules.md § Hibernation Lifecycle — verbatim quote match)
✓ DI-S1 @ design-intent.md (extension-capability claim vs business-rules.md § Extension System — verbatim quote match)
✓ DI-S1 @ design-intent.md (GPUI single-writer claim vs architecture.md § Concurrency & Event Model — verbatim quote match)
✓ DI-S1 @ design-intent.md (rail.rs:71-74, rail.rs:10 RAIL_WIDTH — confirmed accurate against source)
✓ DI-S1 @ design-intent.md (multi_workspace.rs:239-247, :304, :308-311, :352-364 — confirmed accurate against source)
✓ DI-S1 @ design-intent.md (extension_manifest.rs:168, capability_granter.rs grant_exec, trusted_worktrees.rs can_trust, lsp_store.rs:449, git_store.rs:1595, permissions-matrix.md:44-46 — all confirmed accurate against source)
✓ DI-S1 @ design-intent.md (WeakEntity<> 594-occurrences-across-158-files claim — independently reproduced via rg, exact match)
✓ DI-S2 @ design-intent.md (all 3 in-body [INFERRED] tags + 3 Appendix entries carry one-clause reasoning, none bare)
✓ DI-S3 @ design-intent.md (zero ADRs exist in repo; no business-rules.md/architecture.md content silently contradicted)
✓ DI-S5 @ design-intent.md (disclaimer banner present, substance unmodified, `<!-- disclaimer:start -->`/`<!-- disclaimer:end -->` markers intact)
✓ DI-S6 @ design-intent.md (Open Questions section's zero-signal notes on WASM-choice and sqlez-vs-alternatives are honest, specific, not generic filler)
✓ DesignIntent.report-only-status-not-flagged @ design-intent.md (F11b: correctly not promoted to docs/system/, per checklist's explicit non-defect guidance)
✓ DesignIntent.confidence-report-sidecar-absence-not-flagged @ design-intent.md (pre-promotion absence is non-defect, per checklist)

---

## Edge Cases Turned Up

- The single soft DRY case (W2) sits right at the DI-S4 boundary — worth watching on future design-intent regenerations for this repo, since business-rules.md's prose is dense enough to tempt heavier quoting than "why" analysis in sections where the source paragraph and the rationale happen to be adjacent.
- The extension-capability section correctly threads a genuinely unresolved upstream question (`technical-spec.md:424`'s own flagged gap) through to `## Open Questions` rather than resolving it — this is the DI-S3/DI-S6 discipline working as intended, not a defect.

## Done Well

- Citation density is real, not decorative: every spot-checked `business-rules.md`/`architecture.md` quote was verbatim-accurate, and every spot-checked `file:line` citation (bar W1) pointed at the exact code it claimed to.
- The WeakEntity occurrence count (594/158) was independently reproduced exactly via `rg` — this is the kind of falsifiable, checkable claim that should be the house standard for this artifact type.
- `[INFERRED]` is used narrowly and honestly: the artifact resists the temptation to editorialize on the collab/AI/auth removal's business motive, and instead states plainly that no doc or commit gives that reason.
- The Handoff Summary's stated 3/13 INFERRED ratio (~23%) and 0/13 ADR-citation ratio are consistent with what a direct count of the document's own claims produces — the self-reported metrics are not inflated or rounded favorably.

## Actions In Order

1. Fix W1 — retarget the NFR1 citation to `multi_workspace.rs:269-270` (or requote the 280-283 span it's actually pointing at).
2. Consider W2 — tighten the worktree-trust section's quote-to-rationale ratio on the next regeneration; not blocking.

## Numbers

- Citations spot-checked: 9 (4 doc-section quotes, 5 file:line/code citations), 8/9 fully accurate, 1/9 accurate-in-substance but mis-lined (W1).
- `[INFERRED]` tags checked: 6 (3 inline + 3 Appendix), 6/6 carry real reasoning.
- Critical findings: 0.
- Warning findings: 2.

## Still Unresolved

None — both warnings are minor precision/polish items and do not block this artifact.
