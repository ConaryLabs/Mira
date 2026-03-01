---
name: tools-reference
description: Reference for all Mira MCP tool signatures, parameters, and workflows.
---

<!-- .claude/skills/tools-reference/SKILL.md -->

# Mira Consolidated Tools Reference

Mira exposes 8 MCP tools to Claude Code. Additional actions are available via the `mira tool` CLI.

## `project` — Project/Session Management

```
project(action="start", project_path="...", name="...")  # Initialize session
project(action="get")                                     # Show current project
```

## `code` — Code Intelligence

```
code(action="search", query="authentication middleware", limit=10)
code(action="symbols", file_path="src/main.rs", symbol_type="function")
code(action="callers", function_name="handle_login", limit=20)
code(action="callees", function_name="process_request", limit=20)
```

## `diff` — Semantic Diff Analysis

```
diff()                                      # Auto-detects staged/working/last commit
diff(from_ref="main", to_ref="feature/x")
diff(from_ref="v1.0", to_ref="v1.1", include_impact=true)
```

## `session` — Session Management

```
session(action="current_session")                          # Show current session
session(action="recap")                                    # Quick overview: goals, sessions, insights
```

## `insights` — Background Analysis & Insights

```
insights(action="insights", insight_source="pondering", min_confidence=0.5)
insights(action="dismiss_insight", insight_id=42)          # Remove resolved insight
```

## `goal` — Cross-Session Goal Tracking

```
goal(action="create", title="...", description="...", priority="high")
goal(action="bulk_create", goals='[{"title": "...", "priority": "medium"}]')
goal(action="list", include_finished=false, limit=10)
goal(action="get", goal_id=1)
goal(action="update", goal_id=1, status="in_progress")
goal(action="delete", goal_id=1)
goal(action="add_milestone", goal_id=1, milestone_title="...", weight=2)
goal(action="complete_milestone", milestone_id=1)
goal(action="delete_milestone", milestone_id=1)
goal(action="progress", goal_id=1)
```

## `index` — Code Indexing

```
index(action="project")                     # Full project index (auto-enqueues as background task)
index(action="project", skip_embed=true)    # Fast re-index without embeddings
index(action="file", path="src/main.rs")
index(action="status")                      # Show index statistics
```

## `launch` — Context-Aware Team Launcher

```
launch(team="expert-review-team")                            # Parse agent file, enrich with project context
launch(team="expert-review-team", members="nadia,jiro")      # Filter to specific members
launch(team="qa-hardening-team", scope="src/tools/")         # Scope code context to a path
launch(team="refactor-team", context_budget=8000)            # Custom context budget (default: 4000)
```

Returns `LaunchData` with pre-assembled agent specs (name, role, model, prompt, task_subject, task_description), shared project context, and suggested team ID. Does not spawn agents -- Claude orchestrates with TeamCreate/TaskCreate/Task.

## CLI-Only Actions

The following actions are available via `mira tool <name> '<json>'` but not exposed as MCP tools:

| Tool | Action | Purpose |
|------|--------|---------|
| `project` | `set` | Change active project without full init |
| `memory` | `export_claude_local` | Export memories to CLAUDE.local.md |
| `code` | `dependencies` | Module dependency graph |
| `code` | `patterns` | Architectural pattern detection |
| `code` | `tech_debt` | Per-module tech debt scores |
| `index` | `compact` | VACUUM vec tables |
| `index` | `summarize` | Generate module summaries |
| `index` | `health` | Full code health scan |
| `session` | `list_sessions` | List recent sessions |
| `session` | `get_history` | View session tool history |
| `session` | `usage_summary` | LLM usage totals |
| `session` | `usage_stats` | LLM usage by dimension |
| `session` | `usage_list` | Recent LLM usage records |
| `session` | `tasks_list` | List background tasks |
| `session` | `tasks_get` | Get background task status |
| `session` | `tasks_cancel` | Cancel a background task |
| `session` | `storage_status` | Database size and retention |
| `session` | `cleanup` | Run data cleanup |
| `documentation` | *(all actions)* | Documentation gap management |
| `team` | *(all actions)* | Agent Teams intelligence |
