# cli

Command-line interface using clap. Defines subcommands and dispatches to handlers.

## Commands

| Command | Purpose |
|---------|---------|
| `serve` | Start the MCP server (stdio) |
| `tool` | Run a single tool invocation |
| `index` | Index a project's code |
| `setup` | Interactive setup wizard for API keys and providers |
| `hook` | Handle Claude Code lifecycle hooks |
| `debug-carto` | Debug cartographer module detection |
| `debug-session` | Debug session start output |

### Setup Flags

| Flag | Description |
|------|-------------|
| `--check` | Read-only validation of current configuration |
| `--yes` | Non-interactive mode: auto-detect Ollama, skip API key prompts |

## Sub-modules

| Module | Purpose |
|--------|---------|
| `serve` | Server startup and configuration |
| `tool` | Single tool execution |
| `index` | Indexing command |
| `setup` | Setup wizard with API key validation and Ollama detection |
| `clients` | Client setup utilities |
| `debug` | Debug subcommands |

## Key Export

`get_db_path()` - Returns the database path (`~/.mira/mira.db`).
