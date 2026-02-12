<!-- docs/modules/mira-server/tools/core/recipe.md -->
# tools/core/recipe

Reusable team workflow recipes for Agent Teams. Defines static blueprints that configure multi-agent teams with roles, tasks, and coordination instructions.

## Overview

Recipes are statically defined team configurations that can be instantiated via Agent Teams. Each recipe specifies a set of named agents with role-specific prompts, a task list with assignments, and coordination instructions describing the workflow phases. Recipes are not stored in the database -- they are compiled into the binary as constants.

## Built-in Recipes

| Recipe | Members | Purpose |
|--------|---------|---------|
| `expert-review` | 6 | Multi-expert code review (architect, security, performance, etc.) |
| `full-cycle` | 8 | End-to-end review with discovery phase + QA phase |
| `qa-hardening` | 5 | Production readiness review and hardening backlog |
| `refactor` | 3 | Safe code restructuring with architect + reviewer validation |

## Key Functions

- `handle_recipe()` - MCP tool dispatcher for `recipe(action=list)` and `recipe(action=get)`

## Sub-modules

| Module | Purpose |
|--------|---------|
| `expert_review` | Expert review recipe definition |
| `full_cycle` | Full-cycle recipe definition |
| `qa_hardening` | QA hardening recipe definition |
| `refactor` | Refactor recipe definition |
| `prompts` | Shared prompt fragments for agent roles |

## Architecture Notes

Recipe lookup is case-insensitive. The `action_get` response includes full member prompts and task descriptions, which Claude Code uses to configure Agent Teams via the `/mira:experts`, `/mira:full-cycle`, `/mira:qa-hardening`, and `/mira:refactor` slash commands.
