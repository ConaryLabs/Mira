# Mira Plugin for Claude Code

Semantic memory and code intelligence for Claude Code. Provides persistent memory across sessions, expert consultations, goal tracking, and proactive context injection.

## Features

- **Persistent Memory**: Remember decisions, preferences, and context across sessions
- **Semantic Code Search**: Find code by meaning, not just text
- **Expert Consultations**: Get second opinions from AI architects, security analysts, code reviewers
- **Goal Tracking**: Track multi-session objectives with milestones
- **Proactive Context**: Automatic context injection based on your prompts

## Installation

### Plugin Marketplace (Recommended)

Install via the Claude Code plugin marketplace:

```bash
claude plugin marketplace add ConaryLabs/Mira
claude plugin install mira@mira
```

The `mira` binary is **auto-downloaded** on first launch — no manual installation needed. The wrapper script (`plugin/bin/mira-wrapper`) handles downloading the correct binary for your platform to `~/.mira/bin/mira`.

After the first launch, add your API keys to `~/.mira/.env`:

```bash
DEEPSEEK_API_KEY=your-key-here  # https://platform.deepseek.com/api_keys
GEMINI_API_KEY=your-key-here    # https://aistudio.google.com/app/apikey
```

> **No API keys?** Expert consultation works without keys via MCP Sampling (uses the host client). All other tools work with heuristic fallbacks.

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
| `/mira:search <query>` | Semantic code search |
| `/mira:recap` | Get session recap |
| `/mira:goals [command]` | Manage goals and milestones |

## MCP Tools

The plugin bundles the Mira MCP server with 11 action-based tools:

| Tool | Actions | Purpose |
|------|---------|---------|
| `memory` | remember, recall, forget | Persistent semantic memory |
| `code` | search, symbols, callers, callees, dependencies, patterns, tech_debt | Code intelligence |
| `project` | start, set, get | Project/session management |
| `session` | history, recap, usage, insights | Session history and analytics |
| `expert` | consult, configure | Expert consultation (architect, security, code_reviewer, plan_reviewer, scope_analyst) |
| `goal` | create, list, update, add_milestone, complete_milestone, ... | Cross-session goal tracking |
| `finding` | list, get, review, stats, patterns, extract | Code review findings |
| `documentation` | list, get, complete, skip, inventory, scan | Documentation management |
| `index` | project, file, status, compact, summarize, health | Code indexing and health |
| `analyze_diff` | — | Semantic git diff analysis |
| `tasks` | list, get, cancel | Async background operations |

All tools return structured JSON via MCP outputSchema.

## Hooks

The plugin uses Claude Code hooks for bidirectional communication:

| Hook | Purpose |
|------|---------|
| `SessionStart` | Inject session recap and active goals |
| `UserPromptSubmit` | Auto-recall relevant memories and code context |
| `PostToolUse` | Track file changes, queue re-indexing |
| `PreCompact` | Extract important context before summarization |
| `Stop` | Save session state, auto-export CLAUDE.local.md, check goal progress |

## Configuration

Create `~/.mira/config.toml` to customize behavior:

```toml
[hooks]
enabled = true

[hooks.user_prompt]
min_prompt_length = 10  # Skip very short prompts

[hooks.post_tool]
security_scan = true    # Run quick security checks

[hooks.stop]
auto_continue_goals = false  # Don't auto-continue incomplete goals
```

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
