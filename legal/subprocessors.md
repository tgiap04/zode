---
title: Subprocessor List
slug: subprocessors
---

**Last updated**: this document was rewritten for the Zode fork; it no longer describes
Zed Industries, Inc.'s hosted service.

## Scope

A subprocessor is a third party a service provider uses to process *your data* on its
behalf. The editor itself is not a service: it collects no usage data, and everything you
edit stays on your machine.

One optional feature does involve a server. If you sign in and turn on settings sync, your
`settings.json`, your `keymap.json` and the list of extension identifiers you have
installed are stored at `api.zodekit.site`. They are encrypted on your machine before they
are sent, with a key generated locally that the server never receives — so what is stored
is ciphertext nobody operating that service can read, and there is no server-held wrapping
key that could change that. Sync is off until you sign in; if you never do, nothing is
stored and no account exists.

Whoever hosts that endpoint therefore holds ciphertext, and the account identity and device
name needed to address it. Nothing else — no source code, no project contents, no usage
data. That is the whole of Zode's own data processing. See `legal/privacy-policy.md` for
the mechanism in detail.

## One request that is not Zode's own

If Claude Code is already signed in on your machine, Zode's status bar reads your plan
quota from Anthropic's usage endpoint, using Claude Code's own OAuth token. Anthropic is
not a subprocessor here: Zode sends them no data of yours to process, it reads a number
about an account you already hold with them, under the terms you already agreed to with
them. It is listed here only because it is an automatic outbound request and this document
should not pretend otherwise. `legal/privacy-policy.md` describes it in full.

## The one dependency worth naming anyway

Zode's extension registry queries and downloads go to Zed Industries' infrastructure
(`api.zed.dev`) — a retained, disclosed dependency, not a subprocessor, since Zode
doesn't send it any of your data. The request itself carries what any HTTP request
carries (your IP address, requested resource), and nothing else. See
`legal/third-party-terms.md` for the full disclosure and why this dependency exists.

Similarly, LSP auto-download fetches a language server binary directly from that
server's own distributor when you open a file in a language you don't have one
installed for yet. That's a download you triggered by opening a file, from whichever
project maintains that language server — not a subprocessor relationship either.
