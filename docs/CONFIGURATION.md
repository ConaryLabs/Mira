# Mira Configuration Guide

This guide details all configuration options for Mira, including environment variables, file locations, and hook setup.

---

## 1. Environment Variables

Mira uses environment variables for API keys and configuration. These are set in `~/.mira/.env`.

### Intelligence Features

| Variable | Required | Description |
|----------|----------|-------------|
| `DEEPSEEK_API_KEY` | Optional | Powers summaries, pondering, background intelligence |
| `OLLAMA_HOST` | Optional | Ollama base URL for local LLM (default: `http://localhost:11434`) |
| `OLLAMA_MODEL` | Optional | Ollama model to use (default: `llama3.3`) |
| `OPENAI_API_KEY` | Recommended | For embeddings (semantic search) via OpenAI text-embedding-3-small |
| `BRAVE_API_KEY` | Optional | Enables web search |
| `DEFAULT_LLM_PROVIDER` | Optional | Override default provider: `deepseek` or `ollama` |
| `MIRA_HOOK_LOG_LEVEL` | Optional | Log level for hook execution (default: warn) |
| `MIRA_FUZZY_SEARCH` | Optional | Enable fuzzy search in hybrid search pipeline (default: true) |
| `MIRA_DISABLE_LLM` | Optional | Set to `1` to disable all LLM calls (forces heuristic fallbacks) |
| `MIRA_PROJECT_PATH` | Optional | Override project path detection (useful when Claude Code hooks are not present) |
| `MIRA_USER_ID` | Optional | User identity override. Identity chain: git config → `MIRA_USER_ID` → system username |

*API keys are optional for core features. Mira's memory, code intelligence, and goal tracking work without any keys. Diff analysis, module summaries, and background insights use heuristic fallbacks (pattern-based parsing, metadata extraction, tool history analysis). Semantic search requires `OPENAI_API_KEY` for embeddings but falls back to fuzzy/keyword search without it.*

### Embeddings Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `MIRA_EMBEDDING_DIMENSIONS` | Output dimensions for OpenAI embeddings | 1536 |

### Environment File Loading

Mira loads only the global environment file:

1. `~/.mira/.env` (Global)

Project `.env` files are NOT loaded for security reasons (a malicious repo could override API keys).

---

## 2. File Locations

### Configuration Files

| File | Purpose |
|------|---------|
| `~/.mira/.env` | Global environment variables |
| `~/.mira/config.toml` | LLM provider configuration |
| `.mcp.json` | MCP server configuration (project) |
| `.codex/config.toml` | Codex CLI configuration (project) |
| `~/.claude/mcp.json` | MCP server configuration (global) |
| `~/.claude/settings.json` | Claude Code settings including hooks |

### Data Storage

| Location | Purpose |
|----------|---------|
| `~/.mira/mira.db` | Main SQLite database (memories, sessions, goals) |
| `~/.mira/mira-code.db` | Code index database (symbols, call graph, embeddings, FTS) |
| `~/.mira/claude-session-id` | Current Claude session ID |

### Project Files

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Core project instructions (always loaded) - see [template](CLAUDE_TEMPLATE.md) |
| `.claude/rules/*.md` | Detailed guidance: tool selection, memory, tasks (always loaded) |
| `.claude/skills/*/SKILL.md` | Reference docs: Context7, tool APIs (loaded on-demand) |
| `CLAUDE.local.md` | Local-only instructions (gitignored) |
| `.miraignore` | Files to exclude from indexing |

See [CLAUDE_TEMPLATE.md](CLAUDE_TEMPLATE.md) for manual setup instructions. (`mira init` is planned but not yet implemented.)

---

## 3. MCP Server Configuration

Configure Mira as an MCP server in `.mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira",
      "args": ["serve"]
    }
  }
}
```

> **Security warning:** Do not put API keys in `.mcp.json` if it is committed to git. Use `~/.mira/.env` instead (see Section 1). If you need to pass environment variables (e.g., for CI or ephemeral setups), you can add an `"env": {}` block, but keep secrets out of version control.

### Options

| Field | Description |
|-------|-------------|
| `command` | Path to Mira binary (or just `mira` if in PATH) |
| `args` | Always `["serve"]` for MCP mode |
| `env` | (Optional) Environment variables to pass to Mira — see security warning above |

### Location

- **Project-specific**: `.mcp.json` in project root
- **Global**: `~/.claude/mcp.json`

### Codex CLI (config.toml)

If you use the Codex CLI, you can configure Mira in `~/.codex/config.toml` (global) or
`.codex/config.toml` (project). Example:

```toml
#:schema https://developers.openai.com/codex/config-schema.json
project_doc_fallback_filenames = ["CLAUDE.md"]

[mcp_servers.mira]
command = "mira"
args = ["serve"]
required = true
startup_timeout_sec = 20
tool_timeout_sec = 90
```

This is additive to Claude setup: `.mcp.json` and `~/.claude/settings.json` hooks continue to work unchanged.

Notes:
- `project_doc_fallback_filenames = ["CLAUDE.md"]` lets Codex reuse existing project instructions without duplicating `AGENTS.md`.
- `MIRA_PROJECT_PATH` (optional env var) lets Mira auto-initialize the project when Claude hooks are not present.
- Mira reads `mcp_servers.*` from Codex config to discover external MCP servers.
- STDIO servers (`command`/`args`) and streamable HTTP servers (`url`) are supported.
- Codex MCP fields supported by Mira's external MCP client: `enabled`, `required`, `startup_timeout_sec`, `startup_timeout_ms`, `tool_timeout_sec`, `enabled_tools`, `disabled_tools`, and `env_vars`.
- HTTP servers support `bearer_token_env_var` for authentication. `http_headers` and `env_http_headers` are parsed but not currently passed to the transport (rmcp's streamable HTTP config only supports bearer auth). OAuth flows are not handled.

---

## 4. Claude Code Hooks

Hooks allow Mira to automatically capture context from Claude Code sessions.

### Auto-Configuration

**Good news:** Hooks are automatically configured when you install Mira via the installer script. No manual setup required.

The installer adds all hooks to `~/.claude/settings.json` using `jq` for JSON manipulation.

### Available Hooks

| Hook | Command | Timeout | Purpose |
|------|---------|---------|---------|
| `SessionStart` | `mira hook session-start` | 10s | Captures session ID, startup vs resume, task list ID |
| `UserPromptSubmit` | `mira hook user-prompt` | 8s | Injects proactive context into prompts |
| `PreToolUse` | `mira hook pre-tool` | 3s | Injects context before Grep/Glob/Read (matcher: `Grep\|Glob\|Read`) |
| `PostToolUse` | `mira hook post-tool` | 5s | Tracks behavior for pattern mining (matcher: `Write\|Edit\|NotebookEdit\|Bash`) |
| `PreCompact` | `mira hook pre-compact` | 30s | Preserves context before summarization |
| `Stop` | `mira hook stop` | 8s | Save session state, auto-export memories to CLAUDE.local.md |
| `SessionEnd` | `mira hook session-end` | 15s | Snapshot tasks on user interrupt |
| `SubagentStart` | `mira hook subagent-start` | 3s | Inject context when subagents spawn |
| `SubagentStop` | `mira hook subagent-stop` | 3s | Capture discoveries from subagent work |
| `PermissionRequest` | `mira hook permission` | 3s | Auto-approve tools based on stored rules |
| `PostToolUseFailure` | `mira hook post-tool-failure` | 5s | Track failures, recall memories after repeated failures |
| `TaskCompleted` | `mira hook task-completed` | 5s | Log completions, auto-complete goal milestones |
| `TeammateIdle` | `mira hook teammate-idle` | 5s | Log teammate idle events for team tracking |

### Manual Configuration

If you need to configure hooks manually, add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{"hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 8}]}],
    "PermissionRequest": [{"hooks": [{"type": "command", "command": "mira hook permission", "timeout": 3}]}],
    "PreToolUse": [{"matcher": "Grep|Glob|Read", "hooks": [{"type": "command", "command": "mira hook pre-tool", "timeout": 3}]}],
    "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit|Bash", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5}]}],
    "PreCompact": [{"hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30, "async": true}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "mira hook stop", "timeout": 8}]}],
    "SessionEnd": [{"hooks": [{"type": "command", "command": "mira hook session-end", "timeout": 15}]}],
    "SubagentStart": [{"hooks": [{"type": "command", "command": "mira hook subagent-start", "timeout": 3}]}],
    "SubagentStop": [{"hooks": [{"type": "command", "command": "mira hook subagent-stop", "timeout": 3, "async": true}]}],
    "PostToolUseFailure": [{"hooks": [{"type": "command", "command": "mira hook post-tool-failure", "timeout": 5, "async": true}]}],
    "TaskCompleted": [{"hooks": [{"type": "command", "command": "mira hook task-completed", "timeout": 5}]}],
    "TeammateIdle": [{"hooks": [{"type": "command", "command": "mira hook teammate-idle", "timeout": 5}]}]
  }
}
```

### What Each Hook Does

**SessionStart**
- Captures Claude's session ID and task list ID
- Detects startup vs resume (session bridging)
- Enables cross-session memory tracking
- Links tool history to sessions

**UserPromptSubmit**
- Fires when user submits a prompt
- Performs semantic search for relevant memories
- Injects proactive suggestions based on behavior patterns
- Context appears automatically without explicit `recall()` calls

**PreToolUse**
- Fires before Grep/Glob/Read tool execution
- Suggests Mira semantic search alternatives
- Injects relevant code context

**PostToolUse**
- Fires after Write/Edit/NotebookEdit/Bash tools complete
- Tracks file access patterns and logs behavior for proactive intelligence
- Detects file-modifying Bash commands (mv, cp, rm, redirects, etc.)
- Detects file conflicts with teammates in Agent Teams
- Note: re-indexing of modified files runs on a background timer, not triggered directly by this hook

**PreCompact**
- Fires before context summarization
- Extracts important decisions, TODOs, issues
- Stores them as memories before they're lost

**Stop**
- Fires when session stops
- Saves session state and snapshots tasks
- Auto-exports memories to CLAUDE.local.md

**SubagentStart**
- Fires when a subagent (Task tool) spawns
- Injects relevant context for the subagent's task
- Provides codebase awareness to subagents

**SubagentStop**
- Fires when a subagent completes
- Captures useful discoveries from subagent work
- Stores insights for future sessions

**PermissionRequest**
- Fires when Claude Code asks the user to approve a tool invocation
- Checks the `permission_rules` table in `~/.mira/mira.db` for a matching rule
- If a rule matches, auto-approves the tool without prompting the user

#### Permission Rule Mechanics

Rules are stored in the `permission_rules` SQLite table (`~/.mira/mira.db`). There is currently no CLI or MCP tool for managing rules — they must be added via direct SQLite manipulation:

```sql
-- Example: auto-approve all Read tool calls
INSERT INTO permission_rules (tool_name, pattern, match_type)
VALUES ('Read', '*', 'glob');
```

**Schema:** `id`, `tool_name`, `pattern`, `match_type` (default `'prefix'`), `scope` (default `'global'`), `created_at`.

**Match types:**
- `exact` — pattern must match the tool input exactly
- `prefix` — tool input must start with the pattern
- `glob` — supports `*` (matches everything) and `prefix*` (prefix matching)

Matching is performed against both the canonical JSON serialization of the tool input and each individual field value (e.g., a `file_path` or `command` string).

> **Security warning:** A broad rule like `tool_name='Bash', pattern='*', match_type='glob'` would auto-approve **all** Bash commands, including destructive ones (`rm -rf /`, `git push --force`, etc.). Always use narrow, specific patterns.

---

## 5. Database Configuration

The SQLite database is automatically created at `~/.mira/mira.db` with secure permissions:

- Directory: `0700` (owner only)
- Database file: `0600` (owner read/write only)

### WAL Mode

Write-Ahead Logging is enabled for better concurrency. This creates additional files:
- `mira.db-wal`
- `mira.db-shm`

These are managed automatically and should not be deleted while Mira is running.

---

## 6. Default LLM Provider

Configure default LLM providers in `~/.mira/config.toml`:

```toml
[llm]
# Provider for background intelligence (summaries, briefings, pondering, code health)
background_provider = "deepseek"
```

### Available Providers

| Provider | Config Value | API Key / Env Var | Default Model |
|----------|--------------|-------------------|---------------|
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` | `deepseek-reasoner` |
| Ollama | `ollama` | `OLLAMA_HOST` (URL, no key) | `llama3.3` |

If not configured, DeepSeek is used as the default when `DEEPSEEK_API_KEY` is available.

---

## 7. Ignoring Files

Create `.miraignore` in your project root to exclude directories from indexing.

**Location:** Project root (next to `.mcp.json`, `CLAUDE.md`, etc.)

**Syntax:**
- One directory name per line (e.g., `my_data`, `generated`)
- Lines starting with `#` are comments
- Blank lines are ignored
- Trailing slashes are treated as part of the name, so use bare names (e.g., `vendor` not `vendor/`)
- **Negation patterns (`!`) are NOT supported**
- **Glob patterns (e.g., `*.min.js`) are NOT supported** — entries are matched as exact directory names

```
# Dependencies
vendor

# Build artifacts
dist
build

# Large data
fixtures
```

**Interaction with `.gitignore`:** Mira independently respects `.gitignore` patterns via the `ignore` crate's `WalkBuilder`. Files ignored by `.gitignore` are always skipped regardless of `.miraignore`. The `.miraignore` file adds **additional** exclusions on top of `.gitignore`.

**Built-in exclusions:** Mira automatically skips common directories (`node_modules`, `target`, `.git`, `dist`, `build`, `vendor`, `__pycache__`, `.next`, `.venv`, etc.) and all hidden directories (starting with `.`). Language-specific directories are also skipped when the project language is detected. See the full list in `crates/mira-server/src/config/ignore.rs`.

---

## 8. Setup Wizard

`mira setup` is the recommended way to configure providers. It handles API key entry, live validation, Ollama auto-detection, and `.env` file management.

### Modes

| Mode | Command | Description |
|------|---------|-------------|
| Interactive | `mira setup` | Guided wizard: choose providers, enter keys, validate, detect Ollama |
| Non-interactive | `mira setup --yes` | Auto-detects Ollama, skips API key prompts. Good for CI/scripted installs |
| Check | `mira setup --check` | Read-only validation of current configuration |

### What It Does

1. Prompts for LLM provider (DeepSeek) with live API key validation
2. Prompts for embeddings provider (OpenAI) with validation
3. Optionally configures Brave Search
4. Auto-detects Ollama and lists available models for background tasks
5. Merges new keys with existing `~/.mira/.env` (never overwrites unrelated keys)
6. Sets `background_provider = "ollama"` in `~/.mira/config.toml` if Ollama is selected
7. Backs up existing `.env` before writing

### Non-Interactive Details

`mira setup --yes` is designed for scripted installs. It skips all API key prompts and auto-selects the first Ollama model if available. If no Ollama is detected and no existing provider keys are configured, it reports "No providers configured" and exits cleanly.

---

## Quick Reference

### Minimal Setup

```bash
mira setup          # Interactive wizard handles everything
```

Or manually:

```bash
# ~/.mira/.env
DEEPSEEK_API_KEY=sk-...

# Add to project's .mcp.json
{
  "mcpServers": {
    "mira": {
      "command": "mira",
      "args": ["serve"]
    }
  }
}
```

### Full Setup

```bash
mira setup          # Configure API keys and detect Ollama

# Hooks are auto-configured by installer
```
