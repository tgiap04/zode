# Organize-Files Docs Directory Review — Three Independent Systems, No Changes Needed

**Date**: 2026-08-07 17:35
**Severity**: low (advisory pass, no defects)
**Component**: Documentation structure, file organization, docs/ directory governance
**Status**: resolved

## What Happened

Ran `/tkm:organize-files docs/` to audit the `docs/` directory for file-placement and naming violations against the generic organize-files ruleset. The skill scanned the full tree and reported back with zero violations and zero suggested moves.

Initial reaction: "That's surprising—organize-files usually finds at least a few small inconsistencies." The second pass was to read the skill's detailed findings and understand why the tree was clean. Answer: the `docs/` directory is not one flat pile governed by a single naming and placement convention. It's actually three independently-governed systems sharing one parent directory, each with its own authority and canonical layout. Once attributed correctly, every file fell into its proper subsystem, and no moves were warranted.

## The Brutal Truth

The temptation exists—when an organize-files pass returns "no changes needed," there's a nagging feeling that the tool didn't look hard enough, or the directory really is a mess but the scan missed it. What actually happened here is the opposite: organize-files' generic Rule 1 (flatten markdown into `docs/*.md` unless a subdirectory is explicitly carved out) would have been _wrong_ to apply across all of `docs/`, because two of the three systems have higher-authority conventions already in place. Respecting that hierarchy—not flattening it—is the correct move. This teaches a lesson about when "no changes" is a genuine victory, not a gap.

## Technical Details

The `docs/` directory holds these three systems:

### 1. Upstream Zed's mdBook Documentation (Pre-Existing, Human-Owned)

- **Files:** `docs/src/`, `docs/theme/`, `docs/book.toml`, `docs/AGENTS.md`, `docs/.rules`, `docs/.prettierrc`, `docs/.prettierignore`, `docs/.gitignore`
- **Governance:** mdBook's own convention (chapters in `src/`, theme customizations in `theme/`, toml config at root)
- **Authority:** Higher than organize-files' generic rules—this is vendored/upstream, not this project's to reorganize
- **Organize-files scope:** None—these files are off-limits

### 2. Rebuild-Spec's Layered Spec Docs (Auto-Generated, Rebuild-Spec-Governed)

- **Files:** `docs/system/`, `docs/generated/`, `docs/features/`, `docs/flows/`, `docs/journals/`, `docs/.rebuild-state.json`, `docs/_source-to-fcode.json`, `docs/Zed_zode_fork_System_Overview.{md,docx}`, plus every `confidence-report_*.md` sidecar
- **Governance:** rebuild-spec's own canonical docs-mapping (defined in the skill, enforced by its generation pipeline)
- **Authority:** Higher than organize-files—this is a declared spec pipeline with its own output contracts
- **Organize-files scope:** Read-only—should recognize the structure and not move it

### 3. Project's Own Journal Entries (`docs/journals/` — Organize-Files' Direct Domain)

- **Files:** All `YYYY-MM-DD-{slug}.md` entries (5 existing entries as of this scan)
- **Governance:** Project journal conventions, enforced by organize-files
- **Authority:** organize-files owns this directory explicitly for naming/placement
- **Organize-files scope:** Full authority—can validate and move files here
- **Finding:** All 5 existing entries already follow the `YYYY-MM-DD-slug.md` pattern correctly ✓

## What We Tried

1. **Naive read of organize-files output:** Interpreted "no changes needed" as "tree is squeaky clean, tool did its job." This was correct, but didn't explain _why_ it was clean. Proceeded to deeper analysis.

2. **Deep directory scan and system attribution:** Traced back the origin and governance of every subdirectory and file cluster. Identified that `docs/src/`, `docs/book.toml`, `docs/theme/` belong to mdBook (upstream). Identified that `docs/features/`, `docs/generated/`, `docs/flows/`, `docs/system/` carry rebuild-spec's own output metadata. Identified that `docs/journals/` is the project's own, organize-files-governed space.

3. **Validate naming within each system:**

   - mdBook files: follow mdBook's conventions (not organize-files' concern) ✓
   - rebuild-spec artifacts: follow rebuild-spec's documented output contracts ✓
   - journal entries: all follow `YYYY-MM-DD-slug.md` pattern ✓

4. **One advisory finding (not acted on):** The rebuild-spec skill's build_navigation.py generator had written `README.md` nav-index files into three directories outside its own spec-output scope:

   - `docs/src/README.md` (inside upstream Zed's mdBook source tree)
   - `docs/.conventions/README.md` (pre-existing kit/doc-writer reference material)
   - `docs/.doc-examples/README.md` (pre-existing kit/doc-writer reference material)

   These are rebuild-spec-generated files landing in non-rebuild-spec directories. Presented this to the user as a question: "Should these be removed?" User chose: "Leave them. They don't hurt, and removing them risks fighting rebuild-spec's own regeneration on its next run." **Correct call.** Cleaning up rebuild-spec-generated artifacts is that skill's own concern, not a file-organization move.

## Root Cause Analysis

Why did the tree pass without changes?

**Core reason:** organize-files was designed as a generic, one-size-fits-many directory auditor. It has sensible defaults for a flat, single-authority docs tree (Rule 1: flatten markdown to `docs/*.md`). But `docs/` here is not flat—it's a federation of three independently-governed subsystems. Once each subsystem's authority and conventions were recognized, every file fell into its proper place. No changes were needed because the tree was already _correctly_ segregated by system.

The potential pitfall: A less careful tool (or an operator who didn't ask "what's the real structure here?") could have mechanically applied organize-files' Rule 1 across everything, sliding files around and flattening directories. That would have corrupted the rebuild-spec pipeline's expected output layout and tangled the upstream mdBook source. **Not acting**—respecting the hierarchy—is the right answer.

## Lessons Learned

1. **When a file-organization audit returns zero changes, don't assume the tool missed something.** Ask instead: "Is this directory actually a single-authority system, or a federation of independently-governed subsystems?" If it's a federation, zero changes is the correct result.

2. **Recognize that a shared parent directory doesn't mean a single set of rules applies everywhere within it.** `docs/` is a parent directory that _happens_ to hold an upstream docs site, a generated spec pipeline, and a project journal—three separate authorities. Respecting each authority's own conventions is more important than enforcing a single flat layout across the whole tree.

3. **Higher-authority conventions take precedence over generic tool rules.** Upstream Zed's mdBook structure > organize-files' generic Rule 1. Rebuild-spec's output contracts > organize-files' generic Rule 1. A tool is useful precisely because it automates low-level details, but those details must yield when a more specific, higher-authority convention is already in place.

4. **Generated-artifact cleanup is the generating tool's concern, not a separate skill's.** When rebuild-spec writes README.md files to unexpected directories, that's rebuild-spec's thing to own (and potentially fix in the next run). Don't pull those files out manually—you'll just be fighting the generator's next regeneration cycle.

## Next Steps

1. **No file moves to perform.** The docs/ directory structure is correct as-is. All three subsystems are properly segregated and follow their own conventions.

2. **Document this finding for future reference.** The organize-files pass is complete; the conclusion ("nothing to move") is intentional and correct, not an incomplete scan. Future sessions that re-run organize-files on `docs/` should be confident in the result.

3. **Optional upstream consideration (rebuild-spec skill maintainers only):** The nav-index generation could be scoped more narrowly so it doesn't write README.md files into non-spec directories. But this is not a zode-codebase concern—it's an upstream toolkit design question.

---

**Session lead:** Orchestrator (main)
**Tool used:** `/tkm:organize-files docs/` (read-only scan)
**Result:** Zero violations, zero changes proposed
**Advisory findings:** 1 (rebuild-spec nav README placement—user chose to leave unchanged)
**Key insight:** Three independent governance systems, not one flat pile
