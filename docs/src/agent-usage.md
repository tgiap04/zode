---
title: Agent Usage Indicator
description: What the status bar shows about your Claude Code and Codex subscription quota, the panel and menu it opens, where the numbers come from, and when it goes blank.
---

# Agent Usage Indicator

A small indicator on the left side of the status bar, next to the activity indicator, shows how much of your Claude Code and/or Codex subscription quota is used:

```
53% used 1h 17m · 10% used 6d 12h · 0% used Fable
```

Each `·`-separated group is one quota window — a percentage, and either a countdown to when it resets or, for a window scoped to one model, that model's name instead.

**Left-click** it to open the usage panel. **Right-click** it for a menu of which status-bar items are shown. Both are described below. Three settings control the indicator itself — see [Status Bar settings](./visual-customization.md#status-bar).

## Usage panel

Left-clicking the indicator opens a panel:

- A header reading "Usage", a scope label (`all agents`, `Claude only`, `Codex only`, or `none on the bar`, depending on which agents are switched on), and a refresh glyph that re-reads both agents immediately.
- A **Detailed** / **Compact** toggle. This writes the `agent_usage_display` setting — the same one described in [Status Bar settings](./visual-customization.md#status-bar) — so the choice persists across restarts rather than reverting when the panel closes.
- One row per agent: its icon, its name, a one-line status (a countdown to the window closest to running out, or the reason there is nothing to show), and a compact percentage bar per quota window.

Click a row's chevron to expand it in place. The expanded row shows, per window, the window's full name, its exact percentage, and an **absolute** reset time in your local timezone — the collapsed row shows a countdown instead, which is easier to read at a glance but can't be checked against a clock.

Opening the panel, expanding a row, and switching Detailed/Compact all read state already in hand or write a setting — none of them makes a network request.

## Right-click menu

Right-clicking the indicator opens a menu of tick-boxes for which status-bar items are shown: **Claude Usage**, **Codex Usage**, then a separator, then **Active File Name**, **Active Language**, **Cursor Position**, **Line Endings**, and **Active Encoding**. Each box writes straight to the matching `status_bar.*` setting (see [Status Bar settings](./visual-customization.md#status-bar)).

Two things worth knowing before you use it:

- **Switching off both Claude Usage and Codex Usage removes the indicator from the status bar entirely** — and with it, the only way to right-click your way back to this menu. This is a known, accepted limitation rather than a bug. If that happens, reopen the menu from the Settings Editor ({#kb zed::OpenSettings}) or by editing `claude_usage_button` / `codex_usage_button` back to `true` in your settings file — see [Settings Files](./configuring-zed.md#settings-files).
- **The Active Encoding row is a two-state tick-box for a setting that has three states.** Ticking it writes `enabled`; clearing it writes `disabled`. The shipped default is `non_utf8` (show the button only for non-UTF-8 files), which the tick-box reads as "on" — but switching it off and back on lands on `enabled`, not back on `non_utf8`. `non_utf8` is only reachable by editing the setting directly.

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

Hover the indicator for the specific reason. A request that simply failed keeps showing the last numbers it had rather than clearing them — clearing only happens for the reasons above, where the old numbers would no longer be a true answer.

## Polling

Both agents are read together every 60 seconds, but only while the window is focused. Regaining focus reads both again immediately rather than waiting for the next tick, and clicking the refresh glyph — on the status bar or in the panel — does the same on demand. This is unchanged by the panel and menu: opening either reads state already in hand and triggers no extra request.

See also: [Telemetry](./telemetry.md#agent-subscription-quota) for how this fits into what this build sends over the network.
