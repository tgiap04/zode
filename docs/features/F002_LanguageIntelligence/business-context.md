# Business Context — F002_LanguageIntelligence

## Why It Matters

Every smart editing feature — error squiggles, autocomplete, jump-to-definition, formatting — depends on a background helper program (a "language server") that understands the code being edited. This feature is the plumbing that starts those helpers, keeps them running, lets a developer correct them when they guess wrong, and formats code automatically. Without it, the editor would just be a plain text box.

## Who Uses It

- **Developer editing code** — benefits automatically every time they open a file; the correct language helper starts itself in the background without any action needed.
- **Developer working across mixed setups** — someone with several versions of Python, Node, or another toolchain installed uses the toolchain picker to tell the editor exactly which one to analyze their code with.
- **Developer whose file was misdetected** — someone whose file extension doesn't match its real content (e.g. Rust code saved as `.txt`) uses the language picker to correct it manually.
- **Developer recovering from a stuck helper** — someone whose language helper has become unresponsive restarts it without needing to close and reopen the whole editor.

## What They Do

1. A developer opens a project folder — before any language helper spawns, the project must first be marked as trusted, protecting the developer from a folder silently running arbitrary tooling on open.
2. The developer opens or edits a file in a recognized language — the matching language helper starts automatically in the background and begins analyzing the code.
3. If the developer has multiple versions of an interpreter or SDK installed, they open the toolchain picker from the status bar and choose the one they want; that choice is remembered for next time and the helper immediately re-analyzes using it.
4. If a file's language was guessed incorrectly, the developer opens the language picker and sets the correct one; highlighting and analysis update right away.
5. If a language helper stops responding, the developer triggers a restart action, and the helper is torn down and relaunched without losing their open files.
6. Separately, when the developer saves or formats a file, a formatting helper (Prettier) starts the same way — preferring a project-specific install, or installing a shared default automatically the first time it's needed.

## Unresolved Questions

- **Toolchain override reliability**: if the developer's choice fails to save (a background storage hiccup), the editor still applies it for the current session but doesn't tell the developer the save failed — is silent-but-functional acceptable, or should this surface a notice?
- **Scope of this feature**: the underlying documentation groups notebook/Python-kernel execution (Jupyter-style "run this cell") into the same feature as language-helper management, even though from a user's perspective these feel like two different capabilities (code intelligence vs. running notebooks). Worth confirming with stakeholders whether that's the intended grouping.
