# Changelog

One file per release, `v<major>.<minor>.<patch>.md`, written for people who use Zode. These files
are the release notes everywhere: the GitHub release body is built from them, the website shows
them at zodekit.com/changelog, and Zode's "View Release Notes" opens them there.

**A stable release needs its file.** The release workflow stops if `docs/changelog/v<version>.md`
is missing. Nightly and preview builds do not need one.

## Format

```md
---
version: 0.1.11
date: 2026-12-01
title: A short title, 60 characters at most
---

## Highlights

- Two to five things a user would want to hear about, in plain words.

## Highlights (vi)

- The same, in natural Vietnamese (not a word-for-word translation).

## Changes

### New

- ...

### Improved

- ...

### Fixed

- ...
```

- `date` is the day the release is published.
- Leave out an empty group under `## Changes`.
- Leave out tests, CI, chores and refactors nobody can see.
- Never paste commit subjects as they are; say what changed for the user.
- After a release, run `npm run changelog:sync` in the web repo to publish the page.
