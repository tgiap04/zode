<!-- layout-exempt: rebuild-spec owns all docs/system|features|generated|flows paths -->

# Business Context — F013_WorkspaceAndProjectManagement

## Why It Matters

Developers routinely keep several projects open at once — a main app, a service it talks to, a docs repo. This feature lets Zode hold all of them open in one window at once (this fork's signature capability) while automatically putting the ones you're not looking at to sleep, so you don't pay a memory and battery cost for projects sitting idle in the background. It also covers the everyday mechanics of getting into and around a project: reopening something you worked on recently, browsing and editing its file tree, and — for teams that want a fully reproducible setup — spinning up a containerized development environment automatically.

## Who Uses It

- **Individual developer juggling several projects** — switches between a frontend repo, a backend repo, and a docs repo in one window without manually closing and reopening any of them.
- **Developer returning after a break** — reopens a project from the welcome screen's recent list instead of hunting for the folder again.
- **Developer working across many files in one project** — browses and edits the file tree from the keyboard, creates new files without leaving the editor.
- **Team using a standardized dev environment** — sets up a containerized environment once (a "dev container") so every teammate who opens the project gets the same tools and versions automatically.
- **Security-conscious user** — is asked to explicitly approve a new folder before Zode runs any language tooling or certain git operations against it, so opening an untrusted folder can't silently execute anything.

## What They Do

1. A developer opens Zode and sees a welcome screen listing recently-used projects; they click one to reopen it exactly where they left off.
2. As they open additional projects, each one appears as an entry in an always-visible project rail along the side of the window; clicking an entry — or using a keyboard shortcut to move to the next/previous one — switches which project's files and editors are shown.
3. If a project sits unused in the background for a while, it quietly winds down its background work (language help, format-on-save, file watching) to save memory — the rail entry shows a small "hibernated" indicator so the developer knows it's asleep, not gone.
4. The moment the developer clicks back into that project, it wakes back up automatically and becomes fully usable again — nothing has to be manually restarted or reopened.
5. Within a project, the developer browses its file tree in a sidebar panel, creates new files directly from there, and can remove a git worktree they no longer need without deleting anything from disk.
6. If a team wants a shared, reproducible development setup, someone runs a short wizard to generate a dev-container configuration for the project; from then on, opening that project automatically builds and starts the containerized environment and runs its setup scripts, so nobody has to run manual container commands by hand.
7. If a developer opens a folder Zode hasn't seen before, it asks for explicit permission before running any language tooling or certain git operations against it, protecting against silently executing code from an untrusted source.

## Unresolved Questions

- **How long before a project is considered "idle"?**: the exact default wait time (and the memory-pressure threshold that can hibernate a project sooner under low-memory conditions) are configurable, but the shipped default values weren't confirmed from a plain-language source in this pass.
- **Is the git-worktree cleanup action meant to be the same concept as the project's own file-tree indexing, or two separate ideas that happen to share the word "worktree"?**: the code treats them as unrelated; whether that's intentional from a product-naming standpoint isn't determinable without the original team's input.
