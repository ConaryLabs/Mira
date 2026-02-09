# recipe

Get reusable team recipes for common workflows. Recipes define team blueprints for Claude Code Agent Teams, including member roles, prompts, tasks, and coordination instructions.

Recipes are static data (not stored in the database). They provide structured blueprints that the team lead uses to spawn and coordinate Agent Teams.

## Usage

```json
{
  "name": "recipe",
  "arguments": {
    "action": "list"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `list` or `get` |
| name | String | For `get` | Recipe name to retrieve |

## Actions

### `list` — List available recipes

Returns all built-in recipes with name, description, and member count.

```json
{ "action": "list" }
```

**Response:**

```json
{
  "action": "list",
  "message": "2 recipe(s) available.",
  "data": {
    "recipes": [
      { "name": "expert-review", "description": "Multi-expert code review...", "member_count": 6 },
      { "name": "full-cycle", "description": "End-to-end review and implementation...", "member_count": 8 }
    ]
  }
}
```

### `get` — Get full recipe details

Returns complete recipe including member prompts, task definitions, and coordination instructions.

```json
{ "action": "get", "name": "expert-review" }
```

**Response includes:**

- `name` — Recipe identifier
- `description` — What the recipe does
- `members` — Array of `{ name, agent_type, prompt }` for each team member
- `tasks` — Array of `{ subject, description, assignee }` for each task
- `coordination` — Markdown instructions for the team lead on how to run the recipe

## Built-in Recipes

### `expert-review`

Multi-expert code review with 6 roles: architect, code-reviewer, security, scope-analyst, ux-strategist, and plan-reviewer. All experts run in parallel, explore the codebase read-only, and report findings to the team lead.

### `full-cycle`

End-to-end review, implementation, and QA cycle with 8 members across 4 phases:
1. **Discovery** — 6 experts analyze the codebase in parallel
2. **Synthesis + Implementation** — Team lead synthesizes findings, spawns implementation agents
3. **QA** — test-runner and ux-reviewer verify changes
4. **Finalize** — Final build verification and cleanup

## Errors

| Error | Cause |
|-------|-------|
| `name is required for recipe(action=get)` | Missing `name` parameter on `get` action |
| `Recipe 'X' not found. Available: ...` | Invalid recipe name |

## See Also

- [**team**](./team.md): Team intelligence for active Agent Teams sessions
- [**goal**](./goal.md): Cross-session goal tracking
