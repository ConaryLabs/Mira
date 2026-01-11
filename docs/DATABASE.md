# docs/DATABASE.md
# Mira Database Schema

SQLite database stored at `~/.mira/mira.db` with sqlite-vec extension for vector search.

## Core Tables

### projects
Project registry for multi-project support.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| path | TEXT UNIQUE | Filesystem path to project root |
| name | TEXT | Display name |
| created_at | TEXT | Timestamp |

### memory_facts
Semantic memory storage for facts, decisions, and preferences.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Optional project scope |
| key | TEXT | Unique key for upsert operations |
| content | TEXT | The fact/preference content |
| fact_type | TEXT | `general`, `preference`, `decision`, `context` |
| category | TEXT | Optional grouping (e.g., `coding`, `tooling`) |
| confidence | REAL | 0.0-1.0, defaults to 1.0 |
| has_embedding | INTEGER | 1 if fact has embedding in vec_memory |
| created_at | TEXT | Timestamp |
| updated_at | TEXT | Last modification |

### corrections
Pattern corrections for learning from mistakes.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project scope |
| what_was_wrong | TEXT | The incorrect pattern/behavior |
| what_is_right | TEXT | The correct approach |
| correction_type | TEXT | `pattern`, `fact`, `style` |
| scope | TEXT | `project` or `global` |
| confidence | REAL | 0.0-1.0 |
| created_at | TEXT | Timestamp |

## Code Intelligence

### code_symbols
Indexed code symbols from AST parsing.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| file_path | TEXT | Relative file path |
| name | TEXT | Symbol name |
| symbol_type | TEXT | `function`, `struct`, `class`, `method`, etc. |
| start_line | INTEGER | Start line number |
| end_line | INTEGER | End line number |
| signature | TEXT | Function/method signature |
| indexed_at | TEXT | When indexed |

Indexes: `(project_id, file_path)`, `(name)`

### call_graph
Function call relationships for find_callers/find_callees.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| caller_id | INTEGER FK | Symbol that makes the call |
| callee_name | TEXT | Name of called function |
| callee_id | INTEGER FK | Resolved symbol ID (nullable) |
| call_count | INTEGER | Number of calls |

Indexes: `(caller_id)`, `(callee_id)`

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
High-level module summaries (LLM-generated).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| module_id | TEXT | Unique module identifier |
| name | TEXT | Module name |
| path | TEXT | Module path |
| purpose | TEXT | LLM-generated description |
| exports | TEXT | JSON list of exports |
| depends_on | TEXT | JSON list of dependencies |
| symbol_count | INTEGER | Number of symbols |
| line_count | INTEGER | Lines of code |
| updated_at | TEXT | Last update |

## Session & History

### sessions
MCP session tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT PK | UUID session ID |
| project_id | INTEGER FK | Active project |
| status | TEXT | `active`, `completed` |
| summary | TEXT | Session summary |
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

### tasks
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

## Chat & Summarization

### chat_messages
Persistent chat history for DeepSeek Reasoner.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| role | TEXT | `user` or `assistant` |
| content | TEXT | Message content |
| reasoning_content | TEXT | DeepSeek reasoning (if any) |
| summarized | INTEGER | 1 if included in a summary |
| summary_id | INTEGER FK | Links to chat_summaries for reversibility |
| created_at | TEXT | Timestamp |

Messages link to their summary via `summary_id`, enabling `unroll_summary()` to restore original messages.

### chat_summaries
Rolling summarization for long conversations.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project scope (NULL for global) |
| summary | TEXT | Summarized content |
| message_range_start | INTEGER | First message ID covered |
| message_range_end | INTEGER | Last message ID covered |
| summary_level | INTEGER | 1=session, 2=daily, 3=weekly |
| created_at | TEXT | Timestamp |

Summaries are project-scoped to keep context separate across projects.

## Vector Tables (sqlite-vec)

### vec_memory
Vector embeddings for semantic memory search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | OpenAI text-embedding-3-small |
| fact_id | INTEGER | Reference to memory_facts.id |
| content | TEXT | Searchable content |

### vec_code
Vector embeddings for semantic code search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | OpenAI text-embedding-3-small |
| file_path | TEXT | Source file |
| chunk_content | TEXT | Code chunk |
| project_id | INTEGER | Project reference |

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

The background worker monitors git HEAD for each project. When changes are detected, it summarizes the commits using DeepSeek Reasoner and stores the briefing. The briefing is shown once in `session_start` then cleared.

### pending_embeddings
Queue for async embedding generation.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER | Project reference |
| file_path | TEXT | Source file |
| chunk_content | TEXT | Content to embed |
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

## Permissions

### permission_rules
Auto-approved tool patterns.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| tool_name | TEXT | Tool name pattern |
| pattern | TEXT | Argument pattern |
| match_type | TEXT | `prefix`, `exact`, `glob` |
| scope | TEXT | `global` or project-specific |
| created_at | TEXT | Timestamp |

## Server State

### server_state
Key-value storage for server state persistence across restarts.

| Column | Type | Description |
|--------|------|-------------|
| key | TEXT PK | State key |
| value | TEXT | State value |
| updated_at | TEXT | Last update timestamp |

Used to persist:
- `active_project_path`: Last active project, restored on web server startup
