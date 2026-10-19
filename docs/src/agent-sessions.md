# Agent Sessions Across Restarts

Close Zode with agent tabs open and reopen it, and the tabs come back: the same
agents, in the same panes, named the same way. What this page is about is the
part underneath — whether a tab comes back on the **conversation** it was in,
or on an empty one.

## What is remembered

Per tab, per project:

- which agent it runs
- its mode
- the name you gave it, if you renamed it
- the session it belongs to, and that session's own title as it read the moment
  the tab opened onto it. This is a snapshot, not a live link — it is what the
  tab is called when you have never renamed it, or after you clear a rename, but
  it will not follow the session's title if the transcript gets re-summarized
  later. A tab that started its own session is still called after its agent,
  even once that session's transcript is titled.

The conversation itself is never stored by Zode. It belongs to the agent's own
CLI, in the agent's own directory, and reconstructing it here would mean
inventing it. What Zode keeps is the session's **id**, which it hands back to the
CLI so that the CLI does the continuing.

## Which agents come back on their conversation

This is not the same for every agent, and the reason is a flag:

| Agent       | A tab you opened from history | A tab you opened fresh        |
| ----------- | ----------------------------- | ----------------------------- |
| Claude Code | comes back on its session     | **comes back on its session** |
| Codex       | comes back on its session     | comes back empty              |
| Copilot     | comes back on its session     | comes back empty              |
| opencode    | comes back on its session     | comes back empty              |

Claude Code accepts `--session-id`, so Zode chooses the id **before** starting
the CLI and knows it from that moment on. Codex, Copilot and opencode mint their
own id and do not report it, so a tab you opened fresh has no id for Zode to
write down — and writing down a guess would be worse than writing down nothing,
because the tab would then come back on somebody else's conversation.

Open a Codex, Copilot or opencode session from the history panel and it is
tracked from the start, because the id came from the panel.

## Two cases worth knowing

**A tab you opened and never typed in.** Claude writes its transcript only after
the first message, so a tab you opened yesterday and never used has an id and no
session on disk. It comes back as a working fresh tab that still owns that id —
type in it and the id becomes real.

**A session you deleted.** Deleting a session from the history panel removes the
conversation, not the tab that was in it. A Claude tab starts fresh under its own
id. A Codex, Copilot or opencode tab says the session is not available on this
machine and offers to start a new one, rather than quietly opening a blank
session that looks like a continuation.

Deleting is not the same operation for all four, though. Claude, Codex and
Copilot transcripts move to the OS trash and can be recovered from there.
opencode owns its sessions in its own store, not a file this editor can move to
the trash, so deleting one asks opencode itself to remove it (`opencode session
delete`). **That cannot be undone.** A bulk delete says so explicitly whenever
the sweep includes an opencode session.

Deleting does not stop an agent that is still running in an open tab. If a tab
is running the session you are about to delete, the confirmation says so and
suggests closing the tab first. The agent may keep writing to its transcript
after it is moved, so the tab can stop reporting finished turns and the session
can come back in the history list. Zode does not refuse the delete: closing the
tab is your call.

## Forked sessions are not tracked

**Continue in New Session…** in the history panel forks a session. The tab
starts the CLI in the folder the session ran in with the agent's own fork flag
(`--resume <id> --fork-session` for Claude Code, `-s <id> --fork` for opencode),
so the new tab starts from the conversation so far. Codex and Copilot have no
fork, so the entry is disabled for them. If the original is gone from the
agent's store, the tab says the session is not available rather than opening a
blank one.

The CLI makes a **new** session id for the branch, and it does not say which, so
Zode cannot write it down. A forked tab is therefore not tracked: after you
restart Zode it comes back empty rather than resuming the branch, which is the
one thing that would be actively misleading. Starting the agent again in that
tab, after its CLI has ended, forks the original once more and gives a new
branch.

## What starting looks like

Restored tabs start their CLI immediately rather than waiting to be clicked, so
the agents are ready when the window is. With several tabs open that means
several CLI processes at startup, each loading its transcript and any MCP servers
it is configured with. The [project footprint
indicator](./project-footprint-indicator.md) on the status bar is where that shows
up.

## Where Claude Code's transcripts are read from

Zode reads Claude Code's transcripts from the `projects` folder under
`CLAUDE_CONFIG_DIR` when that variable is set to an absolute path, and from
`~/.claude/projects` otherwise. It reads the variable from its own environment.
An empty value and a relative path are ignored: a relative path is relative to
the CLI's working directory, which Zode does not have, so Zode reads the default
folder instead.

Only transcripts named by a session UUID are listed. A `.jsonl` file with any
other name is skipped, because its name is what gets passed to `--resume`, and
Claude Code cannot resume a transcript it did not name that way.

## Where it is stored

In Zode's own database, beside the rest of the workspace layout — not in your
settings file, and not in the agent's directory. Removing a project's history
removes its tabs' session ids with it.
