---
passed: true
issues: 0
warnings: 4
---

# DataModel Structural Gate — Wave 1.5

Passed Checks:
✓ entity_completeness
✓ disc_scope
✓ model_uniqueness
✓ disc_orphan_check
✓ relationship_completeness

## Warnings

1. **DISC-001 orphan-leaning anchor** (Workspace, MODEL002): anchored field is `open_mode (call-param, OpenMode enum, workspace.rs:1422)` — explicitly documented as a function call-parameter, not a stored field in the Workspace attributes table (no `open_mode` row exists there). Values `NewWindow`/`Add` are behaviorally distinct (2 values, non-boolean), so Check 2 scope passes, but Check 4's "references an actual entity field in the same entity block" is only partially satisfied since the field isn't in the table. Self-disclosed, not misleading — downgraded from critical to warning.

2. **DISC-005 orphan anchor** (Worktree, MODEL004): `WorkDirectory` enum (`InProject`/`AboveProject`, worktree.rs:207) has no corresponding field row in the Worktree attributes table (table lists `Snapshot.*`, `LocalWorktree.*`, `RemoteWorktree.*`, `LocalSnapshot.git_repositories` only — nothing typed `WorkDirectory`). Values are distinct (2, non-boolean), so Check 2 scope passes, but the field-anchor in Check 4 is not demonstrated within the entity's own table.

3. **Missing cardinality tag** (Entity<T>/WeakEntity<T>, MODEL006): the relationship line "Referenced by every other struct-based entity in this document as their storage/ownership mechanism" is prose without an explicit cardinality label (unlike the sibling One-to-One line above it).

4. **Missing cardinality tag** (Editor, MODEL010): the relationship line "Referenced by ProjectPanel (filename_editor…) and many other panels for inline rename/filter inputs" omits an explicit cardinality label.

## Notes

- All 18 MODEL### codes (001–018) are unique and match the Summary's count.
- All 13 DISC### codes (001–013) are sequential with no gaps and no duplicates; no DISC-### has only boolean or single-value enums.
- Every entity block has name, description, and a field table with types populated.
- Core relationships (the vast majority) carry explicit cardinality (One-to-One / One-to-Many / Many-to-One); only the two supplementary prose lines above lack it.
- None of the above rise to critical: no entity lacks fields/types, no DISC-### is boolean-only or single-valued, no duplicate MODEL### codes, and the two DISC anchor gaps are self-documented (source line cited) rather than undocumented dangling references.

**Recommendation**: proceed. Pipeline is not blocked; the four warnings are worth a follow-up pass (either add `open_mode`/`WorkDirectory`-typed fields to their entity tables, or move those DISC entries to a "cross-cutting discriminators" note) but do not compromise the model's structural integrity.
