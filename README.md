# FayeDark Agent Kit

AI-powered development kit for coding agents (Claude Code, OpenCode). Specialized subagents handle planning, implementation, testing, and review.

## Quick Start

### Install CLI from a private GitHub Release

Requirements:
- Node.js 18+
- GitHub CLI authenticated with repo access: `gh auth login`

```bash
# Download the packaged CLI from the latest private release
gh release download --repo cychipo/fd-kit --pattern "fayedark-kit-*.tgz"

# Install globally from the downloaded tarball
npm install -g ./fayedark-kit-*.tgz

# Verify installation
fdk --help
```

Alternative global install commands:

```bash
pnpm add -g ./fayedark-kit-*.tgz
# or
yarn global add file:./fayedark-kit-*.tgz
# or
bun add -g ./fayedark-kit-*.tgz
```

For project setup from the local monorepo:

```bash
cd ../my-project
fdk init --local
```

Or point directly at a local kit directory:

```bash
fdk init --kit-path /path/to/agent-kit/claude
```

### Initialize a project

```bash
# From GitHub release (recommended for installed users)
fdk init

# From local monorepo (recommended when developing this repo)
fdk init --local

# Explicit local kit path
fdk init --kit-path /path/to/agent-kit/claude
```

### Core commands

```bash
fdk doctor         # Health check
fdk skills --list  # List installed skills
fdk config show    # View configuration
fdk update         # Update CLI, then offer to refresh project kit with fdk init
```

## Repo Layout

This is a **monorepo** containing the kit and its CLI:

```
agent-kit/
├── claude/           # Kit source (shipped as .claude/ after install)
│   ├── agents/       # 16 specialized agents
│   ├── skills/       # 80+ skills
│   ├── commands/fdk/  # /fdk: slash commands
│   ├── hooks/        # Lifecycle hooks
│   ├── rules/        # Workflow rules
│   ├── output-styles/
│   ├── schemas/
│   ├── session-state/
│   ├── templates/
│   ├── metadata.json
│   └── settings.json
├── cli/              # fayedark-agent-kit-cli (published to npm, CLI binary: `fdk`)
├── scripts/          # Build scripts (prepare-release-assets.cjs)
├── docs/             # Documentation
├── tests/, evals/    # Dev-only
└── .github/workflows/release.yml
```

## Update workflow

When a new release is published:

```bash
fdk update
```

This updates the global CLI first, then hands off to `fdk init` so the `.claude/` kit inside each project can be refreshed.

## Claude workflow after installation

After the kit is installed, use these slash commands inside Claude Code:

```
/fdk:plan → /fdk:cook → /fdk:code-review → /fdk:ship
```

Claude orchestrates specialized subagents:

| Agent | Role |
|-------|------|
| `planner` | Architecture breakdown, phase planning |
| `implementer` | Code implementation per phase |
| `tester` | Unit + integration tests, coverage |
| `reviewer` | Code quality, spec compliance |
| `debugger` | Root-cause investigation |
| `researcher` | Technical research (parallel) |
| `doc-writer` | Documentation updates |
| `git-manager` | Git operations, commits |

See `.claude/rules/primary-workflow.md` for the full protocol.

## Common Claude slash commands

These run inside Claude Code after `fdk init` has installed the kit:

```text
/fdk:plan Build a task management app
/fdk:brainstorm Choose stack for real-time collaboration
/fdk:fix Authentication bug when logging in with Google
/fdk:code-review
/fdk:ship
```

## Plan Format

```
plans/
└── 260409-1226-task-management-app/
    ├── plan.md
    ├── phase-01-setup.md
    ├── phase-02-database.md
    └── phase-03-implementation.md
```

## Documentation

- [Installation Guide](./docs/INSTALL.md)
- [Development Guide](./docs/DEV.md)
- [User Guide](./docs/GUIDE.md)
- [Code Standards](./docs/code-standards.md)
- [System Architecture](./docs/system-architecture.md)

## Contributing

1. Fork and create a feature branch
2. Follow `.claude/rules/development-rules.md`
3. Run `fdk doctor` to verify setup
4. Submit PR

## License

Internal use — FayeDark.
