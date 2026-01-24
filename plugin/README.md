# Mira Plugin for Claude Code

Semantic memory and code intelligence for Claude Code. Provides persistent memory across sessions, expert consultations, goal tracking, and proactive context injection.

## Features

- **Persistent Memory**: Remember decisions, preferences, and context across sessions
- **Semantic Code Search**: Find code by meaning, not just text
- **Expert Consultations**: Get second opinions from AI architects, security analysts, code reviewers
- **Goal Tracking**: Track multi-session objectives with milestones
- **Proactive Context**: Automatic context injection based on your prompts

## Installation

### From Source

```bash
# Build Mira
cd /path/to/mira
cargo build --release

# Ensure mira is in PATH
export PATH="$PATH:/path/to/mira/target/release"

# Load plugin in Claude Code
claude --plugin-dir /path/to/mira/plugin
```

### Environment Variables

Set these in your shell or `.env`:

```bash
export DEEPSEEK_API_KEY="your-key"    # For expert consultations
export GOOGLE_API_KEY="your-key"       # For embeddings
```

## Skills

| Skill | Description |
|-------|-------------|
| `/mira:search <query>` | Semantic code search |
| `/mira:recap` | Get session recap |
| `/mira:goals [command]` | Manage goals and milestones |

## MCP Tools

The plugin bundles the Mira MCP server with these tools:

### Memory
- `remember` - Store facts for future recall
- `recall` - Search memories semantically
- `forget` - Delete a memory

### Code Intelligence
- `search_code` - Semantic code search
- `get_symbols` - Extract symbols from a file
- `find_callers` - Find functions that call a given function
- `find_callees` - Find functions called by a given function
- `check_capability` - Check if a feature exists in codebase

### Expert Consultation
- `consult_architect` - System design advice
- `consult_code_reviewer` - Code quality review
- `consult_security` - Security analysis
- `consult_plan_reviewer` - Validate implementation plans
- `consult_scope_analyst` - Find missing requirements

### Goals
- `goal` - Create, list, update goals and milestones

### Session
- `session_start` - Initialize session with project context
- `get_session_recap` - Get preferences, context, goals

## Hooks

The plugin uses Claude Code hooks for bidirectional communication:

| Hook | Purpose |
|------|---------|
| `SessionStart` | Inject session recap and active goals |
| `UserPromptSubmit` | Auto-recall relevant memories and code context |
| `PostToolUse` | Track file changes, queue re-indexing |
| `PreCompact` | Extract important context before summarization |
| `Stop` | Save session state, check goal progress |

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

MIT
