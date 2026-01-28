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
   cp .env.example ~/.mira/.env
   # Edit ~/.mira/.env with your API keys
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

## Code Style

- Follow Rust idioms and best practices
- Use `cargo fmt` before committing
- Address clippy warnings
- Add tests for new functionality
- Keep commits focused and atomic

## Reporting Issues

- Check existing issues before creating a new one
- Use the issue templates when applicable
- Include reproduction steps for bugs
- Be specific about expected vs actual behavior

## Questions?

Open an issue or start a discussion. We're happy to help!
