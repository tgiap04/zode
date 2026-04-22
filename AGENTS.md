# AGENTS.md

This file provides guidance to OpenCode when working with code in this repository.

## Project Overview

**Name:** FayeDark Agent Kit
**Type:** AI development kit for coding agents (Claude Code, OpenCode)
**Repo:** `cychipo/fd-kit`

## Role & Responsibilities

Analyze user requirements, delegate tasks to appropriate sub-agents, and ensure cohesive delivery of features that meet specifications and architectural standards.

## Workflows

- Primary workflow: `./.claude/rules/primary-workflow.md`
- Development rules: `./.claude/rules/development-rules.md`
- Orchestration protocols: `./.claude/rules/orchestration-protocol.md`
- Documentation management: `./.claude/rules/documentation-management.md`
- And other workflows: `./.claude/rules/*`

## Development Principles

- **YAGNI**: You Aren't Gonna Need It
- **KISS**: Keep It Simple, Stupid
- **DRY**: Don't Repeat Yourself

## Repo Structure

```
agent-kit/
├── claude/           # Kit source (shipped as .claude/ after install)
│   ├── agents/       # 16 specialized agents
│   ├── skills/       # 80+ skills
│   ├── commands/fdk/  # Slash commands
│   ├── hooks/        # Lifecycle hooks
│   └── rules/        # Workflow rules
├── cli/              # CLI tool (published as fayedark-agent-kit-cli)
├── docs/             # Documentation
└── scripts/          # Build scripts
```

## Documentation

```
./docs
├── INSTALL.md
├── DEV.md
├── GUIDE.md
├── GUIDE-VI.md
├── code-standards.md
└── system-architecture.md
```

## External Files

Reference external instruction files in `opencode.json`:

```json
{
  "instructions": ["docs/*.md", ".opencode/agents/*.md"]
}
```
