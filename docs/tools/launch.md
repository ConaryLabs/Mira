<!-- docs/tools/launch.md -->
# Launch

Context-aware team launcher. Parses `.claude/agents/{team}.md` files, enriches agent prompts with project context (type, goals, code bundle), and returns ready-to-spawn agent specs.

Returns data only -- does not spawn agents or create teams. Claude orchestrates using TeamCreate, TaskCreate, and Task tools.

## Parameters

- `team` (string, required) - Agent team file name (e.g. `"expert-review-team"`). Resolves to `.claude/agents/{team}.md`
- `scope` (string, optional) - Scope for code context enrichment: file path, module path, or concept
- `members` (string, optional) - Filter to specific members by first name, comma-separated (e.g. `"nadia,jiro"`)
- `context_budget` (integer, optional) - Context budget in characters (default: 4000, min: 500, max: 20000)

## Response

Returns `LaunchData`:

- `team_name` - Team name from frontmatter
- `team_description` - Team description from frontmatter
- `agents` - Array of `AgentSpec`:
  - `name` - Agent name (lowercase, e.g. `"nadia"`)
  - `role` - Role title (e.g. `"Systems Architect"`)
  - `read_only` - Whether agent is read-only (derived from Tools field)
  - `model` - Suggested model (`"sonnet"` for read-only, empty for default)
  - `prompt` - Pre-assembled prompt: persona + focus + project context
  - `task_subject` - Suggested subject for TaskCreate
  - `task_description` - Suggested description for TaskCreate
- `project_context` - Shared project context block (type, goals, code bundle)
- `suggested_team_id` - Timestamped team name for TeamCreate

## Agent File Format

Agent files use YAML frontmatter and H3 sections:

```markdown
---
name: expert-review-team
description: Expert review team for code analysis
---

### Nadia -- Systems Architect

**Personality:** Thinks in systems...

**Focus:** Design patterns, API design, coupling

**Tools:** Read-only (Glob, Grep, Read)
```

Dynamic agents (heading contains "dynamic") are excluded from the spawn list.

## Examples

```json
{"team": "expert-review-team"}
```

```json
{"team": "expert-review-team", "members": "nadia,jiro"}
```

```json
{"team": "qa-hardening-team", "scope": "src/tools/", "context_budget": 8000}
```

## See Also

- [team](./team.md) - Team status and review
