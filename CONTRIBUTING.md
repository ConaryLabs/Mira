# Contributing to Mira

Thanks for your interest in contributing to Mira!

## Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/ConaryLabs/Mira.git
   cd Mira
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run tests:
   ```bash
   cargo test
   ```

4. Set up API keys for full functionality:
   ```bash
   mkdir -p ~/.mira
   cp .env.example ~/.mira/.env
   # Edit ~/.mira/.env with your API keys
   ```

## Project Structure

```
crates/
  mira-server/       # Main server crate
    src/
      background/    # Background workers (embeddings, summaries, health checks)
      cartographer/  # Codebase mapping and module detection
      cli/           # Command-line interface and subcommands
      config/        # Configuration management (config.toml, env loading)
      context/       # Proactive context injection for hooks
      db/            # Database operations, schema, migrations (SQLite)
      embeddings/    # Embedding queue and OpenAI embedding client
      entities/      # Lightweight entity extraction for recall boost
      fuzzy/         # Fuzzy search fallback
      git/           # Git operations (branch via git2, commits/diffs via CLI)
      hooks/         # Claude Code lifecycle hooks (session, prompt, tool)
      indexer/       # Code parsing and symbol extraction (tree-sitter)
      llm/           # LLM provider clients (DeepSeek, Zhipu, Ollama, MCP Sampling)
      mcp/           # MCP protocol server (rmcp-based)
      proactive/     # Proactive intelligence (behavior mining, predictions)
      search/        # Semantic, keyword, and cross-reference search
      tasks/         # Async task management
      tools/         # MCP tool implementations (core/ has per-tool modules)
      utils/         # Shared utilities (JSON helpers)
      error.rs       # MiraError types
      http.rs        # Shared HTTP client factory
      identity.rs    # User identity detection
      project_files.rs # File discovery and filtering (.miraignore, .gitignore)
  mira-types/        # Shared types (ProjectContext, MemoryFact)
docs/                # Documentation
plugin/              # Claude Code plugin (hooks, skills, MCP config)
```

## Making Changes

1. Fork the repository
2. Create a feature branch: `git checkout -b my-feature`
3. Make your changes
4. Run tests: `cargo test`
5. Run clippy: `cargo clippy --all-targets`
6. Format code: `cargo fmt`
7. Commit your changes
8. Push to your fork and submit a pull request

## Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture
```

Tests that require API keys are skipped if keys aren't configured.

## Code Style

- Follow Rust idioms and best practices
- Use `cargo fmt` before committing
- Address clippy warnings
- Add tests for new functionality
- Keep commits focused and atomic

## Commit Messages

We use conventional commits:

- `feat:` New features
- `fix:` Bug fixes
- `docs:` Documentation changes
- `refactor:` Code refactoring
- `test:` Test additions/changes
- `chore:` Maintenance tasks

## Pull Request Guidelines

- Keep PRs focused on a single change
- Update documentation if needed
- Add tests for new functionality
- Ensure CI passes before requesting review

## Reporting Issues

- Check existing issues before creating a new one
- Use the issue templates when applicable
- Include reproduction steps for bugs
- Be specific about expected vs actual behavior

## Questions?

Open an issue or start a [discussion](https://github.com/ConaryLabs/Mira/discussions). We're happy to help!
