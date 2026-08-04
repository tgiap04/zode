---
title: Terms of Service
slug: terms
---

**Last updated**: this document was rewritten for the Zode fork; it no longer describes
Zed Industries, Inc.'s hosted service, subscription, or its original 40+ page Terms of
Service.

## There is no service, so there are no Terms of Service

Upstream Zed's Terms of Service governed a hosted subscription service: accounts,
billing, a collaboration server, hosted AI features. This fork removed all of that —
there is no service to have terms for. Zode is software you run locally.

## What actually governs your use of Zode

The software license does:

- **[LICENSE-GPL](/LICENSE-GPL)** (GPL-3.0-or-later) covers the application code —
  `zed`, `editor`, `project`, `workspace`, `client`, and most of the rest of the crates
  in this repository.
- **[LICENSE-APACHE](/LICENSE-APACHE)** (Apache-2.0) covers `gpui` and a handful of
  related crates.
- **[NOTICE](/NOTICE)** credits the upstream Zed project this fork comes from.

Those licenses grant you the rights to run, study, modify, and redistribute this
software, and they're the only legal terms that apply to using it. There is no
Contributor License Agreement, no Code of Conduct tied to a hosted forum (this isn't a
hosted product), and no arbitration clause, because there is no company on the other
side of this software to have one with.

## The one exception: the extension registry

Installing an extension reaches Zed Industries' extension registry
(`api.zed.dev`) — a disclosed, retained third-party dependency, not part of this
software's own terms. See `legal/third-party-terms.md` for what that means and
[Zed's own Terms of Service](https://zed.dev/terms) for the registry service itself.
Everything else in Zode runs with no service, and therefore no terms, behind it.
