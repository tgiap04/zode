# Business Context — F016_Search

## Why It Matters

Developers spend a huge share of their time just locating things — a file by half-remembered
name, a string or symbol scattered across dozens of files, or the next occurrence of a term in
the file already open. Search gives them three purpose-built ways to do that instead of one
generic tool trying to do everything, and it makes the whole-project results directly editable
so finding and fixing a problem is one motion, not two.

## Who Uses It

- **Developer opening a file** — types a fragment of a filename to jump straight to it, instead of clicking through folders.
- **Developer investigating usage** — searches a term or symbol across the entire project to see everywhere it appears, with enough surrounding text to judge each hit without opening every file.
- **Developer editing the file in front of them** — searches within the open file to jump between repeated occurrences of a term, and optionally replaces some or all of them.
- **Developer running any command** — every command they confirm through the command palette is quietly remembered, so commands they use often surface faster next time.

## What They Do

1. A developer wanting to open a file types a partial, even out-of-order fragment of its name into the file finder — the best-matching files appear ranked, and confirming the top one opens it.
2. If nothing matches an existing file, the finder offers to create a new file at that path instead of leaving the developer with an empty list.
3. A developer wanting every usage of a term submits a project-wide search — matching lines from every affected file appear together, with surrounding context, and the results can be edited directly in place just like a normal file.
4. If a project is unusually large and a search would return far more matches than can be shown reasonably, the results view says so plainly (a "+" on the count) instead of pretending the count is exact.
5. A developer working in one file searches within it, sees every occurrence highlighted, and steps between them one at a time — optionally replacing one occurrence or all of them at once.
6. Every time a developer confirms a command from the command palette, that choice is quietly logged in the background so the palette can better predict which commands that developer reaches for most.

## Unresolved Questions

- **Search history persistence**: is it acceptable that recent search queries (project search, buffer search) are lost when the app restarts, or should they persist across sessions the way command-palette usage does?
- **Result-cap thresholds**: are the fixed caps on fuzzy-finder results (100) and whole-project search (5,000 files / 10,000 matches) the right limits for very large projects, or should they scale with project size / be user-configurable?
