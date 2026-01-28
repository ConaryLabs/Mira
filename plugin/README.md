# Mira Plugin for Claude Code

Semantic memory and code intelligence for Claude Code. Provides persistent memory across sessions, expert consultations, goal tracking, and proactive context injection.

## Features

- **Persistent Memory**: Remember decisions, preferences, and context across sessions
- **Semantic Code Search**: Find code by meaning, not just text
- **Expert Consultations**: Get second opinions from AI architects, security analysts, code reviewers
- **Goal Tracking**: Track multi-session objectives with milestones
- **Proactive Context**: Automatic context injection based on your prompts

## Installation

### 1. Build Mira

```bash
cd /path/to/mira
cargo build --release
```

### 2. Configure Plugin Files

Copy the example config files and update paths to your mira binary:

```bash
cd /path/to/mira/plugin

# MCP server config
cp .mcp.json.example .mcp.json

# Hooks config
cp hooks/hooks.json.example hooks/hooks.json
```

Edit both files and replace `/path/to/mira` with your actual path:

```bash
# Example: if mira is at /home/user/mira
sed -i 's|/path/to/mira|/home/user/mira|g' .mcp.json hooks/hooks.json
```

### 3. Set Environment Variables

Add to your shell profile (`~/.bashrc` or `~/.zshrc`):

```bash
export DEEPSEEK_API_KEY="your-key"    # For expert consultations
export GEMINI_API_KEY="your-key"       # For embeddings
```

### 4. Run Claude Code with Plugin

```bash
claude --plugin-dir /path/to/mira/plugin
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
- `consult_experts` - Get advice from specialized experts:
  - `architect` - System design advice
  - `code_reviewer` - Code quality review
  - `security` - Security analysis
  - `plan_reviewer` - Validate implementation plans
  - `scope_analyst` - Find missing requirements
  - `documentation_writer` - Documentation generation

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

Verify hooks work correctly (replace `/path/to/mira` with your actual path):

```bash
MIRA=/path/to/mira/target/release/mira

# Test session-start hook
echo '{}' | $MIRA hook session-start

# Test user-prompt hook
echo '{"prompt": "test"}' | $MIRA hook user-prompt

# Test post-tool hook
echo '{"tool_name": "Write", "tool_input": {"file_path": "/tmp/test.rs"}}' | $MIRA hook post-tool

# Test pre-compact hook
echo '{}' | $MIRA hook pre-compact

# Test stop hook
echo '{}' | $MIRA hook stop
```

## License

Apache-2.0
