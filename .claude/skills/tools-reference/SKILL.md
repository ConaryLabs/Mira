# Mira Consolidated Tools Reference

Mira uses 9 action-based tools. Reference for tool signatures and workflows.

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
memory(action="forget", id="42")
memory(action="archive", id="42")                         # Exclude from auto-export
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
session(action="history", history_action="current")
session(action="history", history_action="list_sessions", limit=5)
session(action="history", history_action="get_history", session_id="...")
session(action="recap")                     # Quick overview: goals, sessions, insights
session(action="usage", usage_action="summary", since_days=7)
session(action="usage", usage_action="stats", group_by="provider_model")
session(action="usage", usage_action="list", limit=50)
session(action="insights", insight_source="pondering", min_confidence=0.5)
session(action="tasks", tasks_action="list")          # Show running/completed tasks
session(action="tasks", tasks_action="get", task_id="abc123")  # Get task status
session(action="tasks", tasks_action="cancel", task_id="abc123")  # Cancel task
```

## `goal` — Cross-Session Goal Tracking

```
goal(action="create", title="...", description="...", priority="high")
goal(action="bulk_create", goals='[{"title": "...", "priority": "medium"}]')
goal(action="list", include_finished=false, limit=10)
goal(action="get", goal_id="1")
goal(action="update", goal_id="1", status="in_progress")
goal(action="delete", goal_id="1")
goal(action="add_milestone", goal_id="1", milestone_title="...", weight=2)
goal(action="complete_milestone", milestone_id="1")
goal(action="delete_milestone", milestone_id="1")
goal(action="progress", goal_id="1")
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

## Other Tools

```
reply_to_mira(in_reply_to="msg_id", content="...", complete=true)
```
