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

*At least one provider key (DeepSeek, Gemini, or OpenAI) is required for intelligence features. DeepSeek is the default. Mira runs without any keys but with reduced functionality (no experts, no summaries, no semantic search).

### Embeddings Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `MIRA_EMBEDDING_DIMENSIONS` | Output dimensions for Google embeddings | 768 |

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
| `.env` | Project-local environment variables |
| `.mcp.json` | MCP server configuration (project) |
| `~/.claude/mcp.json` | MCP server configuration (global) |
| `~/.claude/settings.json` | Claude Code settings including hooks |

### Data Storage

| Location | Purpose |
|----------|---------|
| `~/.mira/mira.db` | SQLite database (memories, indexes, history) |
| `~/.mira/claude-session-id` | Current Claude session ID |

### Project Files

| File | Purpose |
|------|---------|
| `CLAUDE.md` | Project instructions (checked into git) |
| `CLAUDE.local.md` | Local-only instructions (gitignored) |
| `.miraignore` | Files to exclude from indexing |

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

### Available Hooks

| Hook | Command | Purpose |
|------|---------|---------|
| `SessionStart` | `mira hook session-start` | Captures session ID for tracking |
| `PreCompact` | `mira hook pre-compact` | Extracts decisions/TODOs before summarization |

### Configuration

Add to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "/path/to/mira hook session-start",
        "timeout": 3000
      }]
    }],
    "PreCompact": [{
      "matcher": "",
      "hooks": [{
        "type": "command",
        "command": "/path/to/mira hook pre-compact",
        "timeout": 5000
      }]
    }]
  }
}
```

### What Each Hook Does

**SessionStart**
- Captures Claude's session ID
- Enables cross-session memory tracking
- Links tool history to sessions

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
| `documentation_writer` | deepseek | Documentation generation |

### Using `configure_expert`

**List current configurations:**
```
configure_expert(action="list")
```

**Set provider and model for an expert:**
```
configure_expert(
  action="set",
  role="architect",
  provider="openai",
  model="gpt-4o"
)
```

**Customize an expert's system prompt:**
```
configure_expert(
  action="set",
  role="code_reviewer",
  prompt="Focus on Rust memory safety and ownership patterns."
)
```

**List available providers:**
```
configure_expert(action="providers")
```

**Revert to defaults:**
```
configure_expert(action="delete", role="architect")
```

### Provider Options

| Provider | Default Model | Best For |
|----------|---------------|----------|
| `deepseek` | `deepseek-reasoner` | Extended reasoning, multi-step analysis |
| `openai` | `gpt-5.2` | General purpose |
| `gemini` | `gemini-3-pro-preview` | Cost-effective, good reasoning |

Use `configure_expert(action="providers")` to see available providers and their configured models.

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

## 7. Proxy Configuration (Experimental)

The LLM proxy allows routing requests through multiple backends.

### Config File

Create `~/.config/mira/proxy.toml`:

```toml
[backends.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com"
api_key_env = "ANTHROPIC_API_KEY"
api_type = "anthropic"

[backends.deepseek]
name = "DeepSeek"
base_url = "https://api.deepseek.com"
api_key_env = "DEEPSEEK_API_KEY"
api_type = "anthropic"

[backends.deepseek.pricing]
input_per_million = 0.14
output_per_million = 2.19

default_backend = "anthropic"
```

### Backend Options

| Field | Description |
|-------|-------------|
| `name` | Display name |
| `base_url` | API endpoint |
| `api_key_env` | Environment variable containing API key |
| `api_type` | `anthropic` or `openai` |
| `model_map` | Optional model name mapping |
| `pricing` | Cost per million tokens for tracking |

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

# Install hooks in ~/.claude/settings.json
# Configure experts per project as needed
```
