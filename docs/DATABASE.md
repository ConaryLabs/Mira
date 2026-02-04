# Mira Database Schema

Mira uses two SQLite databases with the sqlite-vec extension for vector search.

| Database | Path | Purpose |
|----------|------|---------|
| **Main** | `~/.mira/mira.db` | Memories, sessions, experts, goals, proactive intelligence |
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
| fact_type | TEXT | `general`, `preference`, `decision`, `context`, `health` |
| category | TEXT | Optional grouping (e.g., `coding`, `tooling`) |
| confidence | REAL | 0.0-1.0, starts at 0.5 for candidates |
| has_embedding | INTEGER | 1 if fact has embedding in vec_memory |
| session_count | INTEGER | Number of sessions where this was seen/used |
| first_session_id | TEXT | Session when first created |
| last_session_id | TEXT | Most recent session that referenced this |
| status | TEXT | `candidate` or `confirmed` (evidence-based promotion) |
| branch | TEXT | Git branch for branch-aware context boosting |
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

Used by the `analyze_diff` tool to cache semantic analysis of git changes.

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

### proactive_preferences

User preferences for proactive features.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| user_id | TEXT | User reference |
| project_id | INTEGER FK | Project reference |
| preference_key | TEXT | `proactivity_level`, `max_alerts_per_hour`, `min_confidence` |
| preference_value | TEXT | JSON value |
| updated_at | TEXT | Last update |

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

## Evolutionary Expert System

Tables for tracking expert consultation history, problem patterns, and prompt evolution.

### expert_consultations

Detailed history of each expert consultation.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| expert_role | TEXT | Expert role used |
| project_id | INTEGER FK | Project reference |
| session_id | TEXT | Session reference |
| context_hash | TEXT | Hash of context for pattern matching |
| problem_category | TEXT | Categorized problem type |
| context_summary | TEXT | Brief summary of context |
| tools_used | TEXT | JSON array of tools called |
| tool_call_count | INTEGER | Number of tool calls |
| consultation_duration_ms | INTEGER | Duration in milliseconds |
| initial_confidence | REAL | Expert's stated confidence |
| calibrated_confidence | REAL | Adjusted based on history |
| prompt_version | INTEGER | Which prompt version was used |
| created_at | TEXT | Timestamp |

### problem_patterns

Recurring problem signatures per expert.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| expert_role | TEXT | Expert role |
| pattern_signature | TEXT | Hash of problem characteristics |
| pattern_description | TEXT | Human-readable description |
| common_context_elements | TEXT | JSON: context elements that appear together |
| successful_approaches | TEXT | JSON: which analysis approaches work best |
| recommended_tools | TEXT | JSON: which tools yield best results |
| success_rate | REAL | Success rate |
| occurrence_count | INTEGER | Times observed |
| avg_confidence | REAL | Average confidence |
| avg_acceptance_rate | REAL | Average acceptance rate |
| last_seen_at | TEXT | Last observation |
| created_at | TEXT | Timestamp |

### expert_outcomes

Tracks whether expert advice led to good results.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| consultation_id | INTEGER FK | Consultation reference |
| finding_id | INTEGER FK | Finding reference |
| outcome_type | TEXT | `code_change`, `design_adoption`, `bug_fix`, `security_fix` |
| git_commit_hash | TEXT | If advice led to code change |
| files_changed | TEXT | JSON array of changed files |
| change_similarity_score | REAL | How closely change matches suggestion |
| user_outcome_rating | REAL | User-provided rating (0-1) |
| outcome_evidence | TEXT | JSON: links to tests, metrics, etc. |
| time_to_outcome_seconds | INTEGER | Time until outcome realized |
| learned_lesson | TEXT | What pattern we learned |
| created_at | TEXT | Timestamp |
| verified_at | TEXT | Verification time |

### expert_prompt_versions

Tracks prompt versions and their performance.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| expert_role | TEXT | Expert role |
| version | INTEGER | Version number |
| prompt_additions | TEXT | Additional context added to base prompt |
| performance_metrics | TEXT | JSON: acceptance_rate, outcome_success, etc. |
| adaptation_reason | TEXT | Why this version was created |
| consultation_count | INTEGER | Number of consultations |
| acceptance_rate | REAL | Acceptance rate |
| is_active | INTEGER | 1 if active |
| created_at | TEXT | Timestamp |

### collaboration_patterns

When experts should work together.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| problem_domains | TEXT | JSON: which expertise domains involved |
| complexity_threshold | REAL | Min complexity score to trigger |
| recommended_experts | TEXT | JSON: which experts to involve |
| collaboration_mode | TEXT | `parallel`, `sequential`, `hierarchical` |
| synthesis_method | TEXT | How to combine outputs |
| success_rate | REAL | Success rate |
| time_saved_percent | REAL | Efficiency vs individual consultations |
| occurrence_count | INTEGER | Times used |
| last_used_at | TEXT | Last use |
| created_at | TEXT | Timestamp |

---

## Cross-Project Intelligence

Tables for privacy-preserving pattern sharing across projects.

### cross_project_patterns

Anonymized patterns that can be shared across projects.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| pattern_type | TEXT | `file_sequence`, `tool_chain`, `problem_pattern`, `collaboration` |
| pattern_hash | TEXT UNIQUE | Hash for deduplication |
| anonymized_data | TEXT | JSON: pattern data with identifiers removed |
| category | TEXT | High-level category (e.g., `rust`, `web`, `database`) |
| confidence | REAL | Aggregated confidence across projects |
| occurrence_count | INTEGER | Projects showing this pattern |
| noise_added | REAL | Differential privacy noise level |
| min_projects_required | INTEGER | K-anonymity threshold (default: 3) |
| source_project_count | INTEGER | Contributing project count |
| last_updated_at | TEXT | Last update |
| created_at | TEXT | Timestamp |

### pattern_sharing_log

Tracks pattern exports and imports.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK | Project reference |
| direction | TEXT | `exported` or `imported` |
| pattern_type | TEXT | Pattern type |
| pattern_hash | TEXT | Pattern hash |
| anonymization_level | TEXT | `full`, `partial`, `none` |
| differential_privacy_epsilon | REAL | Privacy budget used |
| created_at | TEXT | Timestamp |

### cross_project_preferences

Per-project sharing preferences.

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| project_id | INTEGER FK UNIQUE | Project reference |
| sharing_enabled | INTEGER | Master opt-in switch |
| export_patterns | INTEGER | Allow exporting patterns |
| import_patterns | INTEGER | Allow importing patterns |
| min_anonymization_level | TEXT | `full`, `partial`, `none` |
| allowed_pattern_types | TEXT | JSON array of allowed types |
| privacy_epsilon_budget | REAL | Total differential privacy budget |
| privacy_epsilon_used | REAL | Privacy budget consumed |
| created_at | TEXT | Timestamp |
| updated_at | TEXT | Last update |

### pattern_provenance

Tracks which projects contributed (anonymously).

| Column | Type | Description |
|--------|------|-------------|
| id | INTEGER PK | Auto-increment ID |
| pattern_id | INTEGER FK | Cross-project pattern reference |
| contribution_hash | TEXT | Hash of project contribution (not project id) |
| contribution_weight | REAL | How much this contribution affects pattern |
| contributed_at | TEXT | Timestamp |

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

### vec_memory *(main database)*

Vector embeddings for semantic memory search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | Google gemini-embedding-001 |
| fact_id | INTEGER | Reference to memory_facts.id |
| content | TEXT | Searchable content |

### vec_code *(code database)*

Vector embeddings for semantic code search.

| Column | Type | Description |
|--------|------|-------------|
| embedding | float[1536] | Google gemini-embedding-001 |
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
