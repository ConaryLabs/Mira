# docs/INSTALLATION.md
# Installation

## Quick Install (Recommended)

```bash
claude plugin install mira
```

Then optionally configure providers:

```bash
mira setup          # interactive wizard with live validation + Ollama auto-detection
mira setup --yes    # non-interactive (CI/scripted installs)
mira setup --check  # read-only validation
```

To verify: start a new Claude Code session in any project. You should see "Mira: Loading session context..." in the status bar.

## Script Install

```bash
curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash
```

Detects your OS, downloads the binary, installs the Claude Code plugin (auto-configures all hooks and skills), and creates `~/.mira/`.

## Manual Binary Install

<details>
<summary>Platform-specific downloads</summary>

**Linux (x86_64):**
```bash
curl -L https://github.com/ConaryLabs/Mira/releases/latest/download/mira-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv mira /usr/local/bin/
```

**macOS (Apple Silicon):**
```bash
curl -L https://github.com/ConaryLabs/Mira/releases/latest/download/mira-aarch64-apple-darwin.tar.gz | tar xz
sudo mv mira /usr/local/bin/
```

**macOS (Intel):**
```bash
curl -L https://github.com/ConaryLabs/Mira/releases/latest/download/mira-x86_64-apple-darwin.tar.gz | tar xz
sudo mv mira /usr/local/bin/
```

**Windows (PowerShell):**
```powershell
Invoke-WebRequest -Uri "https://github.com/ConaryLabs/Mira/releases/latest/download/mira-x86_64-pc-windows-msvc.zip" -OutFile mira.zip
Expand-Archive mira.zip -DestinationPath .
Remove-Item mira.zip
Move-Item mira.exe C:\Tools\  # Or another directory in your PATH
```

</details>

Then install the plugin:

```bash
claude plugin install ConaryLabs/Mira
```

## Install via Cargo (MCP Server Only)

```bash
cargo install --git https://github.com/ConaryLabs/Mira.git
```

Then add to your project's `.mcp.json`:

```json
{
  "mcpServers": {
    "mira": {
      "command": "mira",
      "args": ["serve"]
    }
  }
}
```

If you use Codex CLI, see [Configuration - Codex CLI](CONFIGURATION.md#codex-cli-configtoml) for setup.

## Build from Source

```bash
git clone https://github.com/ConaryLabs/Mira.git
cd Mira
cargo build --release
```

Binary lands at `target/release/mira`. Add to `.mcp.json`:

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

## Plugin vs MCP Server

The **plugin** (quick install) is the full experience -- hooks and skills auto-configured, context injected on every prompt.

The **MCP server** (cargo install / build from source) gives you the core tools. You'll need to add hooks manually for context injection and session tracking. See [Configuration - Hooks](CONFIGURATION.md#4-claude-code-hooks) for the full hook configuration.

## Adding Mira Instructions to Your Project

See **[CLAUDE_TEMPLATE.md](CLAUDE_TEMPLATE.md)** for a recommended `CLAUDE.md` layout that teaches Claude Code how to use Mira's tools:

- `CLAUDE.md` -- Core identity, anti-patterns, build commands (always loaded)
- `.claude/rules/` -- Tool selection, memory, tasks (always loaded)

---

## CLI Reference

```bash
mira setup                # Interactive configuration wizard
mira setup --check        # Validate current configuration
mira index                # Index current project for semantic code search
mira index --no-embed     # Index without embeddings (faster, keyword-only search)
mira debug-session        # Debug project(action="start") output
mira debug-carto          # Debug cartographer module detection
mira config show          # Display current configuration
mira config set <k> <v>   # Update a configuration value
mira statusline           # Status line for Claude Code's status bar (auto-installed)
mira cleanup              # Data retention dry-run (sessions, analytics, behavior)
mira cleanup --execute    # Delete accumulated data (add --yes to skip confirmation)
```

---

## Troubleshooting

### Semantic search not working

Make sure `OPENAI_API_KEY` is set in `~/.mira/.env`. Without it, search falls back to keyword and fuzzy matching.

### MCP connection issues

1. Check the binary path in `.mcp.json` is absolute
2. Run `mira serve` directly and confirm it starts without errors
3. Check Claude Code logs for MCP errors

### Memory not persisting

Project context is auto-initialized from Claude Code's working directory. Use the project tool with `action="get"` to verify Mira is running and that the working directory matches your project root.
