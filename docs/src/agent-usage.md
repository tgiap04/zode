---
title: Agent Usage Indicator
description: What the status bar shows about your Claude Code and Codex subscription quota, the panel and menu it opens, where the numbers come from, when it goes blank, and why Antigravity and Copilot are absent from it.
---

# Agent Usage Indicator

A small indicator on the left side of the status bar, next to the activity indicator, shows how much of your Claude Code and/or Codex subscription quota is used:

```
53% used 1h 17m · 10% used 6d 12h · 0% used Fable
```

Each `·`-separated group is one quota window — a percentage, and either a countdown to when it resets or, for a window scoped to one model, that model's name instead.

**Left-click** it to open the usage panel. **Right-click** it for a menu of which status-bar items are shown. Both are described below. Three settings control the indicator itself — see [Status Bar settings](./visual-customization.md#status-bar).

## Usage panel

Left-clicking the indicator opens a panel. It closes as soon as you click anywhere outside it.

- A header reading "Usage", a scope label (`all agents`, `Claude only`, `Codex only`, or `none on the bar`, depending on which agents are switched on), and a refresh glyph that re-reads both agents immediately.
- A **Detailed** / **Compact** toggle. This writes the `agent_usage_display` setting — the same one described in [Status Bar settings](./visual-customization.md#status-bar) — so the choice persists across restarts rather than reverting when the panel closes.
- One row per agent: its icon, its name, a one-line status (a countdown to the window closest to running out, or the reason there is nothing to show), and a compact percentage bar per quota window.

Click a row's chevron to expand it in place. The expanded row shows, per window, the window's full name, its exact percentage, and an **absolute** reset time in your local timezone — the collapsed row shows a countdown instead, which is easier to read at a glance but can't be checked against a clock.

Opening the panel, expanding a row, and switching Detailed/Compact all read state already in hand or write a setting — none of them makes a network request.

## Right-click menu

Right-clicking the indicator opens a menu of two tick-boxes: **Claude Usage** and **Codex Usage**. Each writes straight to the matching `status_bar.*` setting (see [Status Bar settings](./visual-customization.md#status-bar)).

This menu is only about the two usage indicators — it doesn't cover the other `status_bar.*` items (active file name, active language, cursor position, line endings, active encoding). Those are still fully settable, just from the Settings Editor or the settings file directly, not from here.

One thing worth knowing before you use it: **switching off both Claude Usage and Codex Usage removes the indicator from the status bar entirely** — and with it, the only right-click target this menu has. This is a known, accepted limitation rather than a bug, and it matters more now that the menu has nothing else to right-click onto. If that happens, get back to `true` from the Settings Editor ({#kb zed::OpenSettings}) or by editing `claude_usage_button` / `codex_usage_button` back to `true` in your settings file — see [Settings Files](./configuring-zed.md#settings-files).

## Where the numbers come from

**Claude** — a request to `https://api.anthropic.com/api/oauth/usage`, authorized with the OAuth access token Claude Code already holds: on macOS, from the keychain (`security find-generic-password -s "Claude Code-credentials"`); otherwise from `~/.claude/.credentials.json`. The token is read fresh for each request — it is never stored, logged, or written anywhere by this build.

This is **not a documented public API**. It is the same endpoint Claude Code's own CLI uses, unannounced and unversioned. It can stop working without anything in this editor changing — if it does, the indicator simply stops reporting Claude's numbers.

**Codex** — a short-lived `codex app-server` subprocess, asked over JSON-RPC (`account/rateLimits/read`, preceded by the `initialize` handshake the server requires before it will answer anything). This editor never touches a Codex credential; the subprocess holds and refreshes its own session.

Verified against `codex-cli 0.149.0` and cross-checked against the protocol schema the CLI emits itself (`codex app-server generate-json-schema`). That schema is marked experimental and still moving, so if it shifts, Codex's numbers stop appearing and one line in the log names the JSON keys that came back instead — never their values. There is an ignored test, `reading_a_live_codex_app_server`, for re-checking this against a newly upgraded CLI.

Money is deliberately not read: Claude's response also carries `spend` and `extra_usage`, and nothing in this build reads them.

## When it shows nothing

The indicator draws no icon and reserves no space for a source with nothing to say. Reasons, per agent:

**Claude** goes quiet when:

- `ANTHROPIC_BASE_URL`, `ANTHROPIC_AUTH_TOKEN`, or `ANTHROPIC_API_KEY` is set — the CLI is pointed somewhere other than your subscription, so subscription quota no longer describes it
- no Claude Code sign-in exists on this machine
- the signed-in plan reports no quota windows at all (for example, a free plan)

**Codex** goes quiet when:

- the `codex` CLI isn't installed
- Codex answered, but this build couldn't find the expected fields in the response

Open the panel for the specific reason — it keeps a row for a silent agent and says why it is silent, which is the one thing the status bar itself cannot tell you. A request that simply failed keeps showing the last numbers it had rather than clearing them — clearing only happens for the reasons above, where the old numbers would no longer be a true answer.

## Why Antigravity and Copilot have no numbers

The rail offers four agents; this indicator reports on two. That is not an oversight
waiting on implementation — it is what probing the other two CLIs turned up.

**Copilot** publishes no route to subscription quota that this editor could read:

- `copilot --help` carries no usage or quota flag, and no subcommand for one.
- Its `/usage` slash command is described by the CLI itself as "Display session usage
  metrics and statistics" — how much this _conversation_ consumed, not how much of a
  subscription remains. It is a different number from the one this indicator shows, and
  it exists only inside the interactive TUI.
- Its ACP server (`copilot --acp`) advertises `loadSession`, prompt capabilities and
  session listing, and nothing about an account. `account/rateLimits/read` — the method
  the Codex route uses — answers `-32601 Method not found`, as do
  `account/usage/read`, `usage/read`, `account/read`, `rateLimits/read` and
  `account/subscription/read`.
- Its own logs name a single host, `api.individual.githubcopilot.com`, and contain no
  quota, premium-request, entitlement or rate-limit vocabulary at all.

GitHub does meter premium requests, so a REST route may well exist. It was not pursued:
reaching it would mean this editor holding a GitHub credential of its own, and the Codex
route was deliberately chosen over exactly that trade for Claude's sibling. If such a
route is ever wanted here, it is a decision about credential scope first and an
implementation second.

**Antigravity** keeps nothing on this machine to read. `agy` 1.1.20 was installed, signed
in, and run twice; it wrote **zero bytes** anywhere under the home directory or the working
directory, and `agy -c` still recalled the previous turn. Its conversations — and therefore
anything the server knows about quota — live behind `/v1/conversations/{conversation_id}`,
reachable only with the credential the CLI holds.

That is the same wall Copilot hits, for a different reason: not a missing method, but a
missing local store. Reading it would mean this editor borrowing a Google credential, which
is the trade the Codex route exists to avoid.

## Polling

Both agents are read together every 60 seconds, but only while the window is focused. Regaining focus reads both again immediately rather than waiting for the next tick, and clicking the refresh glyph — on the status bar or in the panel — does the same on demand. This is unchanged by the panel and menu: opening either reads state already in hand and triggers no extra request.

See also: [Telemetry](./telemetry.md#agent-subscription-quota) for how this fits into what this build sends over the network.
