---
passed: true
issues: 0
warnings: 2
---

# User Stories Review (Wave 4.5 re-check)

**Scope**: `user-stories.md` only. Re-check after splitting old US005 (diagnostics+completions → US005/US026) and old US015 (view diff+stage hunk → US015/US027), and renumbering to stay contiguous.

## Checks

### 1. Single intent per story (critical) — PASS
All 27 stories now describe exactly one user/system intent. Confirmed the two previously-flagged multi-intent stories are resolved:
- US005_ReceiveLspDiagnostics (diagnostics only) / US026_ReceiveLspCompletions (completions only) — cleanly split, each with its own AC, Test Scenarios, Background Logic.
- US015_ViewGitDiff (view diff only) / US027_StageGitHunk (stage hunk only) — cleanly split, each with its own AC, Test Scenarios.
No remaining story bundles two independent actions under one AC set.

### 2. Actor clarity (critical) — PASS
Every story opens "As a {developer|collaborator|extension author|collaboration Admin}, I want ... so that ...". Roles match the Roles-used glossary. No ambiguous/unnamed actor found.

### 3. Outcome present (warning) — PASS
All 27 stories include a "so that" clause stating a concrete benefit, not just a restated action.

### 4. Overly broad scope (warning) — PASS with 2 minor notes
No story is broad enough to require a split, but two stories carry one AC that stretches slightly past the story's core action:
- US012_ChangeCollaboratorRole (Criterion 3): the "hard-denied destructive git-worktree ops regardless of role" rule is a permissions-model detail bleeding in from permissions.md's Special Conditions, not really about the role-change action itself.
- US013_StartVoiceCall (Criterion 2): the Talker-vs-Guest mic/chat permission split is role-gating logic riding along in a "start a call" story rather than a dedicated call-permissions story.
Neither warrants a critical split — both remain single-intent (role change; start a call) with the extra criterion as supporting context — flagged for awareness only.

### 5. US### uniqueness (critical) — PASS
Index lists US001–US027, all codes distinct, contiguous (no gaps, no duplicates). Renumbering from the split is confirmed contiguous.

## Result
0 critical issues. 2 warnings (both minor, non-blocking). Previously-flagged critical multi-intent violations (old US005, old US015) are confirmed resolved.
</content>
