---
title: Third-Party Dependencies
slug: third-party-terms
---

**Last updated**: this document was rewritten for the Zode fork; it no longer describes
Zed Industries, Inc.'s hosted service or its AI-provider integrations (none of which
exist in this fork).

## Extension registry — a disclosed dependency on Zed Industries

Zode's extension browsing and installation still queries and downloads from Zed
Industries' extension registry (`api.zed.dev`). This is a deliberate, disclosed choice,
not an oversight: dropping the registry would mean either running an independent
marketplace (real ongoing infrastructure and moderation work) or losing the extension
ecosystem entirely. For now, this fork keeps the registry and discloses the dependency
honestly instead.

What this means in practice:

- Browsing or installing an extension sends a request to `api.zed.dev`, carrying
  whatever any HTTP request carries (your IP address, the extension you're requesting).
  No other information about you or your project is sent.
- This is a genuine third-party dependency: Zed Industries operates that API, can
  change or discontinue it, and its own terms govern that service, not this document.
  See [Zed's Terms of Service](https://zed.dev/terms) for the registry itself.
- This dependency did not exist as a disclosed choice before this document — see
  `research/network-verification.md` in this fork's development history for how it was
  found and verified.

If you'd rather not depend on Zed Industries' infrastructure at all, don't use the
Extensions panel — nothing else in Zode reaches it.

## Language servers — downloaded from their own distributors

When Zode auto-downloads a language server for a language you've opened a file in, that
download comes directly from the language server's own project (its GitHub releases, a
package registry, etc.) — never through Zed Industries or any server this fork
operates. Each language server is subject to its own license and its own distributor's
terms, which this document can't enumerate exhaustively since it depends entirely on
which languages you use.

## Everything Zed Industries used to integrate is gone

Earlier versions of this document (inherited from upstream Zed) listed acceptable-use
policies for Anthropic, GitHub Copilot, Google, OpenAI, OpenRouter, Vercel, and xAI —
all AI providers reachable through Zed's AI features. None of those integrations exist
in this fork; the `agent`, `language_models`, and `edit_prediction` subsystems were
removed. There is nothing left in Zode that talks to any of those providers.
