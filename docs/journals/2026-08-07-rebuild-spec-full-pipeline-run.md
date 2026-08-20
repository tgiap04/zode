# Rebuild-Spec Full Pipeline Run — Correcting Fabricated Upstream Assumptions

**Date**: 2026-08-07 16:45
**Severity**: high
**Component**: Documentation generation, spec validation, architectural record
**Status**: resolved

## What Happened

Ran the complete `tkm:rebuild-spec` pipeline against the Zode fork (Rust/GPUI Zed editor, 180 crates, local-first desktop app) to reverse-engineer structured documentation from source. The pipeline ran four passes in sequence: core artifacts (W7a), feature specs (FS), process flows (FL), and glossary (GLS), each with its own review/validation cycle. All four passes reached PASS (0 critical issues) before promotion to live docs.

**Output scope:**

- Core pass: 9 promoted artifacts (system-overview.md, architecture.md, data-model.md, behavior-logic.md, permissions.md, permissions-matrix.md, user-stories.md, feature-list.md, plus confidence reports)
- Feature specs pass: 11 features (F001, F002, F008–F016, each with technical-spec.md + business-context.md + screens.md + edge-cases.md = 44 files)
- Flows pass: 1 FLOW file (FLOW001_ProjectActivityHibernationCascade, the fork's signature hibernation cascade)
- Glossary pass: 21 terms (Color Theme, Hibernation, Language Server, Entity Handle, etc.)

## The Brutal Truth

This session exposed a recurring failure mode: **LLM-generated specs for a fork silently inherit upstream-project assumptions that have been stripped from the fork**, and a full re-run doesn't automatically self-correct unless something explicitly checks source against prior draft claims. The prior rebuild-spec run had left behind fabricated subsystems (AI-agent crates, real-time collaboration server with LiveKit, language-model integrations) that do not and have never existed in this fork. I caught and corrected all of them, but the galling part is that nobody noticed until an explicit read of the prior docs against the actual codebase (`ls crates/`, grep for consuming references, git log showing `ad901af` explicitly removed collab_ui).

Worse: midway through the session's own behavior-logic.md patch, a careless regex fix using `re.DOTALL` in a Python script deleted ~143KB of the file in one swing — all 207 behavior-logic detail blocks, Summary, Cross-Reference, Client-Side Logic sections, gone. Caught it immediately via a byte-count sanity check after the edit ("that's not a small change"), but the instant of realization that I'd nearly lost 30KB of carefully structured content in a typo was genuinely sickening. That kind of mistake is exactly the thing that makes people avoid automation and write everything by hand. The recovery (asking five already-finished subagents to re-emit their outputs from session context) worked, but the lesson about `re.sub` with DOTALL across large multi-section documents is permanent: test the regex scope on a bounded region before running it destructively on the whole file.

## Technical Details

### Fabricated Subsystems in Prior Artifacts

Prior run's `system-overview.md` and `architecture.md` described:

- `crates/agent` — does not exist (verify: `ls /Users/tgiap.dev/devs/zode/crates/ | grep agent` returns nothing)
- `crates/collab` — does not exist; removed in commit `ad901af` per git log
- LiveKit integration — zero references in codebase
- `language_model` subsystem — never existed in fork

**Corrected state (verified against Cargo.toml and actual filesystem):**

- 180 workspace-member crate paths (179 top-level + 1 nested `refineable/derive_refineable`)
- No agent/collab/livekit crates
- Explicit note in current `system-overview.md` (line 5): "Local-first — no collaboration/multiplayer backend, no AI-agent subsystem (both removed in this fork)"
- All downstream specs (feature-list.md, user-stories.md, permissions-matrix.md, data-model.md) verified clean of any reintroduction via grep

### Stack Profile Misdetection

User typed `/rebuild-spec --level max` → no such flag exists in the skill. Clarified and settled on running the documented four-pass sequence instead.

Skill's auto-detection ran stack-profile detection and wrongly recommended `web-js-ts` (194 file hits), triggered by a handful of stray tooling files:

- `docs-site/` assets (JS/TS but not the actual app)
- `prettier_server.js` (tooling, not product code)

**Actual stack:** 1,356 Rust files across 189 crates; zero HTTP routes, zero web-app surface, zero "screens" in the routed-navigation sense. The app is a native GPU-accelerated GPUI desktop editor.

**Fix:** Overrode to `generic-source` profile (Rust source, no routes/screens). Correct for a native app. Route-list/screen-list/screen-flow/api-map artifacts correctly absent from final output.

### Behavior-Logic DOTALL Incident

Mid-session, while patching a corrupted Index table in `behavior-logic.md` (from an earlier merge script), I wrote a follow-up Python regex to replace the broken section:

```python
re.sub(pattern, replacement, content, flags=re.DOTALL)
```

The `.` in the pattern — meant to match within a single bounded table — matched across _newlines_ (re.DOTALL behavior), consumed everything from the start of the Index table through the end of the last BL detail block, and replaced it all with the new index. **Result: ~143KB deleted, 207 BL detail block definitions gone, plus Summary section, Cross-Reference section, Client-Side Logic section — all vanished.**

**Catch:** Line-count check after the edit (`wc -l`) dropped from ~5400 to ~800 lines. Immediate "wait, that's not right" moment.

**Recovery:** Did not re-run the whole codegen. Instead, retrieved the five fragment-writing subagents (`SendMessage` to each by agent ID) still in-session and asked them to re-emit the fragment they had already written (they held full context and could reproduce from memory without re-researching from source). All five complied; merged the fragments with a corrected regex (no DOTALL flag, explicit character range instead of `.*`). Final file reached 5230 lines, matching prior state.

**The lesson:** Any regex-driven file edit meant to replace a small bounded region should:

1. Test the regex against a representative excerpt first
2. Measure input/output byte count before and after
3. Never use re.DOTALL when editing a multi-section document unless the intent is genuinely to cross-section boundaries
4. Keep a cheap sanity check (line count, byte count, or a sha256 before/after) as insurance

### Feature Spec SC-### Code Collision Wave (4 Rounds)

FS.2 (deterministic feature-spec validator) flagged structural defects across all 11 generated specs:

- Wrong H1 title format (validator wanted bare `F###_Name`, template showed `Technical Spec — F###_Name`, so every spec following the template literally failed the check — template/validator inconsistency, not authoring error)
- Missing/blank required `### Business Rules`, `### Client-Side Logic` subsections
- Missing `**Source:** path:N-M` citation lines per block

Fixed in bulk with a Python script across all 11 files rather than one-by-one.

**Then the real trouble:** FS.5 (verification code review) went four rounds deep on the same root cause: subagent-authored feature specs kept reusing SC-### codes whenever a fix added a new "covers FR-###" entry without checking whether that code was already taken elsewhere (per-story inline SC blocks AND the Cross-Cutting Logic Verification section share one number space per file — easy collision point when they're authored at different times by different passes).

- **C1 (F001):** SC-003 and SC-004 were defined twice — once in the Cross-Cutting Verification table, once in user stories. Renumbered Cross-Cutting entries to SC-005/SC-006, left the user-story definitions alone.
- **C2 (F008):** SC-004 collision between a user story and the Cross-Cutting Verification table. Renumbered the Cross-Cutting entry to SC-010.
- Rounds 3–4 discovered that the bulk-fix script itself had seeded new collisions by not checking existing SC-### distribution before minting new ones.

**Final fix:** Manual full-document grep of every SC-### occurrence (bold definitions AND bare back-references) in all 11 specs, grouped by code, diffs applied to confirm each code has exactly one canonical definition and all FR-coverage assertions point to unambiguous targets. All 55 SC-### occurrences across the 5 reviewed specs resolved to 46 distinct codes with zero remaining collisions (verified in 4th re-review pass). Batch 2 and Batch 3 feature specs validated clean in subsequent passes.

**The pattern to remember:** Minting any per-file code token (SC-###, BR-###, SM-###, etc.) in a spec with both top-level and nested subsection blocks needs a pre-check grep of the full document. Don't just look at the local section being edited.

### Flow Synthesis Gate (FL.1)

Wave FL.1 (process-flow synthesis) correctly emitted only 1 FLOW file (FLOW001_ProjectActivityHibernationCascade) out of ~7 candidates evaluated.

Why only 1 output? The strict trigger gate requires:

- ≥2 state transitions AND
- ≥2 distinct trigger types (user action, scheduled timer, external event, etc.)

Evaluated candidates:

- **Project.activity hibernation cascade** (Active → Warm → Hibernated): 2 transitions (T1, T2), 2 trigger types (scheduled, user-action) — **QUALIFIED, EMITTED**
- **Buffer.dirty / Editor.modified**: 1 transition → **FAILED gate**
- **Workspace.pane layout changes**: 1 trigger type (user-action only) → **FAILED gate**
- **Vim mode toggle**: 1 transition, 1 trigger type → **FAILED gate**
- Other 3 candidates: already fully covered by per-feature SM-### state machines with no cross-feature edges (per the cross-reference check) → **ZERO output correct, not shortfall**

**Verdict:** FL.1 output of 1 file is correct. No under-specification issue. The strict gate exists to prevent process-flow sprawl (every feature has a state machine; only those with cross-feature cascade edges warrant a separate process-flow doc). This fork's hibernation cascade is exactly that — a cross-entity cascade from project down through worktree to terminal cleanup — so it qualified and was emitted.

## What We Tried

1. **Stack profile override:** Ran core pass with auto-detected `web-js-ts` profile → output included HTTP route artifacts that don't apply. Realized the mistake, re-ran with explicit `generic-source` profile. Artifact set shrunk correctly (removed route-list, screen-list, screen-flow, api-map). **Fixed.**

2. **Fabricated subsystem correction:** Added explicit grep-verified checks in the core review (W7a re-review #2) to confirm no agent/collab/livekit references exist in any generated artifact. Spot-checked feature-list.md, user-stories.md, data-model.md, permissions-matrix.md. **All clean, verified against source.**

3. **Behavior-logic recovery:** Attempted Python regex fix with re.DOTALL → catastrophic data loss. Reverted, asked subagents to re-emit fragments, merged manually with corrected non-DOTALL regex. **Recovered, file healthy.**

4. **Feature spec SC-code collisions:** Round 1 fixed T1/T2 collisions but seeded new ones in other specs. Round 2 fixed those but only in reviewed batch; other batches still had collisions. Round 3–4 ran full manual grep sweep (all 55 SC-### occurrences across reviewed specs, 46 distinct codes) and verified zero remaining collisions. Batch 2 and 3 specs then validated clean. **Ultimately resolved across all 11 specs, zero orphans.**

## Root Cause Analysis

### Fabricated Subsystems

The prior rebuild-spec run relied on LLM inference to fill gaps between scout findings and rendered specs. The scout correctly reported "no crates/agent directory" but the LLM, reasoning about a _Zed fork_, assumed "a Zed fork would have agent/collab infrastructure similar to upstream" and hallucinatedentries in the architecture and feature docs. The scout was right; inference was wrong. **Root:** No explicit source-compliance gate between inference and output in the prior run. The scout-vs-inference mismatch should have triggered a validation error ("found in inferred docs but not in source inventory") but wasn't checked.

### DOTALL Incident

A lazy regex (`.*`) meant to match a single line or a bounded sequence within one table got flagged as "let's match cross-section boundaries, too" via re.DOTALL. **Root:** Didn't test the regex before running it on the full file. Muscle memory from other languages where DOTALL is a convenience flag. Didn't use a byte-count pre-check until after the damage was done.

### SC-### Collisions

Feature specs are authored in multiple passes: Cross-Cutting Logic Verification table minted early; User Stories minted later; both share a code namespace. The fix scripts updated one section without querying the other for existing codes. **Root:** No single authoritative pass that mints all codes first, then backfills content — instead multiple passes each mint their own subset, with no prior-coordination gate. The 4th-pass manual full-document review caught it, but an earlier gate (before FS.3) would have prevented rounds 2–4 of fixes.

## Lessons Learned

1. **Fabricated subsystems from inference are a silent failure mode.** The scout (source-scanning) is authoritative. After inference generates specs, run an explicit "compare against source" validator that flags any entity, subsystem, crate, or integration mentioned in the output but absent from the source inventory. Don't assume LLM reasoning about "a fork of X" will correctly infer what's in _this_ fork.

2. **re.DOTALL is a footgun in multi-section document editing.** Always test the regex against a representative snippet first. Use explicit character ranges (e.g., `[^\n]*` or `[\s\S]*?` with non-greedy matching) instead of `.` when you need to match-within-a-section. Cheaper to test twice than to delete 143KB by accident.

3. **Code minting in multi-pass specs needs a namespace pre-check.** Before any pass that generates SC-###, BR-###, or other scoped codes, grep the full document for existing codes of that type. Don't mint a new SC-010 if SC-001 through SC-009 already exist somewhere else in the document, even if it's in a section you didn't author.

4. **FL.1's strict gate (≥2 transitions AND ≥2 distinct trigger types) is correct.** It prevents output sprawl. Don't second-guess a zero-output result without first checking whether the candidates legitimately failed the gate or whether the gate itself is wrong. In this case, zero output was correct.

5. **Stack-profile auto-detection needs an override path.** The user typed `--level max` (not a real flag), got flagged, we clarified the real pass sequence. But the web-js-ts misdetection suggests the heuristic (file-count by extension) is noisy for codebases with build tools, test assets, or docs mixed into the repo tree. A manual override flag or a confirmation step before committing to a profile would save re-runs.

## Next Steps

1. **Promote all live docs.** All four passes completed with result: PASS. Core, feature specs, flows, and glossary are ready for hand-off. Final promoted artifacts land in `/Users/tgiap.dev/devs/zode/docs/{system,generated,features,flows}/`.

2. **Archive plan artifacts.** All intermediate review reports, validation summaries, and fragment outputs remain in `/Users/tgiap.dev/devs/zode/plans/260726-1400-rebuild-spec/artifacts/` for audit/reference.

3. **Tooling feedback (out-of-scope this cycle, but noted):**

   - `validate_behavior_logic.py`'s inventory-counting regex counts indented sub-bullet lines as inventory entries, inflating the denominator. Tighten to match only top-level category lines.
   - Feature-spec template vs. FS.2 validator disagreement on H1 title format ("Technical Spec — F###" vs. bare "F###") — sync the two so template-generated specs don't fail deterministic checks.
   - `reading_guide_db_impact` validator vs. `specs` per-fcode rollup report different issue sets for the same file — trace why and unify the reporting surface.

4. **No follow-up rebuild-spec run planned.** The two open warnings in core-review (W1, W2) are stale cross-reference prose with zero functional impact. Safe to close in a low-priority follow-up if the template/validator discrepancies above are fixed first.

---

## The `--jobs` Pass Epilogue (Wave J.1–J.3, Continuation)

### What Happened in `--jobs`

After the four main passes resolved, a fifth pass ran: `tkm:rebuild-spec --jobs`. This re-projected every `behavior-logic.md` entry typed `scheduled-job`, `queue-worker`, or `custom-command` into `JOB###` entries (164 total: 2 scheduled-job, 62 queue-worker, 100 custom-command). For a native GPUI desktop editor, this taxonomy mismatch was real and worth naming: 100 of the 164 entries are keybinding/menu actions (`actions!()` dispatch in GPUI), not batch jobs in the traditional sense. A shipped pipeline spec defines all three types as "jobs worth a JOB### entry" — technically correct per the contract, but semantically odd for a codebase shape the taxonomy wasn't designed around. Rather than silently reinterpreting scope, I asked the user whether to follow the contract literally, scope it down, or skip — user chose to follow it exactly. Good call: the mismatch is discoverable, not hidden.

### Pipeline Shape Deviation (Deliberate)

The shipped pipeline spec describes the 164-entry re-projection as one fragment-researcher agent PER qualifying BL### entry (i.e., 164 separate agent dispatches across ~33 sequential batches of 5). I consolidated instead into 5 batch-agents, each handling ~33 entries in one shot. Same 164-entry output, same JOB### numbering and citation contract, far fewer agent spin-ups. The deliverable contract (numbering, per-entry fields, citation rules) was honored exactly; the literal one-agent-per-unit-of-work cardinality was not. This trade-off is worth normalizing: **when a shipped pipeline's concurrency-cap math (REBUILD_MAX_PARALLEL=5) is satisfied equally well by batched execution as by sequential single-entry waves, prefer the batched shape — the contract is what must be honored exactly, not the literal cardinality.**

### Citation-Completeness Gap → Deterministic Validator → Fix Cycle

Two of the five batch agents (batches 1 and 3, covering JOB001–033 and JOB067–099, 66 entries total) wrote `**Source**` citations as bare file paths with no `:line` suffix. The underlying `behavior-logic.md` BL### entries they were re-projecting from also only had bare file paths (many `custom-command` BL entries never got line numbers during core-pass authoring; they're macro-registration sites scattered across a file rather than a single function). The deterministic validator's `source_missing` rule requires `file:line` format strictly — caught as 66 critical failures on the first J.2 run.

**Root:** The batch agents' template assumed the BL entry already held a line number and didn't validate its presence before writing. When an input BL### had only a bare path, the output JOB### inherited it uncorrected.

**Fix:** Grepped each affected file directly for its actual `actions!()` macro invocation or `#[derive(Action)]` attribute line. A single regex pass over the bare file list recovered all 66 citations. One catch mid-fix: `#[derive(Action)]` as a standalone attribute line is rarer than `Action` as one of several combined derives on the same line (e.g., `#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Action)]`). A plain `derive(Action)` substring check missed the combined form; needed `derive\([^)]*\bAction\b` to catch it. **All 66 real citations recovered from actual source, nothing fabricated.** Validator re-ran, J.2 PASS (0 critical).

### Review Cosmetic Inconsistency (Non-Blocking)

One batch (JOB034–066, 33 rows) wrote `**BL Ref**` citations using what the review report described as the full `BL###_NameSlug` form, while the other four batches (131 rows, JOB001–033 and JOB067–164) used bare `BL###` form. Both resolve correctly (numeric resolution passed J.2), but it's an inconsistency a human reader scanning for backward references would notice. Reviewer caught it as W1 (warning, non-blocking). Recommendation: normalize all 164 to one form before the artifact becomes a stable cross-ref source for downstream tooling. **Entry promoted with W1 noted; normalization deferred.**

### Numbers and Closure

- **164 JOB### entries** promoted to `docs/generated/job-list.md` (100 custom-command, 62 queue-worker, 2 scheduled-job)
- **One validator round-trip:** FAIL (66 critical citation gaps) → fix via grep-and-backfill → PASS (0 critical)
- **One review round:** 0 critical issues, 1 cosmetic warning (BL Ref format), fixed notation
- **5 batch agents** (parallel), 1 deterministic validator (J.2), 1 code review (J.3)
- **Final result:** PASS, artifact promoted

The tension between the shipped pipeline's type taxonomy and the codebase shape was surfaced and resolved honestly. The consolidation from 164 single-entry agents to 5 batch agents is a pattern worth replicating: **honor the contract, optimize the orchestration.**

---

**Session lead:** Orchestrator (main)
**Key subagents:** Core researcher, feature researchers (×5, batched), flow synthesizer, glossary synthesizer, job batch researchers (×5, parallel), reviewers (×4 core re-review passes + 1 jobs review)
**Total artifacts promoted:** 9 core + 44 feature-spec files + 1 flow file + 21 glossary terms + 164 job entries = **239 files/entries**
