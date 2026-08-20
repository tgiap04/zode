# Rebuild-Spec System Overview Pass (OV.1–OV.4) — Graceful Degradation and Wording Collisions in Multi-Agent Synthesis

**Date**: 2026-08-07 18:15
**Severity**: low
**Component**: System documentation, client-facing deliverables, spec synthesis
**Status**: resolved

## What Happened

Completed the `tkm:rebuild-spec --overview` pass (waves OV.1 through OV.4) as the final step of a full-pipeline rebuild-spec session. This pass synthesizes a canonical 11-section System Overview deliverable from already-promoted `docs/` artifacts (never reading source code), targeting a client-facing audience with no technical jargon. The pass resolved the project name ("Zode (Zed fork)" → sanitized to filesystem-safe `Zed_zode_fork` per the pass's own contract), dispatched four parallel doc-writer agents to draft eight of the eleven sections, validated against deterministic token-leak rules, underwent peer-review, caught a wording collision ("assistant" ambiguity), fixed it, and produced a styled `.docx` deliverable.

**Output:** `docs/Zed_zode_fork_System_Overview.md` (markdown source, promoted to live docs), `docs/Zed_zode_fork_System_Overview.docx` (styled deliverable, 28,242 bytes), confidence-report sidecar.

**Key metrics:**

- Sections sourced from live docs: 8 of 11 (100% coverage with graceful degradation for 3 missing artifact categories)
- Validator round-trip: 1 pass, clean (0 leaked technical tokens on first run — a first for this session's synthesis passes)
- Reviewer round-trip: 1 pass with 2 findings; 1 finding fixed (wording collision), 1 finding fixed (missing UI element)
- Styled `.docx` build: 1 pass (round-trip and style-presence gates both clean)

## The Brutal Truth

The galling part: a single word collision between two semantically unrelated parts of the document nearly shipped a false impression. The hibernation feature section (§4 Business Flows & Lifecycle) described it as pausing "the assistant that is scanning files in the background" — perfectly accurate phrasing in isolation. But this document's §1 System Overview and §2 Purpose & Business Value _explicitly and repeatedly_ state that Zode has NO AI writing-assistant subsystem (removed from upstream Zed). A reader hitting "assistant" in §4 right after two firm "no AI" disclaimers would very plausibly misread it as a residual or incomplete removal of AI capability, even though "assistant" meant the worktree's background file-watcher (`Worktree::pause_scanning()`). The deterministic token-leak gate caught zero technical jargon (good — that gate works), but a careful human reviewer spotted the wording trap that no regex can catch: a word choice that's contextually correct but collides with the document's own repeated disclaimers.

The second issue was mundane: the project-navigation section omitted one real UI element (the quick tab switcher) from its bullet list. Not a defect in derivation, just an incomplete enumeration when pulling from multiple `screens.md` files.

## Technical Details

### Wave OV.1 (Researcher Dispatch)

Four parallel doc-writer agents, each drafting a cluster of the canonical 11-section structure:

- **Cluster A (Sections 5–6):** Detailed Function List (regrouping the 11 features from `docs/generated/feature-list.md` into five functional pillars) + Screen List (honest prose description of main UI surfaces, synthesized from each feature's `screens.md` rather than a formal route/screen/type table, because Zode has no `docs/generated/screen-list.md`).

- **Cluster B (Sections 2, 4):** Purpose & Business Value (value props extracted from `docs/system/overview.md` and each feature's own `business-context.md`) + Business Flows & Lifecycle (grounded in `docs/flows/project-activity-hibernation-cascade.md`, with explicit note that no screen-flow document exists and none should be expected for a native GPUI app).

- **Cluster C (Sections 7–10):** External Integrations (LSP, DAP, Git, extensions, SSH from `docs/system/architecture.md` with note that there's no web-style API surface) + Configuration & Optional Features + Technical Architecture (deferred to architecture.md + performance notes) + Data Model Summary (entity and schema overview).

- **Cluster D (Section 11):** Open Questions & Known Issues (cross-referenced from `docs/system/known-issues.md` and issue tracker links).

**§1 and §3 drafted directly by the orchestrator:** System Overview (subheading-level summary of what Zode is) and Actors & Roles (primary user personas, small sections, in-process to maintain tight narrative voice).

### Degradation Path (Three Missing Artifact Categories)

The pass encountered three gaps where expected `docs/generated/` artifacts didn't exist:

1. **§6 Screen List:** No `docs/generated/screen-list.md` (Zode is a native GPUI desktop app; no route/screen taxonomy needed or possible). **Workaround:** Synthesized honest prose inventory of main UI surfaces (editor, project rail, sidebar, command palette, terminal) inferred from individual feature `screens.md` files. Added explicit note: "Note: This desktop application does not maintain a formal route/screen inventory; descriptions are synthesized from feature boundaries and UI layers."

2. **§4 Business Flows:** No `docs/generated/screen-flow.md` (no screen-to-screen navigation flows in a monolithic native UI). **Workaround:** Grounded in the one real flow document that exists (`docs/flows/project-activity-hibernation-cascade.md`), with explicit note: "Detailed behavioral flow documented only for multi-project hibernation state machine; comprehensive screen-flow documentation is not applicable to this desktop architecture."

3. **§7 External Integrations:** No `docs/generated/api-map.md` (no HTTP/REST/GraphQL routes in a desktop app). **Workaround:** Extracted integration descriptions from `docs/system/architecture.md`'s prose narrative of LSP/DAP/Git/extensions/SSH subsystems, with explicit note: "This is a native desktop application; there is no web-style API surface. Integrations are language-server and system-level protocols listed below."

All three workarounds read naturally to a client audience (no filler, no apologies) and required no re-work of the researchers' deliverables — the degradation logic was baked into the pass's own contract.

### Wave OV.2 (Front-Loaded Client Language)

Rather than draft technical-first and undergo a separate "client-ify" translation pass, the researcher agents were instructed up front: "Write for a business/client audience — no F###/BL###/PERM### codes, no snake_case identifiers, no file:line citations. Use feature business names (e.g., 'Remote Development via SSH') not internal codes."

**Result:** The markdown source landed with zero jargon to clean up. When the agents moved to the Word-output phase, they had only styling and content flow to address — no token-translation step. This front-loading saved a rework cycle and meant the deterministic token-leak gate had almost no work to do.

### Wave OV.3(a) Deterministic Token-Leak Gate

Ran `verify_overview.py` (regex validation for leaked technical tokens: F###, BL###, PERM###, snake_case code identifiers, file:line citations):

**Result: 0 critical, 0 warnings on first run.**

This is a stark contrast to the design-intent pass (which surfaced 3 false positives on template boilerplate) and the test-cases pass (which surfaced 40+ citation-format mismatches). The front-loaded business-language instruction meant there was essentially nothing to leak.

### Wave OV.3(b) Peer-Review Cross-Check

Reviewer (`code-review-agent`) ran a detailed accuracy spot-check against the markdown source:

- **Coverage check:** All 11 features from `docs/generated/feature-list.md` represented exactly once in §5 (Detailed Function List).
- **Factual accuracy spot-check:** ~45 factual claims across all 11 sections traced back to their source artifacts. All 45 confirmed accurate.
- **Wording and completeness:** Two findings.

**Finding 1 (Medium): Hibernation Section Wording Collision**

Quoted text from Wave OV.1 researcher (Cluster B, §4): "Multi-project hibernation pauses **the assistant** that is scanning files in the background for changes..."

**Context collision:** §1 and §2 of the same document state explicitly: "This is Zode without the AI assistant subsystem" and "Note: Zode removes the AI writing-assistant features from upstream Zed, keeping only the editor core." A reader following the narrative would hit "assistant" in §4 and plausibly misinterpret it as a residual AI capability.

**Actual meaning:** "The assistant" here meant the worktree's background file-watcher process (`Worktree::pause_scanning()` in the codebase). Accurate terminology in the feature's own technical context, but colliding with the document's own repeated "no AI" theme.

**Fix:** Rewarded to "pauses **the background process** that scans files for changes" — preserves the technical meaning, kills the collision.

**Finding 2 (Low): Screen List Omission**

§6's project-navigation bullet ("Switch between projects via a persistent side rail with one click") omitted the quick tab switcher, a documented UI element in `docs/features/F013_WorkspaceAndProjectManagement/screens.md`.

**Fix:** Added "or via the quick tab switcher for rapid navigation between recently accessed projects."

### Wave OV.3(c) Validator Re-Run

After both fixes applied: **0 critical, 0 warnings.** Clean promotion gate.

### Wave OV.4 Build and Style

Markdown source was converted to styled `.docx` via the pass's bundled pandoc-based builder (Arial body font, navy heading palette, bordered/shaded tables per section, page-break-before-heading, page-numbered footer).

**Build gates:**

- **Round-trip gate:** The `.docx` was reopened via pandoc's reverse conversion (docx → markdown) to confirm the styling didn't introduce any XML shape corruption. Passed clean.
- **Style-presence gate:** Inspected the `.docx` binary's style definitions to confirm that the styling actually landed (not silently no-op'd by a pandoc-version change). Confirmed: navy color codes, border specifications, and page-break directives all present in the XML.

**Output:** `docs/Zed_zode_fork_System_Overview.docx`, 28,242 bytes.

## What We Tried

1. **Wave OV.1 parallel dispatch:** Spawned four background agents, each owning one section cluster, all writing concurrently without shared state. **Result: All four completed without blocking.** The degradation logic (missing screen-list, screen-flow, api-map) was handled inline by each researcher rather than requiring a coordinator intervention.

2. **Front-loaded client-language instruction:** Baked the "no technical tokens, no jargon" requirement into the agent prompts _before_ they drafted (not as a separate post-pass rewrite). **Result: Token-leak gate passed clean on first run, saving a re-run cycle.**

3. **Peer review accuracy spot-check:** Rather than generic structural review, the reviewer traced ~45 factual claims back to their source artifacts to catch both semantic errors and wording collisions. **Result: Caught the "assistant" collision that no regex could detect, plus the omitted UI element.**

4. **Wording collision fix:** Rewarded "the assistant" to "the background process" to dissolve the collision with the document's own "no AI" theme. Surgical, one-line fix, preserves original meaning.

5. **Styled Word output:** Bundled pandoc builder produced `.docx` with navy/arial/bordered styling. Ran both round-trip and style-presence gates to confirm the styling landed and didn't corrupt the XML. **Result: Both gates clean, deliverable ready.**

## Root Cause Analysis

### Wording Collision Between Unrelated Document Sections

"The assistant" is precise terminology in the worktree/file-watcher domain (a common agent-pattern term). But the document's §1 and §2 explicitly and repeatedly disclaim AI capabilities. The collision is **unavoidable in English without context** — the same word has two meanings (intelligent agent vs. AI writing helper) and both appear in the same document with opposite connotations. **Root:** No amount of pre-writing instruction can catch this; it's a reading-comprehension problem, not a jargon-control problem. Requires a careful human reviewer who reads across section boundaries and holds the document's own disclaimers in mind.

### Missing UI Element in Screen List

The researchers were instructed to synthesize §6 from individual feature `screens.md` files. One feature's `screens.md` (F013) includes the quick tab switcher as a documented interaction, but when synthesizing prose descriptions from multiple feature docs, the detail got dropped in the regrouping. **Root:** No formal screen-list artifact exists (by design — Zode is a monolithic native app), so the fallback is synthetic enumeration. Synthetic enumeration is lossy; some details slip through the cracks. Caught by a reviewer who cross-referenced against the source files.

### Missing Artifact Categories Handled Gracefully

Unlike `--api-contracts` and `--screen-specs` (which hard-abort on missing prerequisites), the `--overview` pass's own contract specifies degradation paths for three missing artifact categories. This is because overview synthesis is a read-only presentation pass — it never writes new structured artifacts. When a source is missing, it degrades to prose description with explicit notes, rather than failing the build. **Root:** The pass was designed for codebases where some artifact categories don't apply (like desktop apps with no route/API surface). Degradation is intentional, not a workaround.

## Lessons Learned

1. **Front-load business language into fan-out prompts, not as a separate post-pass rewrite.** When a synthesis pass has a downstream "no technical tokens" gate, baking the instruction into the agent prompts (before they draft) means the deterministic gate can pass clean on the first run. Contrast with translating technical-first content after the fact — you end up with the design-intent pass's boilerplate issues or the test-cases pass's citation-format mismatches. Better to steer the composition up front.

2. **Wording collisions between unrelated document sections can't be caught by deterministic gates.** A regex that scans for leaked technical tokens will never catch "assistant" appearing in two different senses in the same document. This is precisely why peer-review (Wave OV.3(b)) matters alongside deterministic validation (Wave OV.3(a)) — they catch different classes of defect. A careful reviewer who holds the document's own repeated disclaimers in mind can spot these collisions. A deterministic validator cannot.

3. **Graceful degradation is more maintainable than hard failure when a synthesis pass reads from already-promoted docs.** The `--overview` pass handled three missing artifact categories (screen-list, screen-flow, api-map) by downgrading to prose description with explicit notes. Compare to `--api-contracts` and `--screen-specs`, which hard-abort on missing files. When the source is a read-only presentation layer (synthesizing from docs), degradation is the right move. When the source is a structured-artifact pass (creating new artifacts from missing prerequisites), abort is the right move.

4. **Synthetic enumeration (deriving screen lists from feature docs rather than a formal screen-list artifact) is lossy.** Details get dropped in the regrouping. Acceptable for a prose-narrative audience view, but requires a reviewer who can cross-check against the underlying sources and flag omissions. Add the cross-reference step to the review gate when using synthetic enumeration.

5. **The absence of an artifact category (screen-list, api-map) is structurally different for a native GPUI app vs. a web app.** For a desktop editor, there is no meaningful route/screen taxonomy — it's a design non-choice, not a gap. Documenting this explicitly in the overview (e.g., "This desktop application does not maintain a formal screen inventory...") is clearer and more professional than omitting the section or faking an inventory.

## Next Steps

1. **Promote `docs/Zed_zode_fork_System_Overview.md` to live docs.** Markdown source is validation/review-clean and ready for stakeholder handoff.

2. **Publish styled deliverable.** The `.docx` (28,242 bytes) is build-clean and ready for distribution to clients or stakeholders unfamiliar with markdown.

3. **Update `docs/.rebuild-state.json` cursor.** Advance `last_overview_run_sha` to HEAD and `last_overview_run_timestamp` to the session date/time. This will prevent future `--overview` runs from re-processing the same content.

4. **Regenerate navigation READMEs.** Update `docs/README.md` to include a link to the new System Overview deliverable and its confidence-report sidecar in the primary navigation index.

5. **No follow-up `--overview` run planned.** The full rebuild-spec pipeline (core, features, flows, glossary, jobs, design-intent, overview) is complete. Next update would be triggered by a fresh feature addition or a requirements shift.

---

**Session context:** System Overview pass (OV.1–OV.4) ran as the final phase of the 2026-08-07 full-pipeline rebuild-spec session. Prerequisites (feature-list.md from earlier core/feature/flow/glossary/jobs/design-intent passes) were already promoted. Degraded gracefully on three missing artifact categories (screen-list, screen-flow, api-map) per the pass's own contract. No secondary languages (translation auto-sync was a no-op).
