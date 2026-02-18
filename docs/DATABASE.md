# Mira Database Schema

Mira uses two SQLite databases with the sqlite-vec extension for vector search.

| Database | Path | Purpose |
|----------|------|---------|
| **Main** | `~/.mira/mira.db` | Memories, sessions, goals, proactive intelligence |
| **Code Index** | `~/.mira/mira-code.db` | Code symbols, call graph, embeddings, FTS |

The code index was separated from the main database in v0.3.5 to eliminate write contention - indexing operations no longer block tool calls. Each database has its own `DatabasePool` and WAL mode configuration.

---

## Core Tables

> Tables below are in the **main database** (`mira.db`) unless marked otherwise.

### projects

Project registry for multi-project support.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| path | TEXT UNIQUE | Filesystem path to project root |
| name | TEXT | Display name |
| created_at | TEXT | Timestamp |

### memory_facts

Semantic memory storage with evidence-based confidence tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Optional project scope |
| user_id | TEXT | User who created (for multi-user) |
| team_id | INTEGER | Team scope (for sharing) |
| scope | TEXT | `project`, `personal`, or `team` |
| key | TEXT | Unique key for upsert operations |
| content | TEXT | The fact/preference content |
| fact_type | TEXT | `general`, `preference`, `decision`, `context`, `pattern`, `persona` |
| category | TEXT | Optional grouping (e.g., `coding`, `tooling`) |
| confidence | REAL | 0.0-1.0, defaults to 0.8 for user-created memories |
| has_embedding | INTEGER | 1 if fact has embedding in vec_memory |
| session_count | INTEGER | Number of sessions where this was seen/used |
| first_session_id | TEXT | Session when first created |
| last_session_id | TEXT | Most recent session that referenced this |
| status | TEXT | `candidate` or `confirmed` (evidence-based promotion) |
| has_entities | INTEGER | 1 if entities have been extracted |
| branch | TEXT | Git branch for branch-aware context boosting |
| created_at | TEXT | Timestamp |
| updated_at | TEXT | Last modification |

**Evidence-Based Memory**: New memories start as `candidate` with confidence 0.8. After being accessed across 3+ sessions, they're promoted to `confirmed` with boosted confidence.

---

## Code Intelligence

> These tables are in the **code index database** (`mira-code.db`).

### code_symbols

Indexed code symbols from tree-sitter parsing.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| file_path | TEXT | Relative file path |
| name | TEXT | Symbol name |
| symbol_type | TEXT | `function`, `struct`, `class`, `method`, etc. |
| start_line | INTEGER | Start line number |
| end_line | INTEGER | End line number |
| signature | TEXT | Normalized function/method signature |
| indexed_at | TEXT | When indexed |

### call_graph

Function call relationships for `code(action="callers")` / `code(action="callees")`.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| caller_id | INTEGER FK | Symbol that makes the call |
| callee_name | TEXT | Name of called function |
| callee_id | INTEGER FK | Resolved symbol ID (nullable) |
| call_count | INTEGER | Number of calls |

### imports

File import/dependency tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| file_path | TEXT | Source file |
| import_path | TEXT | Import target |
| is_external | INTEGER | 1 if external dependency |

### codebase_modules

High-level module structure with LLM-generated summaries.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| module_id | TEXT | Unique module identifier |
| name | TEXT | Module name |
| path | TEXT | Module path |
| purpose | TEXT | DeepSeek-generated description |
| exports | TEXT | JSON list of exports |
| depends_on | TEXT | JSON list of dependencies |
| symbol_count | INTEGER | Number of symbols |
| line_count | INTEGER | Lines of code |
| updated_at | TEXT | Last update |

---

## Session & History

### sessions

MCP session tracking for evidence and provenance.

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT PK | UUID session ID |
| project_id | INTEGER FK | Active project |
| status | TEXT | `active`, `completed` |
| summary | TEXT | Session summary |
| branch | TEXT | Git branch at session start |
| source | TEXT | `startup` or `resume` (default: `startup`) |
| resumed_from | TEXT | Previous session ID if resumed |
| started_at | TEXT | Start timestamp |
| last_activity | TEXT | Last activity |

### tool_history

Tool call history per session.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| session_id | TEXT FK | Session reference |
| tool_name | TEXT | Tool that was called |
| arguments | TEXT | JSON arguments |
| result_summary | TEXT | Abbreviated result (first 2000 chars) |
| full_result | TEXT | Complete tool result for recall |
| success | INTEGER | 1 if successful |
| created_at | TEXT | Timestamp |

> **Security note:** This table may contain sensitive data from tool outputs. For example, if Claude reads a file containing API keys or credentials, that content ends up in `result_summary` and `full_result`. Unlike `memory_facts` (which applies secret detection), `tool_history` stores results as-is. Treat `~/.mira/mira.db` as a sensitive file.

---

## Task Management

### goals

High-level project goals with progress tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| title | TEXT | Goal title |
| description | TEXT | Detailed description |
| status | TEXT | `planning`, `in_progress`, `blocked`, `completed`, `abandoned` |
| priority | TEXT | `low`, `medium`, `high`, `critical` |
| progress_percent | INTEGER | 0-100 |
| created_at | TEXT | Timestamp |

### milestones

Goal milestones for progress calculation.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| goal_id | INTEGER FK | Parent goal |
| title | TEXT | Milestone title |
| completed | INTEGER | 1 if done |
| weight | INTEGER | Weight for progress calc |

### tasks (Deprecated)

> **Note:** Task tracking via Mira is deprecated. Use Claude Code's native task system for in-session tracking, and Goals with Milestones for cross-session tracking.

Actionable tasks, optionally linked to goals.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| goal_id | INTEGER FK | Optional parent goal |
| title | TEXT | Task title |
| description | TEXT | Details |
| status | TEXT | `pending`, `in_progress`, `completed`, `blocked` |
| priority | TEXT | `low`, `medium`, `high`, `urgent` |
| created_at | TEXT | Timestamp |

---

## Documentation System

### documentation_tasks

Queue of documentation that needs to be written or updated.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| doc_type | TEXT | `api`, `guide`, `reference` |
| doc_category | TEXT | `mcp_tool`, `public_api`, `module` |
| source_file_path | TEXT | Source code being documented |
| target_doc_path | TEXT | Where doc should be written |
| priority | TEXT | `low`, `medium`, `high` |
| status | TEXT | `pending`, `skipped`, `completed` |
| reason | TEXT | Why this doc is needed (preserved on skip) |
| skip_reason | TEXT | Why the task was skipped (set on skip) |
| source_signature_hash | TEXT | Hash of source signatures for staleness |
| git_commit | TEXT | Commit when task was created |
| created_at | TEXT | Timestamp |

### documentation_inventory

Registry of existing documentation files.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| doc_path | TEXT | Path to doc file |
| doc_type | TEXT | Type of documentation |
| doc_category | TEXT | Category |
| title | TEXT | Document title |
| source_signature_hash | TEXT | Hash of source being documented |
| source_symbols | TEXT | Source file path (format: `source_file:<path>`) |
| last_seen_commit | TEXT | Last git commit when verified |
| is_stale | INTEGER | 1 if doc needs update |
| staleness_reason | TEXT | Why it's stale |
| verified_at | TEXT | Last verification |

---

## Diff Analysis

### diff_analyses

Semantic analysis of git diffs and commits.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| from_commit | TEXT | Base commit hash |
| to_commit | TEXT | Target commit hash |
| analysis_type | TEXT | `commit`, `staged`, `working` |
| changes_json | TEXT | JSON breakdown of changes by type |
| impact_json | TEXT | JSON analysis of affected code |
| risk_json | TEXT | JSON risk assessment |
| summary | TEXT | Human-readable summary |
| files_changed | INTEGER | Number of files modified |
| lines_added | INTEGER | Lines added |
| lines_removed | INTEGER | Lines removed |
| files_json | TEXT | JSON list of files from semantic changes |
| status | TEXT | `complete` or `partial` |
| created_at | TEXT | Timestamp |

Used by the `diff` tool to cache semantic analysis of git changes.

---

## Proactive Intelligence

Tables for behavior tracking, pattern mining, and proactive context injection.

### behavior_patterns

Tracks file sequences, tool chains, and session flows for pattern mining.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| pattern_type | TEXT | `file_sequence`, `tool_chain`, `session_flow`, `query_pattern` |
| pattern_key | TEXT | Unique identifier (hash of sequence) |
| pattern_data | TEXT | JSON: sequence details, items, transitions |
| confidence | REAL | Reliability score (0.0-1.0) |
| occurrence_count | INTEGER | Times pattern observed |
| last_triggered_at | TEXT | Last trigger time |
| first_seen_at | TEXT | First observation |
| updated_at | TEXT | Last update |

### proactive_interventions

Tracks suggestions made and user responses for learning.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| session_id | TEXT | Session reference |
| intervention_type | TEXT | `context_prediction`, `security_alert`, `bug_warning`, `resource_suggestion` |
| trigger_pattern_id | INTEGER FK | Pattern that triggered this |
| trigger_context | TEXT | What triggered intervention |
| suggestion_content | TEXT | What was suggested |
| confidence | REAL | Confidence in suggestion |
| user_response | TEXT | `accepted`, `dismissed`, `acted_upon`, `ignored`, NULL |
| response_time_ms | INTEGER | Time to respond |
| effectiveness_score | REAL | Computed effectiveness |
| created_at | TEXT | Timestamp |
| responded_at | TEXT | Response time |

### session_behavior_log

Raw events for pattern mining.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| session_id | TEXT | Session reference |
| event_type | TEXT | `file_access`, `tool_use`, `query`, `context_switch` |
| event_data | TEXT | JSON: file_path, tool_name, query_text, etc. |
| sequence_position | INTEGER | Position in session sequence |
| time_since_last_event_ms | INTEGER | Milliseconds since previous event |
| created_at | TEXT | Timestamp |

### proactive_suggestions

Pre-generated suggestions for fast lookup during UserPromptSubmit hook.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| pattern_id | INTEGER FK | Source pattern |
| trigger_key | TEXT | Fast lookup key (file path or tool name) |
| suggestion_text | TEXT | LLM-generated contextual hint |
| confidence | REAL | Suggestion confidence |
| shown_count | INTEGER | Times shown |
| accepted_count | INTEGER | Times accepted |
| created_at | TEXT | Timestamp |
| expires_at | TEXT | Expiration (7 days) |

---

## Multi-User & Teams

### teams

Team registry for Claude Code Agent Teams. One row per active team.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| name | TEXT | Team name (from Agent Teams config) |
| project_id | INTEGER FK | Project reference (nullable) |
| config_path | TEXT | Path to team config JSON |
| status | TEXT | `active` or `disbanded` |
| created_at | TEXT | Timestamp |
| updated_at | TEXT | Last update |

**Indexes:** `idx_teams_status(status)`, `idx_teams_name_project(name, COALESCE(project_id, 0))` (NULL-safe uniqueness).

### team_sessions

Active teammate sessions within a team. Each Claude Code agent participating in a team gets a row.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| team_id | INTEGER FK | Team reference (CASCADE delete) |
| session_id | TEXT | Claude Code session ID |
| member_name | TEXT | Teammate name (e.g., `researcher`, `tester`) |
| role | TEXT | `leader` or `teammate` |
| agent_type | TEXT | Agent type (e.g., `general-purpose`, `Explore`) |
| joined_at | TEXT | Timestamp |
| last_heartbeat | TEXT | Last heartbeat (stale after 30 min) |
| status | TEXT | `active` or `stopped` |

**Unique:** `(team_id, session_id)`. **Indexes:** `idx_ts_team_status`, `idx_ts_session`, `idx_ts_heartbeat`.

Heartbeats reactivate stale sessions — if a teammate's status was `stopped`, a new heartbeat sets it back to `active`.

### team_file_ownership

Tracks which teammate modified which files, used for conflict detection and convergence analysis.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| team_id | INTEGER FK | Team reference (CASCADE delete) |
| session_id | TEXT | Session that made the change |
| member_name | TEXT | Teammate who modified |
| file_path | TEXT | Path to modified file |
| operation | TEXT | Tool name (e.g., `Write`, `Edit`, `NotebookEdit`) |
| timestamp | TEXT | When the modification occurred |

**Indexes:** `idx_tfo_team_file`, `idx_tfo_session`, `idx_tfo_timestamp`.

Only write operations (`Write`, `Edit`, `NotebookEdit`, `MultiEdit`) are recorded — filtering is done in the PostToolUse hook layer, not via database constraints.

---

## Vector Tables (sqlite-vec)

### vec_memory *(main database)*

Vector embeddings for semantic memory search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | OpenAI text-embedding-3-small |
| fact_id | INTEGER | Reference to memory_facts.id |
| content | TEXT | Searchable content |

### vec_code *(code database)*

Vector embeddings for semantic code search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[N] | Provider-dependent dimension (1536 for OpenAI, 768 for nomic-embed-text). Detected and adjusted at startup via `ensure_code_vec_table_dimensions`. |
| file_path | TEXT | Source file |
| chunk_content | TEXT | Code chunk |
| project_id | INTEGER | Project reference |
| start_line | INTEGER | Starting line number |

### code_fts (FTS5) *(code database)*

Full-text search index for fast keyword search.

| Column | Type | Description |
|--------|------|-------------|
| file_path | TEXT | Source file (searchable) |
| chunk_content | TEXT | Code chunk (searchable) |
| project_id | INTEGER | Project reference (not searchable) |
| start_line | INTEGER | Starting line (not searchable) |

Uses `unicode61` tokenizer with `remove_diacritics 1` and `tokenchars '_'`. No stemming — identifiers like `database_pool` are indexed as single tokens, preserving exact matches for code search. Rebuilt from `vec_code` after indexing. A migration (`migrate_fts_tokenizer`) rewrites the FTS table if the tokenizer config has changed.

---

## Background Processing

### project_briefings

"What's New" briefings generated when git changes are detected.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK UNIQUE | Project reference |
| last_known_commit | TEXT | Git HEAD when briefing was generated |
| last_session_at | TEXT | When last session occurred |
| briefing_text | TEXT | DeepSeek-generated summary of changes |
| generated_at | TEXT | When briefing was created |

### pending_embeddings *(code database)*

Queue for async embedding generation. Stored in `mira-code.db` alongside the vector tables it feeds.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER | Project reference |
| file_path | TEXT | Source file |
| chunk_content | TEXT | Content to embed |
| start_line | INTEGER | Starting line number in source file |
| status | TEXT | `pending`, `processing`, `done` |
| created_at | TEXT | Timestamp |

### background_batches

Tracking for batch embedding requests.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| batch_id | TEXT | External batch ID |
| item_ids | TEXT | JSON list of pending_embeddings IDs |
| status | TEXT | `active`, `completed` |
| created_at | TEXT | Timestamp |

---

## Usage Tracking

### llm_usage

LLM API usage and cost tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| provider | TEXT | `deepseek` or `sampling` |
| model | TEXT | Model name |
| role | TEXT | LLM role/purpose for the call |
| prompt_tokens | INTEGER | Input token count |
| completion_tokens | INTEGER | Output token count |
| total_tokens | INTEGER | Total tokens |
| cache_hit_tokens | INTEGER | Cached input tokens |
| cache_miss_tokens | INTEGER | Non-cached input tokens |
| cost_estimate | REAL | Estimated cost in USD |
| duration_ms | INTEGER | Request duration |
| project_id | INTEGER FK | Project reference |
| session_id | TEXT | Session reference |
| created_at | TEXT | Timestamp |

### embeddings_usage

Embedding API usage tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| provider | TEXT | `openai` |
| model | TEXT | Model name (text-embedding-3-small) |
| tokens | INTEGER | Token count |
| text_count | INTEGER | Number of texts embedded |
| cost_estimate | REAL | Estimated cost |
| project_id | INTEGER FK | Project reference |
| created_at | TEXT | Timestamp |

---

## Server State

### server_state

Key-value storage for server state persistence.

| Column | Type | Description |
|--------|------|-------------|
| key | TEXT PK | State key |
| value | TEXT | State value |
| updated_at | TEXT | Last update timestamp |

Used to persist:
- `active_project_path`: Last active project, restored on MCP server startup
- `last_claude_session_id`: Claude Code session ID from hooks

---

## Additional Tables (v0.5.0+)

### session_tasks *(main database)*

Claude Code task persistence bridge. Snapshots tasks from Claude's native task system for cross-session continuity.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| session_id | TEXT | Claude Code session ID |
| project_id | INTEGER FK | Project reference |
| task_list_id | TEXT | Claude task list ID |
| task_data | TEXT | JSON snapshot of task state |
| created_at | TEXT | Timestamp |
| updated_at | TEXT | Last update |

### session_task_iterations *(main database)*

Task iteration summaries for tracking work across session segments.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| session_task_id | INTEGER FK | Reference to session_tasks |
| iteration_summary | TEXT | Summary of work done |
| created_at | TEXT | Timestamp |

### session_snapshots *(main database)*

Session state snapshots for resume support.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| session_id | TEXT | Session reference |
| snapshot_type | TEXT | Type of snapshot |
| data | TEXT | JSON snapshot data |
| created_at | TEXT | Timestamp |

### diff_outcomes *(main database)*

Change outcome tracking — detects whether changes were reverted or caused issues.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| diff_analysis_id | INTEGER FK | Reference to diff_analyses |
| project_id | INTEGER FK | Reference to projects |
| outcome_type | TEXT | Type of outcome detected |
| evidence_commit | TEXT | Git commit providing evidence |
| evidence_message | TEXT | Commit message of evidence |
| time_to_outcome_seconds | INTEGER | Time between change and outcome |
| detected_by | TEXT | Detection method (default: 'git_scan') |
| created_at | TEXT | Timestamp |

**Unique:** `(diff_analysis_id, outcome_type, evidence_commit)`

### memory_entities *(main database)*

Canonical entity registry for recall boost.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| name | TEXT | Entity name |
| entity_type | TEXT | Entity type |
| project_id | INTEGER FK | Project reference |
| created_at | TEXT | Timestamp |

### memory_entity_links *(main database)*

Many-to-many between memory facts and entities.

| Column | Type | Description |
|--------|------|-------------|
| fact_id | INTEGER FK | Reference to memory_facts |
| entity_id | INTEGER FK | Reference to memory_entities |

### tech_debt_scores *(main database)*

Per-module tech debt scoring.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| module_path | TEXT | Module path |
| score | REAL | Debt score |
| details | TEXT | JSON breakdown |
| created_at | TEXT | Timestamp |

### module_conventions *(main database)*

Convention-aware context injection data.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| module_path | TEXT | Module path |
| conventions | TEXT | JSON conventions data |
| created_at | TEXT | Timestamp |

### code_chunks *(code database)*

Canonical chunk store for indexed code.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER | Project reference |
| file_path | TEXT | Source file |
| chunk_content | TEXT | Code chunk content |
| start_line | INTEGER | Starting line |
| created_at | TEXT | Timestamp |

### module_dependencies *(code database)*

Cross-module dependency analysis.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER | Project reference |
| source_module | TEXT | Source module path |
| target_module | TEXT | Target module path |
| dependency_type | TEXT | Type of dependency |
| created_at | TEXT | Timestamp |

---

## Chat History

> **Note:** Legacy tables from the web chat era. Schema preserved but not actively used in current MCP-only architecture.

### chat_messages

Persistent chat history for DeepSeek Reasoner conversations.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| role | TEXT | `user` or `assistant` |
| content | TEXT | Message content |
| reasoning_content | TEXT | DeepSeek reasoning (if any) |
| summarized | INTEGER | 1 if included in a summary |
| summary_id | INTEGER FK | Links to chat_summaries |
| created_at | TEXT | Timestamp |

### chat_summaries

Rolling summarization for long conversations.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project scope |
| summary | TEXT | Summarized content |
| message_range_start | INTEGER | First message ID covered |
| message_range_end | INTEGER | Last message ID covered |
| summary_level | INTEGER | 1=session, 2=daily, 3=weekly |
| created_at | TEXT | Timestamp |
