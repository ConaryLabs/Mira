# Mira-Chat Features

Power-armored coding assistant with GPT-5.2 Thinking model.

## Core Architecture

### GPT-5.2 Integration (`responses.rs`)
- **Responses API** - Uses OpenAI's latest `/v1/responses` endpoint
- **Streaming SSE** - Real-time token streaming for responsive UI
- **Variable Reasoning Effort** - `none`, `low`, `medium`, `high`, `xhigh`
- **Conversation Continuity** - `previous_response_id` chains requests
- **Function Calling** - Native tool use with parallel execution support

### Automatic Reasoning Classification (`reasoning.rs`)
Task complexity detection routes queries to appropriate reasoning level:
- **xhigh**: Architecture, refactoring, multi-file changes
- **high**: Algorithm design, debugging complex issues
- **medium**: Code writing, feature implementation (default)
- **low**: Simple edits, formatting, documentation
- **none**: Factual questions, file listing

---

## Invisible Session Management (`session.rs`)

### Message Persistence
- All user/assistant messages saved to SQLite (`chat_messages`)
- Embeddings stored in Qdrant for semantic search
- Project-scoped (each project has its own context)

### Context Assembly (per-query)
Dynamically assembles context from multiple sources:
1. **Recent messages** - Sliding window (last 20 turns)
2. **Semantic recall** - Query-relevant past conversation (not code!)
3. **Mira context** - Corrections, goals, memories
4. **Code compaction** - Encrypted understanding blob
5. **Summaries** - Compressed older conversation

### Auto-Summarization
- Triggers when message count exceeds 30
- Calls GPT-5.2 (low reasoning) to summarize oldest messages
- Stores summary, deletes old messages
- Keeps context manageable indefinitely

### Code Compaction
- Tracks files touched via tools (read/write/edit)
- Auto-triggers when 10+ files touched
- Calls `/responses/compact` endpoint
- Stores encrypted blob preserving code understanding
- Manual trigger via `/compact` command

### Cache Optimization
Prompt structure optimized for LLM prefix caching:
1. Base instructions (static)
2. Project path (stable per session)
3. Corrections, goals, memories (occasional changes)
4. Compaction blob (changes on compaction)
5. Summaries (changes on summarization)
6. Semantic context (changes per query)

---

## Tools (`tools.rs`)

### File Operations
| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write/create file |
| `edit_file` | Search/replace edit with uniqueness check |
| `glob` | Find files by pattern |
| `grep` | Search file contents (uses ripgrep) |

### Shell
| Tool | Description |
|------|-------------|
| `bash` | Execute shell commands |

### Web
| Tool | Description |
|------|-------------|
| `web_search` | Search the web (via DuckDuckGo) |
| `web_fetch` | Fetch and extract URL content |

### Memory
| Tool | Description |
|------|-------------|
| `remember` | Store fact in persistent memory |
| `recall` | Semantic search of memories |

### Git
| Tool | Description |
|------|-------------|
| `git_status` | Working tree status (branch, staged, unstaged, untracked) |
| `git_diff` | Show staged or unstaged changes |
| `git_commit` | Create commit with optional stage-all |
| `git_log` | Recent commit history |

### Test Runner
| Tool | Description |
|------|-------------|
| `run_tests` | Execute tests with auto-detection (cargo/pytest/npm/go) |

### Mira Power Armor (Persistence & Learning)
| Tool | Description |
|------|-------------|
| `task` | Persistent task management with subtasks, priorities, statuses. Actions: create/list/update/complete/delete |
| `goal` | High-level goal tracking with milestones. Actions: create/list/update/add_milestone/complete_milestone |
| `correction` | Record and learn from user corrections. Actions: record/list/validate |
| `store_decision` | Store architectural/design decisions with context for semantic recall |
| `record_rejected_approach` | Record failed approaches to avoid re-suggesting them |

---

## REPL Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/clear` | Clear conversation history |
| `/compact` | Manually compact code context |
| `/context` | Show current Mira context |
| `/status` | Show session stats (messages, summaries, etc.) |
| `/switch [path]` | Switch to different project |
| `/remember <text>` | Store in memory |
| `/recall <query>` | Search memory |
| `/tasks` | List active tasks |
| `/quit`, `/exit` | Exit the REPL |

---

## Mira Context Integration (`context.rs`)

Loads from Mira's SQLite backend:
- **Corrections** - Things user has corrected before
- **Goals** - Active project goals with priority/progress
- **Memories** - Facts and preferences

---

## Configuration (`config.rs`)

Config file: `~/.mira/config.toml`

```toml
openai_api_key = "sk-..."
gemini_api_key = "..."  # For embeddings
database_url = "sqlite://data/mira.db"
qdrant_url = "http://localhost:6334"
reasoning_effort = "medium"
project = "/default/project/path"
```

CLI args override config file.

---

## HTTP Server Mode (`server.rs`)

For Studio integration:
```bash
mira-chat --serve --port 3000
```

Provides SSE streaming endpoint for web UI.

---

## What's Working

- [x] GPT-5.2 Responses API with streaming
- [x] Variable reasoning effort (auto-classified)
- [x] All core tools (file, shell, web, memory)
- [x] Message persistence to SQLite + Qdrant
- [x] Semantic recall of past conversation
- [x] Auto-summarization when context grows
- [x] Auto-compaction of code context
- [x] Cache-optimized prompt structure
- [x] REPL with tab completion
- [x] Multi-line input (Alt+Enter)
- [x] Ctrl+C to cancel streaming
- [x] Mira context injection
- [x] Project switching
- [x] HTTP server mode
- [x] Studio web UI integration (SSE streaming, tool calls, diffs)
- [x] Task management (create, list, update, complete, delete)
- [x] Goal tracking with milestones
- [x] Correction recording and learning
- [x] Decision storage with semantic recall
- [x] Rejected approach tracking
- [x] Git tools (status, diff, commit, log)
- [x] Test runner with auto-detection
- [x] Terminal diff view with colors

---

## TODO / Possible Enhancements

### High Priority
- [x] **Diff view in terminal** - Show unified diff for file changes
- [x] **Git integration** - git_status, git_diff, git_commit, git_log tools
- [x] **Test runner integration** - run_tests with auto-detection
- [ ] **Linter integration** - Auto-fix lint errors

### Medium Priority
- [ ] **Multi-file edits** - Batch changes across files
- [ ] **Undo/redo** - Revert recent file changes
- [ ] **Conversation export** - Save session to file
- [ ] **Image support** - Screenshot/diagram analysis
- [ ] **Voice input** - Speech-to-text
- [ ] **Clipboard integration** - Paste code directly

### Low Priority / Nice to Have
- [ ] **Custom tools** - User-defined tool plugins
- [ ] **Multiple models** - Route to different models (Claude, etc.)
- [ ] **Local models** - Ollama/llama.cpp support
- [ ] **Workspace indexing** - Pre-index codebase for faster search
- [ ] **Smart file watching** - Auto-reload changed files
- [ ] **Terminal UI** - Full TUI with panels (like lazygit)

### Compaction Improvements
- [ ] **Smarter compaction triggers** - Based on token count, not just file count
- [ ] **Incremental compaction** - Add to existing blob vs. replace
- [ ] **Compaction expiry** - Auto-expire old blobs

### Summarization Improvements
- [ ] **Hierarchical summaries** - Summary of summaries for very long sessions
- [ ] **Topic-based summarization** - Group by topic before summarizing
- [ ] **Keep important messages** - Don't summarize starred/pinned messages

---

## Database Tables Used

| Table | Purpose |
|-------|---------|
| `chat_messages` | Message history |
| `chat_context` | Per-project session state |
| `chat_summaries` | Rolling conversation summaries |
| `code_compaction` | Encrypted code context blobs |
| `memory_facts` | Persistent memories |
| `corrections` | User corrections |
| `goals` | Project goals |
| `projects` | Project registry |

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `OPENAI_API_KEY` | Required for GPT-5.2 |
| `GEMINI_API_KEY` | For embeddings (Qdrant) |
| `DATABASE_URL` | SQLite path |
| `QDRANT_URL` | Qdrant server |
| `RUST_LOG` | Logging level |
