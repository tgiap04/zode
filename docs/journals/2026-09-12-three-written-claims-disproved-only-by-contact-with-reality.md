# Three written claims were wrong, and only contact with reality found them

**Date**: 2026-09-12 18:35
**Severity**: Medium — every mistake was caught before landing, but none by the process that should have caught it
**Component**: `agent_sessions`, `agent_ui`, `floating_pane`
**Status**: Resolved (PR #37, commits `39fe5ca` through `743e0db`)

## What Happened

opencode joined the agent rail as a fifth entry — icon, launch button, session history with list, resume, fork and delete. The work went through red-team review (25 findings, 2 critical, all applied), seven phases of implementation, and an inspection gate that scored 9.3 and sealed the feature. Three things had been written down during the planning or implementation phases and were provably wrong. None was caught until contact with reality or a human reading the code.

## The Brutal Truth

### 1. A hardening that would have broken the feature

**Plan acceptance:** The red team's H1 finding proposed emitting a `--` separator before the session id in the delete argv, so a leading dash could never be read as a flag. It was adjudicated, accepted, and written into the plan as a belt-and-braces control.

**Reality:** During phase 06's implementation, the proposed command was tested against the real opencode 1.14.20:

| Argv                              | Observed                                                                    | Why it matters                                                                                               |
| --------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `opencode session delete <id>`    | Attempts the delete; exits 1 with `Error: Session not found`                | Correct path.                                                                                                |
| `opencode session delete -- <id>` | Prints `session delete`'s own usage; exits 1; **never attempts the delete** | yargs does not bind a positional through `--`. The separator would break **every opencode delete silently**. |
| `opencode -- session delete <id>` | Stops subcommand matching; launches the interactive TUI as a detached child | Exactly the hang the timeout exists to survive, but now on every delete.                                     |

The `^ses_[A-Za-z0-9]+$` id anchor — verified against the live store on this machine, 7 of 7 sessions matched, zero exceptions — already closes the concern. A dash-prefixed id is a violation of the anchor; nothing it admits can begin with `-`. The hardening was belt-and-braces over the anchor and it cut the belt.

**How it was caught:** Testing against the real CLI during the forge, not during the review that accepted it.

**Recorded:** Three places in the code (`opencode.rs:244-264`, `277-281`, `288-294`) so no later reader "restores" the missing `--`.

---

### 2. An ownership list built from where a symbol is defined, not where it is used

**Plan:** Phase 05's file ownership list named `floating_pane/src/content.rs`, where the `AGENTS` table was declared, but not `floating_pane/src/render.rs`, where it was consumed.

**Reality:** The implementing agent made the substitutions (`AGENTS.iter().copied()` → `agent_ui::agent_marks()`) and discovered `floating_pane/src/render.rs` held `use crate::content::AGENTS;`. The crate would not compile.

**How it was caught:** The agent disclosed it rather than silently widening the scope to fix it. The fix was mechanical (two substitutions) and applied immediately.

**Lesson written to the tree:** A file ownership list built from call sites rather than definitions keeps this mistake from coming back. Phase 05's plan now records the error and the fix (`phase-05-consolidate-the-agent-roster.md:27-39`).

---

### 3. A self-contradicting claim that survived red team and the first forge pass

**Claim:** `opencode.rs`'s id-validation comment said _"neither delete nor resume has a shell in the path."_

**Then immediately contradicted:** Two sentences later it said `agent_task` drops args into `SpawnInTerminal` — which is exactly the path that shell-quotes and joins for a remote workspace (`crates/project/src/terminals.rs:160-168`).

**What it should have said:** "Delete has no shell in the path. Resume does, and this matters if the pattern is loosened."

**How it was caught:** The inspection gate read the comment against the code and refused to seal until it was fixed. This was the branch's own accepted M1 finding from red team; the phase that should have applied it did not.

**Recorded:** Commit `886068c` corrects all three claims with explicit scope: "This is a shape check and defence in depth, not an injection guard. For deletion that is because it spawns through real argv with no shell in between... **Resume does not get that guarantee, and the difference matters** to anyone loosening the pattern above."

**Also noted:** The same commit records why no `busy_timeout` is set on the read-only WAL connection — it was **measured** against the real store with opencode holding it open and appending, not assumed. Every read returned in 1-7ms and none blocked. That evidence (`evidence/temper-results.json:liveObservations[2]`, score 9.3) did not exist during the initial inspection; contact with the real system is what made the observation provable.

## Technical Details

### The evidence gate itself caught orchestrator dishonesty

A probe was recorded with `exitCode: 1` and `status: "pass"` — non-zero exit marked pass, which is the shape of a hidden failure. The gate refused it as inconsistent. The fix was to separate observations of a third-party binary from our own pass/fail gates, keeping the real exit codes and adding a `matchedExpectation` field. One of those observations is `false`: the `--` hardening tested above.

### Coverage paid where it became payable

Phase 04 built the deletion command trait but could not test three paths — no provider returned `Deletion::Command` until phase 06 added opencode. The implementing agent wrote three GPUI tests against a real subprocess, hit two different failure modes (including a panic inside `blocking::Executor`'s thread pool on a second run), and **removed them rather than ship flaky tests**. Phase 06 closed all three by injecting the command runner as a seam instead of fighting the harness.

### The riskiest phase stalled on its first attempt with zero files touched

Phase 04 split from phase 05 specifically to de-risk the rail button — if the deletion trait shape was wrong, it should break before opencode was entangled. The first run touched zero files and hit a coordination issue. The retry succeeded with explicit anti-stall instructions (run builds in the foreground, edit everything first and verify once, never poll).

### An unmeasured claim became a measured fact

The live-WAL read under contention was stated in the plan as a concern the design reasoned about without evidence. Phase 06 proved it: an `opencode serve` process held the store open and appended while `availability()` and `list()` were driven against it. Every read returned in 1-7ms. None blocked. The observation now sits in `with_connection`'s doc comment.

## Lessons

1. **A claim about what code does is checkable, and it should be checked before the code is shipped.** Three specific statements — "the `--` is safe", "this file does not consume that symbol", "neither delete nor resume has a shell" — were all false. They were caught by different gates: one by testing the CLI, one by the agent building it, one by a human reading both the comment and the code it described. A fourth gate between acceptance and implementation would have caught all three: read the claim, check it against what the code says, and do not proceed if they disagree.

2. **An ownership list built from definitions, not call sites, will keep misplacing files.** The pattern that works: grep for the symbol's uses, name every file where it appears or is consumed, then verify by deleting it. If the build fails, the list was incomplete.

3. **A `-shm`/`-wal` live database under contention is something the design can reason about but the implementation should verify.** The measurement (1-7ms, no blocks) was not expensive, and it upgraded "we believe the timeout is safe" to "we have measured it". Record such verifications in the code and in the evidence, so the next person does not re-derive it.

4. **When a subagent finds a defect in its own phase that's small enough to fix, disclosing it rather than hiding it keeps the design visible.** The file ownership mistake would have been silent otherwise. Recording why the fix worked preserves the lesson.

5. **An inspection gate can catch contradictions between comment and code, but not contradictions between design and reality.** Only contact with the real CLI found the `--` failure. A step in the forge that runs the proposed command against the real binary would have caught it at implementation time rather than review time.

## What Could Not Be Proven Without a Display

- **No end-to-end click test opening opencode via the rail, resuming a session, then deleting it from the live UI** — the command delete was proven to reach the real binary and close the session (verified by `opencode session list` after), but the round trip through the panel's UI was not exercised.
- **A command delete opening no terminal tab** — verified by code inspection of `util::command::Command` and how `agent_task` uses it, not by running it.

## Next Steps

No action. All three claims have been corrected in the code or in the evidence record. The lessons are now written down for the next session that handles deletion logic, CLI integration, or claims about what the code does.
