# Ground truth: the Claude usage endpoint

**Date:** 2026-08-21 · **Method:** live probe against the maintainer's own account,
on their machine, with their own OAuth token. Nothing here is inferred.

## The call

```
GET https://api.anthropic.com/api/oauth/usage
Accept: application/json
Authorization: Bearer <accessToken>
anthropic-beta: oauth-2025-04-20
```

Returned `HTTP 200`.

**Not a documented public API.** The `anthropic-beta: oauth-2025-04-20` header and
the `/api/oauth/` path are what Claude Code itself uses. It can change without
notice, and the response already carries codenamed fields for features that do not
exist publicly. Anything built on it needs to degrade to "no data" rather than
break.

## Credentials

macOS: `security find-generic-password -s "Claude Code-credentials" -w` → JSON.
Other platforms: `~/.claude/.credentials.json` (absent on this machine).

```
claudeAiOauth: {
  accessToken, refreshToken, expiresAt, refreshTokenExpiresAt,
  scopes, subscriptionType, rateLimitTier
}
```

On this machine: `subscriptionType: "team"`, `rateLimitTier: "default_claude_max_5x"`.

Two consequences worth designing for:

- **`expiresAt` is real.** The access token expires. Refreshing it is Claude Code's
  job, not the editor's — an editor that tried would be racing another process for
  the same credential. Treat a 401 as "no data right now".
- **`ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_API_KEY`** being set
  means the user is pointed at something other than their subscription, so the
  subscription quota is meaningless. Skip the display entirely in that case.

## Response: use `limits[]`, not the named fields

The response has two parallel representations. The named one is what the takumi
kit reads:

```
five_hour:  { utilization: 53.0, resets_at: "2026-08-21T12:29:59.983789+00:00", … }
seven_day:  { utilization: 10.0, resets_at: "2026-08-27T23:59:59.983816+00:00", … }
```

The self-describing one carries strictly more:

```json
{"kind":"session",      "group":"session","percent":53,"severity":"normal","resets_at":"2026-08-21T12:29:59.983789+00:00","scope":null,"is_active":true}
{"kind":"weekly_all",   "group":"weekly", "percent":10,"severity":"normal","resets_at":"2026-08-27T23:59:59.983816+00:00","scope":null,"is_active":false}
{"kind":"weekly_scoped","group":"weekly", "percent":0, "severity":"normal","resets_at":null,                             "scope":{"model":{"id":null,"display_name":"Fable"},"surface":null},"is_active":false}
```

`limits[]` wins on three counts:

1. **It is where the third entry comes from.** The reference screenshot reads
   `50% used 1h 17m · 10% used 6d 12h · 0% used Fable`. That third entry exists
   only in `limits[]`, and the word "Fable" is `scope.model.display_name`. The
   named field for it (`seven_day_opus`, `seven_day_sonnet`, …) is `null`.
2. **`percent` is an integer.** `utilization` is a float whose scale is ambiguous —
   the kit carries a `normalizeUtilization` helper that guesses between 0–1 and
   0–100 precisely because of that. `percent` needs no guessing.
3. **It adapts.** The response also holds `amber_ladder`, `cinder_cove`,
   `iguana_necktie`, `omelette_promotional`, `nimbus_quill`, `tangelo` — codenames
   for things that are not public. All `null` here except `nimbus_quill`. Reading
   `limits[]` means never naming any of them.

### Do NOT filter on `is_active`

Only `session` is `is_active: true`, yet the screenshot shows all three entries.
So `is_active` marks which window is currently binding, not which to display.
Filtering on it would silently show one entry where three belong — a bug that
would have looked correct in any test written from the same wrong assumption.

### The rendering rule that reproduces the screenshot

For each entry in `limits[]`, in order:

```
"{percent}% used" + (countdown to resets_at, if resets_at is present)
                  + (scope.model.display_name, if there is no resets_at)
```

Checked against all three rows:

| entry | renders | screenshot |
|---|---|---|
| `session` 53%, resets 12:29 | `53% used 1h 17m` | `50% used 1h 17m` (probed minutes later) |
| `weekly_all` 10%, resets 27th | `10% used 6d 12h` | `10% used 6d 12h` |
| `weekly_scoped` 0%, no reset, model Fable | `0% used Fable` | `0% used Fable` |

`resets_at` is nullable, so the countdown must be optional — the `weekly_scoped`
row proves it rather than assuming it.

## Deliberately not displayed

`spend` and `extra_usage` carry money — limits, balances, and credit caps. Nothing
in the request asked for spend on the status bar, and it is the most sensitive part
of this payload. Left alone.
