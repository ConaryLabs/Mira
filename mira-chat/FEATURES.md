# Mira-Chat Features

Power-armored coding assistant with multi-model support: GPT-5.2 and DeepSeek V3.2.

## Core Architecture

### Multi-Model Support

Mira-Chat supports two model providers, selectable via the Studio UI:

| Model | Use Case | Pricing | Caching |
|-------|----------|---------|---------|
| **GPT-5.2** | Full capability, complex reasoning | ~$2.50/M input, $10/M output | Server-side (response_id chain) |
| **DeepSeek V3.2** | Cost-effective, fast | $0.27/M input (cache: $0.014/M) | Prefix caching (automatic) |

Both models receive Mira persona and corrections. GPT-5.2 also gets goals and memories.

### GPT-5.2 Integration (`responses.rs`)
- **Responses API** - Uses OpenAI's latest `/v1/responses` endpoint
- **Streaming SSE** - Real-time token streaming for responsive UI
- **Variable Reasoning Effort** - `none`, `low`, `medium`, `high`, `xhigh`
- **Conversation Continuity** - `previous_response_id` chains requests
- **Function Calling** - Native tool use with parallel execution support

### DeepSeek V3.2 Integration (`provider/deepseek.rs`)
- **Chat Completions API** - OpenAI-compatible endpoint
- **Streaming SSE** - Uses `mira_core::SseDecoder` for consistent parsing
- **Prefix Caching** - Aggressive server-side caching (95% discount on cache hits)
- **Cached Token Reporting** - Reports `prompt_cache_hit_tokens` in usage stats
- **Function Calling** - Full tool support with streaming
- **Reasoning Tokens** - Supports `deepseek-reasoner` for visible chain-of-thought

### Provider Abstraction (`provider/mod.rs`)
Unified `Provider` trait for multi-model support:
```rust
pub trait Provider: Send + Sync {
    async fn create_stream(&self, request: ChatRequest) -> Result<Receiver<StreamEvent>>;
    async fn create(&self, request: ChatRequest) -> Result<ChatResponse>;
    async fn continue_with_tools_stream(&self, request: ToolContinueRequest) -> Result<Receiver<StreamEvent>>;
}
```

### Context Injection
- **GPT-5.2**: Full context (persona + corrections + goals + memories)
- **DeepSeek**: Focused context (persona + corrections only)

Prompt structure optimized for prefix caching:
```
[PERSONA - stable]           ← cached
[CORRECTIONS - stable]       ← cached
[PROJECT PATH - stable]      ← cached
[TOOL GUIDELINES - stable]   ← cached
[USER MESSAGE - varies]      ← not cached
```

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

### Smart Chain Reset (GPT-5.2 Only)

Intelligent chain management prevents context degradation while maximizing cache hits:

**Thresholds:**
| Threshold | Value | Purpose |
|-----------|-------|---------|
| `CHAIN_RESET_TOKEN_THRESHOLD` | 400k | Soft reset threshold |
| `CHAIN_RESET_HARD_CEILING` | 420k | Hard reset (quality guard) |
| `CHAIN_RESET_MIN_CACHE_PCT` | 30% | Cache efficiency threshold |
| `CHAIN_RESET_HYSTERESIS_TURNS` | 2 | Consecutive low-cache turns required |
| `CHAIN_RESET_COOLDOWN_TURNS` | 3 | Minimum turns between resets |

**Reset Logic:**
- **Hard Reset**: Triggers at 420k tokens regardless of cache% (prevents silent quality degradation)
- **Soft Reset**: Triggers at 400k tokens + <30% cache for 2+ consecutive turns
- **Hysteresis**: Prevents flappy resets from single bad turns
- **Cooldown**: Minimum 3 turns between resets

**Handoff Context** (preserves continuity after reset):
- Recent conversation (last 6 messages, truncated)
- Latest summary from summaries table
- Active goals with progress
- Recent decisions from memory
- Working set (last ~10 touched files)
- Last known failure (command + error)
- Recent artifact IDs (last 5)
- Continuity note for smooth transition

**Context Deduplication**: On handoff turns, normal context assembly is skipped to avoid duplicating content already in the handoff blob.

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

## Studio Web UI (`studio/`)

SvelteKit-based terminal-style web interface.

### Features
- **Terminal aesthetic** - Monospace fonts, dark theme, prompt-style input
- **Model selector** - Switch between GPT-5.2 and DeepSeek V3.2
- **Multiple themes** - Dark, Retro (CRT), Modern (Tokyo Night), Neon
- **Streaming responses** - Real-time SSE with tool call visualization
- **Collapsible sidebar** - Project selector, model, reasoning effort, status, theme picker
- **Mobile responsive** - Hamburger menu, overlay sidebar, safe viewport height

### Mobile Support
- **iOS Safari safe area** - Uses `100dvh` for proper viewport height (no URL bar overlap)
- **Responsive breakpoint** - 768px; sidebar hidden on mobile, shown as overlay
- **Touch-friendly** - Larger tap targets, backdrop to close sidebar
- **Compact header** - Mobile header with hamburger menu and status indicator

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
- [x] **Multiple models** - GPT-5.2 and DeepSeek V3.2 with UI switcher
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

## Shared Infrastructure (`mira-core`)

Mira-chat shares core functionality with the MCP server via the `mira-core` crate:

### Memory Operations (`mira_core::memory`)
- `make_memory_key()` - Normalized key generation (50 char, lowercase, alphanumeric)
- `upsert_memory_fact()` - Insert/update with `MemoryScope` (Global/ProjectId)
- `recall_memory_facts()` - Semantic-first search with text fallback
- `forget_memory_fact()` - Delete from SQLite + Qdrant
- **Batch updates** - Fixes N+1 query issue for `times_used` tracking

### SSE Streaming (`mira_core::streaming`)
- `SseDecoder` - Buffered SSE frame parser with partial chunk handling
- `SseFrame` - Parsed frame with `is_done()`, `parse<T>()`, `try_parse<T>()`
- Used by DeepSeek provider (and ready for OpenAI migration)

### Other Shared Modules
- `secrets` - Secret detection and redaction
- `excerpts` - Smart text excerpting and UTF-8 helpers
- `semantic` - Qdrant + Gemini embedding integration
- `artifacts` - Large output storage with deduplication
- `limits` - Shared constants (token thresholds, chain reset settings)

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
| `DEEPSEEK_API_KEY` | Required for DeepSeek V3.2 |
| `GEMINI_API_KEY` | For embeddings (Qdrant) |
| `DATABASE_URL` | SQLite path |
| `QDRANT_URL` | Qdrant server |
| `RUST_LOG` | Logging level |
