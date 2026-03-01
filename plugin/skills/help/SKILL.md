<!-- plugin/skills/help/SKILL.md -->
---
name: help
description: This skill should be used when the user asks "help", "what commands", "list commands", "what can mira do", "mira help", "show commands", "what can you do", "how do I use mira", or wants to see all available Mira skills and tools.
---

# Mira Commands

## Getting Started

| Command | Description |
|---------|-------------|
| `/mira:help` | Show all available commands and tools |
| `/mira:status` | Quick health check: index stats, storage, active goals |
| `/mira:recap` | Get session context, preferences, and active goals |

## Daily Use

| Command | Description |
|---------|-------------|
| `/mira:search <query>` | Semantic code search — find code by meaning, not just text |
| `/mira:goals` | Track cross-session objectives with milestones |
| `/mira:diff` | Semantic analysis of git changes with impact assessment |
| `/mira:insights` | Surface background analysis and predictions |

## Power User

| Command | Description |
|---------|-------------|
| `/mira:experts` | Expert consultation via Agent Teams |
| `/mira:full-cycle` | End-to-end expert review with implementation and QA |
| `/mira:qa-hardening` | Production readiness review and hardening backlog |
| `/mira:refactor` | Safe code restructuring with architect and reviewer validation |

## MCP Tools

Beyond slash commands, Mira provides MCP tools that Claude uses automatically:

`code`, `diff`, `project`, `session`, `insights`, `goal`, `index`, `launch`

Note: `team` is available via `mira tool team ...` (CLI only, not exposed as an MCP tool).

These power semantic search, call graph analysis, persistent memory, and more — no slash command needed.

## Tool Dependencies

Some tools require indexing before they work. Here's what depends on what.

### Requires code index (`index(action="project")`)

| Tool | Notes |
|------|-------|
| `code(action="search")` | Semantic search over indexed code |
| `code(action="callers")` / `code(action="callees")` | Call graph analysis |
| `code(action="bundle")` | Module extraction |
| `diff(include_impact=true)` | Impact analysis of changes |

### Requires health scan (`index(action="health")`)

| Tool | Notes |
|------|-------|
| `code(action="dependencies")` | Auto-queues health scan if missing |
| `code(action="dead_code")` | Auto-queues health scan if missing |
| `insights()` | Needs health data for analysis |

### Works without indexing

| Tool | Notes |
|------|-------|
| `project(*)`, `session(*)`, `goal(*)` | Session and goal management |
| `code(action="symbols")` | Single-file parsing via tree-sitter |
| `diff()` (without impact) | Git-based analysis only |

Note: `project(action="start")` auto-triggers background indexing, so most users never need to manually index.

## Instructions

Present the command reference above. If the user seems new to Mira, highlight `/mira:status` and `/mira:recap` as good starting points.

## Tip

New session? Run `/mira:recap` to restore context from previous work.
Quick health check? Run `/mira:status` to see index stats, storage, and active goals.
