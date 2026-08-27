# Agent Sessions Across Restarts

Close Zode with agent tabs open and reopen it, and the tabs come back: the same
agents, in the same panes, with the names you gave them. What this page is about
is the part underneath — whether a tab comes back on the **conversation** it was
in, or on an empty one.

## What is remembered

Per tab, per project:

- which agent it runs
- its mode
- the name you gave it, if you renamed it
- the session it belongs to

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

Claude Code accepts `--session-id`, so Zode chooses the id **before** starting
the CLI and knows it from that moment on. Codex and Copilot mint their own id and
do not report it, so a tab you opened fresh has no id for Zode to write down —
and writing down a guess would be worse than writing down nothing, because the
tab would then come back on somebody else's conversation.

Open a Codex or Copilot session from the history panel and it is tracked from the
start, because the id came from the panel.

## Two cases worth knowing

**A tab you opened and never typed in.** Claude writes its transcript only after
the first message, so a tab you opened yesterday and never used has an id and no
session on disk. It comes back as a working fresh tab that still owns that id —
type in it and the id becomes real.

**A session you deleted.** Deleting a session from the history panel removes the
conversation, not the tab that was in it. A Claude tab starts fresh under its own
id. A Codex or Copilot tab says the session is no longer on this machine and
offers to start a new one, rather than quietly opening a blank session that looks
like a continuation.

## Forked sessions are not tracked

Forking (`--fork-session`) tells the CLI to make a **new** session id, and it does
not say which. A forked tab therefore comes back empty rather than resuming the
session it was forked from, which is the one thing that would be actively
misleading.

## What starting looks like

Restored tabs start their CLI immediately rather than waiting to be clicked, so
the agents are ready when the window is. With several tabs open that means
several CLI processes at startup, each loading its transcript and any MCP servers
it is configured with. The [project footprint
indicator](./project-footprint-indicator.md) on the status bar is where that shows
up.

## Where it is stored

In Zode's own database, beside the rest of the workspace layout — not in your
settings file, and not in the agent's directory. Removing a project's history
removes its tabs' session ids with it.
