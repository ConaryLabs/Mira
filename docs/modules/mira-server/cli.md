# cli

Command-line interface using clap. Defines subcommands and dispatches to handlers.

## Commands

| Command | Purpose |
|---------|---------|
| `serve` | Start the MCP server (stdio or WebSocket mode) |
| `tool` | Run a single tool invocation |
| `index` | Index a project's code |
| `hook` | Handle Claude Code lifecycle hooks |
| `debug-carto` | Debug cartographer module detection |
| `debug-session` | Debug session start output |

## Sub-modules

| Module | Purpose |
|--------|---------|
| `serve` | Server startup and configuration |
| `tool` | Single tool execution |
| `index` | Indexing command |
| `clients` | Client setup utilities |
| `debug` | Debug subcommands |

## Key Export

`get_db_path()` - Returns the database path (`~/.mira/mira.db`).
