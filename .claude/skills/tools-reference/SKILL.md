---
name: tools-reference
description: Reference for all Mira MCP tool signatures, parameters, and workflows.
---

# Mira Consolidated Tools Reference

Mira uses 10 action-based tools. Reference for tool signatures and workflows.

## `project` — Project/Session Management

```
project(action="start", project_path="...", name="...")  # Initialize session
project(action="set", project_path="...", name="...")    # Change active project
project(action="get")                                     # Show current project
```

## `memory` — Persistent Memory

```
memory(action="remember", content="...", fact_type="decision", category="...")
memory(action="recall", query="...", limit=10, category="...", fact_type="...")
memory(action="forget", id=42)
memory(action="archive", id=42)                            # Exclude from auto-export
```

## `code` — Code Intelligence

```
code(action="search", query="authentication middleware", limit=10)
code(action="symbols", file_path="src/main.rs", symbol_type="function")
code(action="callers", function_name="handle_login", limit=20)
code(action="callees", function_name="process_request", limit=20)
code(action="dependencies")                 # Module dependency graph
code(action="patterns")                     # Architectural pattern detection
code(action="tech_debt")                    # Per-module tech debt scores
code(action="diff")                         # Auto-detects staged/working/last commit
code(action="diff", from_ref="main", to_ref="feature/x")
code(action="diff", from_ref="v1.0", to_ref="v1.1", include_impact=true)
```

## `session` — Session Management & Analytics

```
session(action="current_session")                          # Show current session
session(action="list_sessions", limit=5)                   # List recent sessions
session(action="get_history", session_id="...")             # Get session history
session(action="recap")                                    # Quick overview: goals, sessions, insights
session(action="usage_summary", since_days=7)              # LLM usage summary
session(action="usage_stats", group_by="provider_model")   # LLM usage by dimension
session(action="usage_list", limit=50)                     # Recent LLM usage records
session(action="insights", insight_source="pondering", min_confidence=0.5)
session(action="dismiss_insight", insight_id=42)           # Remove resolved insight
session(action="tasks_list")                               # Show running/completed tasks
session(action="tasks_get", task_id="abc123")              # Get task status
session(action="tasks_cancel", task_id="abc123")           # Cancel task
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

## `documentation` — Documentation Management

```
documentation(action="list", status="pending", priority="high")
documentation(action="get", task_id=123)    # Get task details + writing guidelines
documentation(action="complete", task_id=123)
documentation(action="skip", task_id=123, reason="...")
documentation(action="inventory")           # Show all existing docs
documentation(action="scan")               # Trigger documentation scan
```

**Workflow:** list → get → (write docs) → complete

## `index` — Code Indexing & Health

```
index(action="project")                     # Full project index (auto-enqueues as background task)
index(action="project", skip_embed=true)    # Fast re-index without embeddings
index(action="file", path="src/main.rs")
index(action="status")                      # Show index statistics
index(action="compact")                     # VACUUM vec tables
index(action="summarize")                   # Generate module summaries
index(action="health")                      # Full code health scan (background task)
```

## `team` — Team Intelligence

```
team(action="status")                       # Team overview: members, files, conflicts
team(action="review", teammate="agent-1")   # Review a teammate's modified files
team(action="distill")                      # Extract key findings into team-scoped memories
```

## `recipe` — Team Recipes

```
recipe(action="list")                              # List available recipes
recipe(action="get", name="expert-review")         # Get full recipe details
recipe(action="get", name="full-cycle")
```

## Other Tools

```
reply_to_mira(in_reply_to="msg_id", content="...", complete=true)
```
