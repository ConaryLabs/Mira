<!-- docs/tools/recipe.md -->
# Recipe

> **This entire tool is CLI-only.** All actions are available via `mira tool recipe '<json>'` but are not exposed as MCP tools.

Get reusable team recipes for common workflows. Recipes define team blueprints for Claude Code Agent Teams, including member roles, prompts, tasks, and coordination instructions.

Recipes are static data (not stored in the database). They provide structured blueprints that the team lead uses to spawn and coordinate Agent Teams.

## Actions

### list

List all available recipes with names, descriptions, member counts, and usage guidance.

**Parameters:**
- `action` (string, required) - `"list"`

**Returns:** Array of recipes with `name`, `description`, `member_count`, and `use_when`.

### get

Get full recipe details including member prompts, task definitions, and coordination instructions.

**Parameters:**
- `action` (string, required) - `"get"`
- `name` (string, required) - Recipe name (case-insensitive)

**Returns:** Complete recipe with `name`, `description`, `members` (name, agent_type, prompt), `tasks` (subject, description, assignee), and `coordination` (markdown instructions for the team lead).

## Built-in Recipes

### expert-review

Multi-expert code review with 6 roles: architect, code-reviewer, security, scope-analyst, ux-strategist, and plan-reviewer. All experts run in parallel, explore the codebase read-only, and report findings to the team lead.

### full-cycle

End-to-end review, implementation, and QA cycle with 8 members across 5 phases:
1. Discovery -- 6 experts analyze the codebase in parallel
2. Synthesis -- Team lead synthesizes findings
3. Implementation -- Spawns implementation agents
4. QA -- test-runner and ux-reviewer verify changes
5. Finalize -- Final build verification and cleanup

### qa-hardening

Production readiness review with 5 read-only agents: test-runner, error-auditor, security, edge-case-hunter, and ux-reviewer. Produces a prioritized hardening backlog (critical/high/medium/low). No implementation phase -- strictly diagnostic.

### refactor

Safe code restructuring with 3 agents across 5 phases: architect designs the migration plan, code-reviewer validates safety, implementation executes step-by-step with per-step compilation checks, and test-runner verifies behavior is preserved. Each refactoring step must compile independently.

## Examples

```json
{"action": "list"}
```

```json
{"action": "get", "name": "expert-review"}
```

```json
{"action": "get", "name": "qa-hardening"}
```

## Errors

- **"name is required"** - `get` needs a recipe name; error includes list of available recipes
- **"Recipe not found"** - Invalid recipe name; error includes list of available recipes

## See Also

- [team](./team.md) - Team intelligence for active Agent Teams sessions
