# Mira Configuration Guide

This guide details all configuration options for Mira, including environment variables, file locations, hook setup, and expert customization.

---

## 1. Environment Variables

Mira uses environment variables for API keys and configuration. These can be set globally in `~/.mira/.env` or locally in a project's `.env` file.

### Intelligence Features

| Variable | Required | Description |
|----------|----------|-------------|
| `DEEPSEEK_API_KEY` | Recommended | Powers experts, summaries, capabilities, documentation (default provider) |
| `GEMINI_API_KEY` | Recommended | For embeddings (semantic search) and as alternative expert provider |
| `DEFAULT_LLM_PROVIDER` | Optional | Override default provider: `deepseek` or `gemini` |
| `MIRA_FUZZY_FALLBACK` | Optional | Enable fuzzy fallback search when embeddings are unavailable (default: true) |
| `MIRA_DISABLE_LLM` | Optional | Set to `1` to disable all LLM calls (forces heuristic fallbacks) |
| `MIRA_USER_ID` | Optional | User identity override (defaults to git config user.email) |

*API keys are optional. Mira runs without any keys using heuristic fallbacks — diff analysis uses pattern-based parsing, module summaries use metadata extraction, and background insights use tool history analysis. Expert consultation works without keys via MCP Sampling (uses the host client). Semantic search requires `GEMINI_API_KEY` for embeddings but falls back to fuzzy/keyword search without it. At least one provider key is recommended for full LLM-powered intelligence features.*

### Embeddings Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `MIRA_EMBEDDING_DIMENSIONS` | Output dimensions for Google embeddings | 1536 |
| `MIRA_EMBEDDING_TASK_TYPE` | Task type for embeddings (see below) | `SEMANTIC_SIMILARITY` |

**Embedding Task Types:** `SEMANTIC_SIMILARITY` (default), `RETRIEVAL_DOCUMENT`, `RETRIEVAL_QUERY`, `CODE_RETRIEVAL_QUERY`

### Environment File Loading

Mira loads environment files in this order (later overrides earlier):

1. `~/.mira/.env` (Global)
2. `.env` (Project directory)

---

## 2. File Locations

### Configuration Files

| File | Purpose |
|------|---------|
| `~/.mira/.env` | Global environment variables |
| `~/.mira/config.toml` | LLM provider configuration |
| `.env` | Project-local environment variables |
| `.mcp.json` | MCP server configuration (project) |
| `~/.claude/mcp.json` | MCP server configuration (global) |
| `~/.claude/settings.json` | Claude Code settings including hooks |

### Data Storage

| Location | Purpose |
|----------|---------|
| `~/.mira/mira.db` | Main SQLite database (memories, sessions, experts, goals) |
| `~/.mira/mira-code.db` | Code index database (symbols, call graph, embeddings, FTS) |
| `~/.mira/claude-session-id` | Current Claude session ID |

### Project Files

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Core project instructions (always loaded) - see [template](CLAUDE_TEMPLATE.md) |
| `.claude/rules/*.md` | Detailed guidance: tool selection, memory, tasks, experts (always loaded) |
| `.claude/skills/*/SKILL.md` | Reference docs: Context7, tool APIs (loaded on-demand) |
| `CLAUDE.local.md` | Local-only instructions (gitignored) |
| `.miraignore` | Files to exclude from indexing |

Run `mira init` in your project to create all instruction files automatically.

---

## 3. MCP Server Configuration

Configure Mira as an MCP server in `.mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "/path/to/mira",
      "args": ["serve"],
      "env": {
        "DEEPSEEK_API_KEY": "sk-...",
        "GEMINI_API_KEY": "..."
      }
    }
  }
}
```

### Options

| Field | Description |
|-------|-------------|
| `command` | Path to Mira binary (or just `mira` if in PATH) |
| `args` | Always `["serve"]` for MCP mode |
| `env` | Environment variables to pass to Mira |

### Location

- **Project-specific**: `.mcp.json` in project root
- **Global**: `~/.claude/mcp.json`

---

## 4. Claude Code Hooks

Hooks allow Mira to automatically capture context from Claude Code sessions.

### Auto-Configuration

**Good news:** Hooks are automatically configured when you install Mira via the installer script. No manual setup required.

The installer adds all hooks to `~/.claude/settings.json` using `jq` for JSON manipulation.

### Available Hooks

| Hook | Command | Timeout | Purpose |
|------|---------|---------|---------|
| `SessionStart` | `mira hook session-start` | 10s | Captures session ID for tracking |
| `UserPromptSubmit` | `mira hook user-prompt` | 5s | Injects proactive context into prompts |
| `PostToolUse` | `mira hook post-tool` | 5s | Tracks behavior for pattern mining (scoped to `Write\|Edit\|NotebookEdit`) |
| `PreCompact` | `mira hook pre-compact` | 30s | Preserves context before summarization |
| `Stop` | `mira hook stop` | 5s | Save session state, auto-export memories to CLAUDE.local.md, check goal progress |

Additional hooks (not auto-configured):

| Hook | Command | Purpose |
|------|---------|---------|
| `Permission` | `mira hook permission` | Auto-approve tools based on stored rules |

### Manual Configuration

If you need to configure hooks manually, add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5000}]}],
    "UserPromptSubmit": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 5000}]}],
    "SessionStart": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10000}]}],
    "PreCompact": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30000}]}],
    "Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook stop", "timeout": 5000}]}]
  }
}
```

### What Each Hook Does

**SessionStart**
- Captures Claude's session ID
- Enables cross-session memory tracking
- Links tool history to sessions

**UserPromptSubmit**
- Fires when user submits a prompt
- Performs semantic search for relevant memories
- Injects proactive suggestions based on behavior patterns
- Context appears automatically without explicit `recall()` calls

**PostToolUse**
- Fires after Write/Edit/Read tools complete
- Tracks file access patterns for behavior mining
- Queues modified files for re-indexing
- Provides contextual hints about changed files

**PreCompact**
- Fires before context summarization
- Extracts important decisions, TODOs, issues
- Stores them as memories before they're lost

---

## 5. Expert Configuration

Mira's experts can be customized per role with different providers, models, and prompts.

### Expert Roles

| Role | Default Provider | Purpose |
|------|-----------------|---------|
| `architect` | deepseek | System design and tradeoffs |
| `plan_reviewer` | deepseek | Implementation plan validation |
| `scope_analyst` | deepseek | Requirements and edge cases |
| `code_reviewer` | deepseek | Code quality and bugs |
| `security` | deepseek | Vulnerability assessment |

### Using `expert(action="configure")`

**List current configurations:**
```
expert(action="configure", config_action="list")
```

**Set provider and model for an expert:**
```
expert(
  action="configure",
  config_action="set",
  role="architect",
  provider="gemini",
  model="gemini-2.5-pro"
)
```

**Customize an expert's system prompt:**
```
expert(
  action="configure",
  config_action="set",
  role="code_reviewer",
  prompt="Focus on Rust memory safety and ownership patterns."
)
```

**List available providers:**
```
expert(action="configure", config_action="providers")
```

**Revert to defaults:**
```
expert(action="configure", config_action="delete", role="architect")
```

### Provider Options

| Provider | Default Model | Best For |
|----------|---------------|----------|
| `deepseek` | `deepseek-reasoner` | Extended reasoning, multi-step analysis |
| `gemini` | `gemini-3-pro-preview` | Cost-effective, good reasoning |

Use `expert(action="configure", config_action="providers")` to see available providers and their configured models.

---

## 6. Database Configuration

The SQLite database is automatically created at `~/.mira/mira.db` with secure permissions:

- Directory: `0700` (owner only)
- Database file: `0600` (owner read/write only)

### WAL Mode

Write-Ahead Logging is enabled for better concurrency. This creates additional files:
- `mira.db-wal`
- `mira.db-shm`

These are managed automatically and should not be deleted while Mira is running.

---

## 7. Default LLM Provider

Configure default LLM providers in `~/.mira/config.toml`:

```toml
[llm]
# Provider for expert tools (expert(action="consult", roles=["architect"]), etc.)
expert_provider = "deepseek"

# Provider for background intelligence (summaries, briefings, capabilities, code health)
background_provider = "deepseek"
```

### Available Providers

| Provider | Config Value | API Key Env Var | Default Model |
|----------|--------------|-----------------|---------------|
| DeepSeek | `deepseek` | `DEEPSEEK_API_KEY` | `deepseek-reasoner` |
| Gemini | `gemini` | `GEMINI_API_KEY` | `gemini-3-pro-preview` |

If not configured, DeepSeek is used as the default when `DEEPSEEK_API_KEY` is available.

---

## 8. Ignoring Files

Create `.miraignore` in your project root to exclude files from indexing:

```
# Dependencies
node_modules/
target/
vendor/

# Build artifacts
dist/
build/
*.min.js

# Large files
*.pb
*.bin
```

The syntax is similar to `.gitignore`. Mira also respects `.gitignore` patterns.

---

## Quick Reference

### Minimal Setup

```bash
# Set required API key
export DEEPSEEK_API_KEY="sk-..."

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
# ~/.mira/.env
DEEPSEEK_API_KEY=sk-...
GEMINI_API_KEY=...

# Hooks are auto-configured by installer
# Configure experts per project as needed
```
