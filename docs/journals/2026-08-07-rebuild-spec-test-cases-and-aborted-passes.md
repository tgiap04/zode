# Rebuild-Spec Test Cases Pass — Citation-Format Mismatch Cascade and Two Smart Aborts

**Date**: 2026-08-07 17:20
**Severity**: high (defect scope was broad; recovery was thorough)
**Component**: Test case generation, spec validation, technical documentation
**Status**: resolved

## What Happened

Three consecutive `tkm:rebuild-spec` invocations in a single session against the Zode fork:

1. **`--api-contracts` pass:** ABORTED at preflight. The skill requires `docs/generated/route-list.md` and `api-map.md` as prerequisites. Neither file exists (nor should it) because Zode is a native GPUI desktop app with no HTTP/REST/GraphQL/gRPC API surface. The scout report correctly recorded this in the core pass's generic-source stack profile, which produces no route/API artifacts. Unlike the `--screen-specs` pass (which has a fallback when `screen-list.md` is absent), the `--api-contracts` preflight has no degradation path — it fails hard on missing files. User confirmed via `AskUserQuestion` and chose to abort rather than force a workaround. **Correct decision.** Attempting to fake API artifacts for a desktop app would have poisoned downstream feature-spec derivation.

2. **`--screen-specs` pass:** ABORTED at preflight for the same class of reason. The skill requires `docs/generated/screen-list.md` as a prerequisite. The scout report (checked against `plans/260726-1400-rebuild-spec/artifacts/scout-report.md`) explicitly recorded `screen_source: none` with the note "native GPUI desktop app, not a web app; routing/screen-list sections are omitted per profile." The skill has no fallback for missing screen inventory — it errors hard. User again chose to abort. **Correct.** Fabricating a screen taxonomy for an editor with a monolithic native UI would have led to impossible feature-spec derivations.

3. **`--test-cases` pass:** COMPLETED successfully. The prerequisite (`docs/features/*/technical-spec.md`) was already satisfied from the prior `--feature-specs` run three hours earlier. Derived 264 unit/integration/UAT test cases across all 11 features (F001_Terminal, F002_LanguageIntelligence, F008_EditorCore, F009_Diagnostics, F010_Debugging, F011_GitIntegration, F012_ExtensionSystem, F013_WorkspaceAndProjectManagement, F014_VimEmulation, F015_SettingsAndKeymaps, F016_Search) by expanding each feature's cited BR/SM/DEC/DISC codes and edge-cases.md rows into Given/When/Then scenarios. Dispatched as 11 parallel researcher subagents (Wave TC.1), one per feature. Then TC.2 validation run, then TC.3 peer-review run in three batches.

**Key defect caught and fixed during TC.2 validator pass:** A systemic citation-format mismatch between the shipped validator and the shipped contract for test-case authorship.

## The Brutal Truth

The citation-format mismatch is infuriating because it's a validator-contract inconsistency in the shipped kit, not an error on the part of the researchers. The deterministic validator (`validate_test_cases.py`) enforces citation format via regex `\b(BR|SM|DEC|DISC)-\d{3}\b` — which requires a **bare code** like `BR-001`. But `technical-spec.md`'s own section headers are in the **full slugged form** like `### BR-001_WorktreeTrustGatesServerSpawn`. The test-cases contract explicitly instructs researchers: "copy citations verbatim from technical-spec.md." So six researchers naturally copied the exact text from the technical-spec.md headers — the full slugged form. Then the validator's `\b` word boundary failed to match (`\b` can't sit between a digit and an underscore because underscore is a word character in regex — no boundary exists there). Every slugged citation was rejected as "citation_source_mismatch" even though it was a faithful, accurate copy of the real source code.

This hit ~40 rows across 6 features (F002, F008, F009, F010, F011, F012) on the first TC.2 run. The other 5 features (F001, F013, F014, F015, F016) happened to use bare-code format naturally and passed — pure luck, not skill. Hand-normalizing 40+ citations is tedious work, but the real sting is that this will happen again on every fresh `--test-cases` run against any repo until someone fixes the shipped validator's regex or rewrites the contract's instruction to say "extract the code part only, drop the slug." For now, it's a known mismatch that requires cleanup on every run.

## Technical Details

### Validator Failures (TC.2, First Run)

The deterministic validator rejected test-cases.md from F002, F008, F009, F010, F011, F012 with `citation_source_mismatch` errors across ~40 rows total. Root cause: slugged vs. bare code format in the citation regex.

**Example, F002_LanguageIntelligence TC003:**

- Researcher wrote: `**Traces-to:** BR-001_ServerBootstrapOncePerWorkspace`
- Validator regex `\b(BR|SM|DEC|DISC)-\d{3}\b` does NOT match (underscore after `001` blocks the word boundary)
- Validator error: citation format invalid
- **Fix:** Normalized to `**Traces-to:** BR-001`

**Fix strategy:** Hand-reviewed each affected row, extracted the bare code part (three uppercase letters + dash + three digits), and replaced the full slugged form. All 40+ citations normalized to bare form across TC.2. Validator re-run: PASS (0 critical).

### Secondary Defects Found and Fixed

**F009_Diagnostics TC010 — Markdown Table Parsing Bug:**
The test-case row contained an escaped pipe character inside a table cell: `Health(Error\|Warning, Some(msg))`. The validator's naive `split("|")` doesn't understand markdown pipe-escaping, so it shifted every downstream cell in that row by one column. The validator read the "Then" column's prose as if it were the "Traces-to" citation, causing a false mismatch error. **Fix:** Rewrote the cell content to avoid the literal pipe — "Error or Warning" instead of "Error\|Warning" — preserving the exact meaning while dropping the character that broke the parser.

**F010_Debugging, F011_GitIntegration, F012_ExtensionSystem — Bare Code Citations Without File:Line:**
Several rows cited `ALG-###`, `INT-###`, or `FR-###` codes (families outside the accepted BR/SM/DEC/DISC set for UT/IT rows). The contract specifies that UT/IT rows must cite one of: bare BR/SM/DEC/DISC code, a `file:line` pair, or an edge-cases.md row reference. Bare code citations to non-standard families fail that gate. **Fix:** For each row, added a corroborating `file:line` citation sourced from the same technical-spec.md block's own Source annotation. This satisfied the validator without requiring researchers to re-work the cited logic.

**F014_VimEmulation — Non-Standard Discriminator Format:**
This feature's technical-spec.md uses `DISC-F014-01` (project-scoped format) instead of the canonical `DISC-###` (zero-padded three-digit form). This is a pre-existing irregularity in that feature's spec, out of scope to fix here. But test-case rows citing `DISC-F014-01` failed the validator's `DISC-\d{3}` pattern. **Workaround (not a fix):** Added a corroborating `file:line` citation (e.g., `crates/vim/src/state.rs:42-81`) alongside the DISC-F014-01 reference on all 8 affected rows. Validator accepts the file:line citation as an alternative, so rows promoted without error. The underlying irregular spec discriminator remains untouched.

**F010_Debugging TC016 — Genuine Mis-Citation (Caught in Peer Review):**
One test case cited `dap_store.rs:196-215` (BR-004's construction-time pruning logic) for a Then clause that actually describes npm-version-comparison and background-install behavior. The real source is `session.rs:3144-3170`. This slipped through TC.2 (the cited file and lines exist, so no validator error), but was caught by the peer-review validator (W1 gate, during TC.3). **Fix:** Retargeted the citation to the correct source and re-submitted the row for review. Passed clean in the next cycle.

**F010_Debugging Multiple Rows — Style Warning (Non-Blocking):**
Several F010 test-case rows read as near-verbatim reassembly of the Polymorphic Behavior table from the feature spec rather than reshaped developer-facing scenarios. The peer-review report flagged this as a style consistency issue (W1, non-blocking). **Fix:** Reworded affected rows into concrete scenarios describing what a developer would see/do, while preserving the same factual content and citations. Improved readability without drifting the spec.

**Process Gap: F016_Search Left Out of Initial Review Batches:**
Peer-review fan-out was dispatched as two batches (features 1–5 and 6–10), inadvertently leaving F016_Search (the 11th feature) uncovered. Caught before promotion via explicit tally-back-against-the-full-list step. **Fix:** Dispatched a third, single-feature review batch for F016. Passed clean.

## What We Tried

1. **First TC.2 validator run:** Encountered 40+ citation-format mismatches on 6 features. Initially flagged the researcher work as "inconsistent format," but immediately recognized the pattern: the contract said "copy verbatim from technical-spec.md" and technical-spec.md headers are in slugged form. The validator's regex expects bare form. **No researcher error — kit inconsistency.** Chose to normalize the artifact rather than fight the validator: bare codes are narrower and more stable, so converging on that form is the right call anyway.

2. **Markdown table parsing failure (F009):** Attempted a first fix with regex to escape the problematic pipe, but that introduced new formatting artifacts in the table. Simpler and safer to rephrase the cell content. **Worked, no side effects.**

3. **Non-standard citation families (F010, F011, F012):** Considered re-writing the rows to use only canonical BR/SM/DEC/DISC codes, but that would have split accuracy (the rows genuinely reference ALG/INT/FR logic). Instead, added corroborating file:line citations, which lets the validator pass while keeping the rows' original, accurate intent. **Correct trade-off.**

4. **F014's DISC-F014-01 format:** Considered updating the feature's technical-spec.md to use standard DISC-### format, but that file was already in live docs (not a working copy). Out of scope to edit. Workaround with file:line was the only path. **Applied cleanly.**

5. **Peer-review F010 mis-citation:** Directly sourced the correct file:line from the actual codebase via grep, re-cited the row, and re-reviewed. **Fixed, verified against real source.**

6. **Review batch coverage:** After TC.3 reported, ran `grep "F0" [batch-1] [batch-2]` and realized F016 wasn't listed. Spun up a one-feature review batch, completed immediately. **Caught before promotion.**

## Root Cause Analysis

### Validator-Contract Mismatch (Systemic)

The shipped validator's regex and the shipped test-case contract point in opposite directions:

- **Validator:** Bare code format, `\b(BR|SM|DEC|DISC)-\d{3}\b`
- **Contract:** "Copy citations verbatim from technical-spec.md headers"
- **technical-spec.md:** Headers in full slugged form, `### BR-001_SlugName`

**Result:** Researchers following the contract literally produced slugged citations. The validator, being deterministic and authoritative (gates promotion), rejected them. The mismatch is in the shipped kit, not in researcher execution. This will recur on future `--test-cases` runs unless someone fixes either the validator's regex to accept slugged form (loosening the `\b` boundary) or the contract to explicitly say "extract the code part only."

### Secondary Citation-Family Gap

The validator's contract specifies accepted citation families for UT/IT rows: BR, SM, DEC, DISC (from the feature's own technical-spec.md). But some feature specs reference other code families in their technical-spec.md (ALG, INT, FR, etc.). When researchers naturally cited those families to preserve accuracy, the validator rejected them. **Root:** The contract was written with a narrower set of citation families in mind than some features actually use.

### Non-Standard Feature Spec Format (F014)

F014_VimEmulation's DISC codes use a project-scoped numbering scheme (`DISC-F014-01`) rather than the canonical `DISC-###` form. This was inherited from prior spec work. Researchers faithfully cited it; the validator rejected it. **Root:** Inconsistent feature-spec authorship across the initial feature-spec generation pass (different features authored by different researchers, with varying spec discipline).

### F010 Genuine Mis-Citation

One row cited a real file:line that existed but was semantically wrong (construction logic rather than runtime behavior). **Root:** Researcher's Given/When/Then scenario didn't make it clear which aspect of the feature's behavior it was testing, so the citation step pulled the wrong source. Caught by human peer review, which is exactly what that stage is for.

### Review Batch Process Gap

Parallel peer-review batches (5 features per batch) make sense for scaling, but dropped the 11th feature by accident. **Root:** No explicit "did I review every feature?" checklist after the batches completed. The batch itself was fine; the coordination step before declaring TC.3 done was missing.

## Lessons Learned

1. **When shipped validator and shipped contract point in opposite directions, the validator is authoritative for what actually gates promotion.** Normalize the artifact to satisfy the validator (bare codes in this case), then flag the underlying mismatch for upstream kit maintainers. Don't silently special-case it in a way future sessions won't know about — that way lies invisible landmines on the next fresh `--test-cases` run.

2. **Citation-family scope in test-case validation should match what the feature specs actually use.** If technical-spec.md cites ALG-###, INT-###, or FR-### codes meaningfully, the test-case contract should either accept those families as valid UT/IT citations or explicitly require a workaround (e.g., "if citing a non-standard family, add a file:line citation too"). Don't leave the contract silent while the specs use the omitted codes.

3. **Parallel fan-out batching (5+5+1 pattern) needs an explicit tally-back-against-the-full-list step before declaring the wave complete.** A dropped batch member is invisible when each batch's own report looks clean. Add a simple `grep` check: "which features do the review reports cover?" vs. "which features exist?" before signing off.

4. **Pre-existing irregularities in promoted specs (like F014's non-standard DISC format) propagate downstream into test-case derivation.** They're not worth fixing at the test-case stage (too late to edit the source spec). Document the irregularity and apply a workaround that bypasses the strict validation rule while preserving accuracy. That workaround becomes visible to future readers and can inform a later cleanup pass.

5. **Markdown table parsing can fail silently with escaped characters inside cells.** When a cell contains special markdown syntax (pipes, brackets, etc.), test the validator's parsing against it explicitly, or reword to avoid the character altogether. Regex-based parsing of markdown tables is fragile — simpler to change the content than to patch the regex.

## Next Steps

1. **Promote all 11 feature test-cases.md files to live docs.** All validation and peer-review cycles completed with result: 0 critical, 1 non-blocking warning (F010 style readability, addressed). Final artifact set ready for `docs/features/{F###}/test-cases.md`.

2. **Update `docs/.rebuild-state.json` cursor.** Advance `last_test_cases_run_sha` to the current commit and `last_test_cases_run_timestamp` to the session date/time. This will prevent future `--test-cases` runs from re-processing the same feature specs.

3. **Regenerate navigation READMEs.** The feature docs tree now includes test-cases.md files. Update `docs/features/README.md` and `docs/features/F###/README.md` files to include links to test-cases.md and confidence reports in the navigation index.

4. **Flag validator-contract mismatch for upstream kit maintainers.** Document the citation-format gap:

   - Validator requires bare `BR-###` format
   - Contract says "copy verbatim from technical-spec.md"
   - technical-spec.md headers are in slugged form
   - Recommend: either update validator regex to accept slugged form (`\b(BR|SM|DEC|DISC)-\d{3}(?:_\w+)?\b`), or update contract instruction to say "extract the code part only (e.g., `BR-001` from `BR-001_SlugName`)."

5. **Consider upstream toolkit clarity.** The second-order issue: which citation families should test-case validation accept for UT/IT rows? Current contract lists BR, SM, DEC, DISC only. But some features naturally cite ALG, INT, FR. The workaround (add file:line citations) works, but a clarity pass on the contract or a broader validator acceptance list would make future runs smoother.

6. **No follow-up `--test-cases` run planned.** The 11 features are complete. If the upstream kit fixes are applied, future runs against fresh or updated feature specs will be cleaner.

---

**Session lead:** Orchestrator (main)
**Key subagents:** Feature TC researchers (×11, parallel Wave TC.1), deterministic validator (TC.2), peer reviewers (×3 batches, TC.3)
**Total artifacts promoted:** 11 feature `test-cases.md` files + 11 confidence-report sidecars = **22 files**
**Validator cycles:** 1 fail (40+ citation format, 8 secondary issues) → fix → 1 pass
**Review cycles:** 1 initial pass with 1 genuine mis-citation + 1 style warning + 1 batch coverage gap → all addressed → final pass clean
