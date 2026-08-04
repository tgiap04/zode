---
title: Subprocessor List
slug: subprocessors
---

**Last updated**: this document was rewritten for the Zode fork; it no longer describes
Zed Industries, Inc.'s hosted service.

## There are no subprocessors

A subprocessor is a third party a service provider uses to process *your data* on its
behalf. Zode isn't a service — there's no company operating it, no account, and no user
data collected in the first place. With nothing collected, there's nothing to hand to a
subprocessor, so this list is empty.

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
