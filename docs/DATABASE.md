# Mira Database Schema

SQLite database stored at `~/.mira/mira.db` with sqlite-vec extension for vector search.

---

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
| fact_type | TEXT | `general`, `preference`, `decision`, `context`, `health`, `capability` |
| category | TEXT | Optional grouping (e.g., `coding`, `tooling`) |
| confidence | REAL | 0.0-1.0, starts at 0.5 for candidates |
| has_embedding | INTEGER | 1 if fact has embedding in vec_memory |
| session_count | INTEGER | Number of sessions where this was seen/used |
| first_session_id | TEXT | Session when first created |
| last_session_id | TEXT | Most recent session that referenced this |
| status | TEXT | `candidate` or `confirmed` (evidence-based promotion) |
| created_at | TEXT | Timestamp |
| updated_at | TEXT | Last modification |

**Evidence-Based Memory**: New memories start as `candidate` with max confidence 0.5. After being accessed across 3+ sessions, they're promoted to `confirmed` with boosted confidence.

### corrections

Pattern corrections learned from reviewed findings.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project scope |
| what_was_wrong | TEXT | The incorrect pattern/behavior |
| what_is_right | TEXT | The correct approach |
| correction_type | TEXT | `bug`, `style`, `security`, `performance` |
| scope | TEXT | `project` or `global` |
| confidence | REAL | 0.0-1.0 |
| created_at | TEXT | Timestamp |

---

## Code Intelligence

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

Function call relationships for find_callers/find_callees.

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
| status | TEXT | `pending`, `skipped`, `applied` |
| reason | TEXT | Why this doc is needed |
| source_signature_hash | TEXT | Hash of source signatures for staleness |
| git_commit | TEXT | Commit when task was created |
| retry_count | INTEGER | Number of generation attempts |
| last_error | TEXT | Last error if generation failed |
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
| source_symbols | TEXT | JSON list of documented symbols |
| last_seen_commit | TEXT | Last git commit when verified |
| is_stale | INTEGER | 1 if doc needs update |
| staleness_reason | TEXT | Why it's stale |
| verified_at | TEXT | Last verification |

---

## Expert System

### system_prompts

Custom configuration for expert roles.

| Column | Type | Description |
|--------|------|-------------|
| role | TEXT PK | `architect`, `code_reviewer`, `security`, etc. |
| prompt | TEXT | Custom system prompt |
| provider | TEXT | LLM provider: `deepseek`, `openai`, `gemini` |
| model | TEXT | Custom model name (optional) |
| updated_at | TEXT | Last modification |

### review_findings

Code review findings from expert consultations (learning loop).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| expert_role | TEXT | Which expert found this |
| file_path | TEXT | File with the issue |
| finding_type | TEXT | Type of finding |
| severity | TEXT | `low`, `medium`, `high`, `critical` |
| content | TEXT | Finding description |
| code_snippet | TEXT | Relevant code |
| suggestion | TEXT | Suggested fix |
| status | TEXT | `pending`, `accepted`, `rejected`, `fixed` |
| feedback | TEXT | User feedback on finding |
| confidence | REAL | Expert's confidence |
| reviewed_by | TEXT | Who reviewed |
| session_id | TEXT | Session when found |
| created_at | TEXT | Timestamp |
| reviewed_at | TEXT | When reviewed |

When findings are accepted/rejected, patterns are extracted into `corrections` for future expert consultations.

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
| status | TEXT | `complete` or `partial` |
| created_at | TEXT | Timestamp |

Used by `analyze_diff` tool to cache semantic analysis of git changes.

---

## Multi-User & Teams

### users

User identity registry.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| identity | TEXT UNIQUE | User identifier |
| display_name | TEXT | Display name |
| email | TEXT | Email address |
| created_at | TEXT | Timestamp |

### teams

Team definitions for shared memory.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| name | TEXT | Team name |
| description | TEXT | Team description |
| created_by | TEXT | User who created |
| created_at | TEXT | Timestamp |

### team_members

Team membership.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| team_id | INTEGER FK | Team reference |
| user_identity | TEXT | Member's identity |
| role | TEXT | `member` or `admin` |
| joined_at | TEXT | When joined |

---

## Vector Tables (sqlite-vec)

### vec_memory

Vector embeddings for semantic memory search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | Google gemini-embedding-001 |
| fact_id | INTEGER | Reference to memory_facts.id |
| content | TEXT | Searchable content |

### vec_code

Vector embeddings for semantic code search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | Google gemini-embedding-001 |
| file_path | TEXT | Source file |
| chunk_content | TEXT | Code chunk |
| project_id | INTEGER | Project reference |
| start_line | INTEGER | Starting line number |

### code_fts (FTS5)

Full-text search index for fast keyword search.

| Column | Type | Description |
|--------|------|-------------|
| file_path | TEXT | Source file (searchable) |
| chunk_content | TEXT | Code chunk (searchable) |
| project_id | INTEGER | Project reference (not searchable) |
| start_line | INTEGER | Starting line (not searchable) |

Uses Porter stemming with Unicode support. Rebuilt from vec_code after indexing.

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

---

## Usage Tracking

### llm_usage

LLM API usage and cost tracking.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| provider | TEXT | `deepseek` or `gemini` |
| model | TEXT | Model name |
| role | TEXT | Expert role that made the call |
| prompt_tokens | INTEGER | Input token count |
| completion_tokens | INTEGER | Output token count |
| total_tokens | INTEGER | Total tokens |
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
| provider | TEXT | `google` |
| model | TEXT | Model name (gemini-embedding-001) |
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

### permission_rules

Auto-approved tool patterns (for permission hook).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| tool_name | TEXT | Tool name pattern |
| pattern | TEXT | Argument pattern |
| match_type | TEXT | `prefix`, `exact`, `glob` |
| scope | TEXT | `global` or project-specific |
| created_at | TEXT | Timestamp |

---

## Chat History

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
