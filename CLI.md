# Mira CLI Cheat Sheet

Quick reference for Mira command-line interface.

## Quick Start

```bash
# Interactive mode (REPL)
mira

# One-shot query (non-interactive)
mira -p "explain this function"

# Continue most recent session
mira -c

# Start with specific project
mira --project /path/to/repo
```

## Command-Line Flags

| Flag | Description |
|------|-------------|
| `-p, --print` | Non-interactive mode, print response and exit |
| `-c, --continue-session` | Continue most recent session |
| `-r, --resume [ID]` | Resume specific session (shows picker if no ID) |
| `--fork [ID]` | Fork from a session to create new branch |
| `--verbose` | Show tool execution details |
| `--show-thinking` | Display model reasoning process |
| `--output-format <fmt>` | Output format: `text`, `json`, `stream-json` |
| `--project <PATH>` | Set project root directory |
| `--max-turns <N>` | Limit turns in non-interactive mode |
| `--allowedTools <tools>` | Comma-separated list of allowed tools |
| `--disallowedTools <tools>` | Comma-separated list of blocked tools |
| `-h, --help` | Show help |
| `-V, --version` | Show version |

## REPL Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/quit` or `/exit` | Exit the REPL |
| `/clear` | Clear the screen |
| `/sessions` | List all sessions |
| `/session` | Show current session info |
| `/commands` | List available slash commands |
| `/checkpoints` | List conversation checkpoints |
| `/rewind <id>` | Rewind to a checkpoint |

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+C` | Cancel current operation |
| `Ctrl+D` | Exit REPL |
| `Up/Down` | Navigate input history |

## Custom Commands

Create custom slash commands in `.mira/commands/` (project) or `~/.mira/commands/` (global).

**Example:** `.mira/commands/review.md`
```markdown
Review the following code for bugs, security issues, and improvements:

$ARGUMENTS
```

**Usage:**
```bash
/review src/main.rs
```

The `$ARGUMENTS` placeholder is replaced with everything after the command name.

## Session Management

**Creating Sessions:**
- New session starts automatically on launch
- Each session has a unique ID and optional title

**Session Picker:**
```
> Recent Sessions
  [a1b2c3d4] 2024-01-15 - Refactoring auth module
  [e5f6g7h8] 2024-01-14 - Bug fix in parser

  Use arrow keys to navigate, Enter to select
```

**Forking:**
Fork creates a new session from a checkpoint, preserving the original:
```bash
mira --fork a1b2c3d4
```

## Configuration

Config file: `~/.mira/config.json`

```json
{
  "theme": "dark",
  "defaultProject": "/home/user/projects/main",
  "showThinking": false,
  "verbose": false
}
```

## Common Workflows

**Quick question:**
```bash
mira -p "what does the --nocapture flag do in cargo test?"
```

**Start project work:**
```bash
cd ~/projects/myapp
mira
```

**Continue where you left off:**
```bash
mira -c
```

**Review with verbose output:**
```bash
mira --verbose -p "review src/auth.rs for security issues"
```

**Fork for experimentation:**
```bash
# Fork a session to try different approach
mira --fork abc123

# Original session unchanged, new session for experiments
```

## Output Formats

| Format | Use Case |
|--------|----------|
| `text` | Human-readable (default) |
| `json` | Single JSON object on completion |
| `stream-json` | Newline-delimited JSON events |

**Example with JSON output:**
```bash
mira -p "list files in src/" --output-format json | jq '.response'
```

## Environment Variables

| Variable | Description |
|----------|-------------|
| `MIRA_PROJECT` | Default project directory |
| `MIRA_CONFIG` | Custom config file path |
| `NO_COLOR` | Disable colored output |

## Tips

- Use `-p` for scripting and automation
- Session IDs can be shortened (first 8 chars usually sufficient)
- Custom commands in project `.mira/commands/` override global ones
- Press `Ctrl+C` during streaming to cancel without exiting
