# Mira — The Intelligence Layer for Claude Code

Claude Code is powerful but amnesiac. Mira gives it persistent memory, semantic code understanding, and background intelligence — all running locally.

## Features

- **Persistent Memory**: Remember decisions, preferences, and context across sessions
- **Semantic Code Search**: Find code by meaning, not just text
- **Goal Tracking**: Track multi-session objectives with milestones
- **Proactive Context**: Automatic context injection based on your prompts

## Installation

### Plugin Marketplace (Recommended)

Install via the Claude Code plugin marketplace:

```bash
claude plugin marketplace add ConaryLabs/Mira
claude plugin install mira@mira
```

The `mira` binary is **auto-downloaded** on first launch and **auto-updated** daily — no manual intervention needed. The wrapper script (`plugin/bin/mira-wrapper`) handles downloading the correct binary for your platform to `~/.mira/bin/mira`, checks for new versions every 24 hours, and verifies downloads via SHA256 checksums.

**Version pinning:** Set `MIRA_VERSION_PIN=0.6.9` to lock to a specific version and skip auto-updates.

After the first launch, configure providers:

```bash
mira setup  # Interactive wizard with API key validation and Ollama detection
```

Or manually add API keys to `~/.mira/.env`:

```bash
DEEPSEEK_API_KEY=your-key-here  # https://platform.deepseek.com/api_keys
OPENAI_API_KEY=your-key-here    # https://platform.openai.com/api-keys
```

> **No API keys?** Core features work without keys using heuristic fallbacks. Ollama can power background tasks locally without any API keys.

### Alternative: Standalone Install

If you prefer to install the binary system-wide (adds `mira` to your PATH):

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

### Alternative: Build from Source

```bash
git clone https://github.com/ConaryLabs/Mira.git
cd Mira
cargo build --release
sudo cp target/release/mira /usr/local/bin/
```

Then run Claude Code with the plugin directory:

```bash
claude --plugin-dir /path/to/Mira/plugin
```

## Skills

| Skill | Description |
|-------|-------------|
| `/mira:help` | List all available commands |
| `/mira:search <query>` | Semantic code search |
| `/mira:recap` | Get session recap |
| `/mira:goals [command]` | Manage goals and milestones |
| `/mira:diff [--from REF] [--to REF]` | Semantic diff analysis |
| `/mira:insights` | Surface background analysis |
| `/mira:remember <content>` | Quick memory storage |
| `/mira:experts` | Expert consultation via Agent Teams |
| `/mira:full-cycle` | End-to-end review, implementation, and QA |
| `/mira:qa-hardening` | Production readiness review |
| `/mira:refactor` | Safe code restructuring with validation |
| `/mira:status` | Quick health check: index, storage, goals |

## MCP Tools

The plugin bundles the Mira MCP server with 8 MCP tools:

| Tool | Actions | Purpose |
|------|---------|---------|
| `memory` | remember, recall, forget, archive | Persistent semantic memory |
| `code` | search, symbols, callers, callees | Code intelligence: semantic search + call graph |
| `diff` | *(single purpose)* | Semantic git diff analysis with impact assessment |
| `project` | start, get | Project/session management |
| `session` | current_session, recap, insights, dismiss_insight | Session context and insights |
| `goal` | create, list, update, add_milestone, complete_milestone, ... | Cross-session goal tracking |
| `index` | project, file, status | Code indexing |
| `recipe` | list, get | Agentic team workflow recipes |

Additional actions (documentation, team, and advanced session/code/index actions) are available via `mira tool <name> '<json>'` CLI.

All tools return structured JSON via MCP outputSchema.

## Hooks

The plugin uses Claude Code hooks for bidirectional communication:

| Hook | Purpose |
|------|---------|
| `SessionStart` | Initialize session, inject recap and active goals |
| `UserPromptSubmit` | Auto-recall relevant memories and code context |
| `PreToolUse` | Inject context before Grep/Glob/Read searches |
| `PostToolUse` | Track file changes, queue re-indexing |
| `PreCompact` | Extract important context before summarization |
| `Stop` | Save session state, auto-export CLAUDE.local.md |
| `SessionEnd` | Snapshot tasks on user interrupt |
| `SubagentStart` | Inject context when subagents spawn |
| `SubagentStop` | Capture discoveries from subagent work |
| `PermissionRequest` | Handle permission checks |

## Testing

Verify hooks work correctly:

```bash
# Test session-start hook
echo '{}' | mira hook session-start

# Test user-prompt hook
echo '{"prompt": "test"}' | mira hook user-prompt

# Test post-tool hook
echo '{"tool_name": "Write", "tool_input": {"file_path": "/tmp/test.rs"}}' | mira hook post-tool

# Test pre-compact hook
echo '{}' | mira hook pre-compact

# Test stop hook
echo '{}' | mira hook stop
```

## License

Apache-2.0
