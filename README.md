# Mira

**AI-Powered Coding Assistant with Hybrid Memory**

Mira is an AI coding assistant powered by OpenAI GPT-5.1 with multi-tier model routing, backed by a comprehensive memory system and real-time streaming architecture. It remembers your codebase, understands your patterns, and helps you code more effectively.

## Features

### Intelligent Code Understanding
- **Semantic Code Graph** - Understands purpose, concepts, and relationships in your code
- **Call Graph Analysis** - Tracks function dependencies and impact of changes
- **Design Pattern Detection** - Recognizes Factory, Repository, Builder, Observer patterns
- **Git Intelligence** - Learns from commit history, co-change patterns, and author expertise

### Conversational Memory
- **Hybrid Storage** - SQLite for structure + Qdrant for semantic search
- **Multi-head Embeddings** - Separate vectors for code, conversations, and git context
- **Rolling Summaries** - Maintains context across long sessions
- **Automatic Analysis** - Extracts intent, topics, and salience from messages

### Developer Tools
- **39 Built-in Tools** - File operations, git commands, code analysis, web search
- **Sandboxed Execution** - Safe command execution with sudo approval prompts
- **Real-time Streaming** - WebSocket-based responses with cancellation support
- **Project Context** - Attaches git repository context to conversations

### Multiple Interfaces
- **Web UI** - React frontend with Monaco editor and real-time streaming
- **CLI** - Full-featured command-line interface with feature parity to web UI
  - Interactive REPL with session management
  - Streaming responses and sudo approval prompts
  - Custom slash commands (`.mira/commands/`)
  - Works in any terminal environment

### Production Ready
- **Budget Tracking** - Daily/monthly spending limits with cost visibility
- **Response Caching** - 80%+ cache hit rate for repeated queries
- **Health Endpoints** - `/health`, `/ready`, `/live` for load balancers
- **Prometheus Metrics** - Request, LLM, and budget metrics at `/metrics`

## Quick Start

### Option 1: Docker Compose (Recommended)

The easiest way to run Mira. Requires only Docker - no Rust or Node.js needed.

```bash
# Clone the repository
git clone https://github.com/ConaryLabs/Mira.git
cd Mira

# Configure your API key
cp .env.example .env
# Edit .env and add your OPENAI_API_KEY

# Start all services
docker compose up -d

# Access Mira at http://localhost:8080
```

**Get an OpenAI API key**: https://platform.openai.com/api-keys

### Option 2: Development Setup

For contributors or those who prefer native installation.

```bash
# 1. Start Qdrant (vector database)
docker run -d -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest

# 2. Setup backend
cd backend
cp .env.example .env
# Edit .env with your OPENAI_API_KEY
cargo build --release
./target/release/mira-backend

# 3. Setup frontend (new terminal)
cd frontend
npm install
npm run dev

# Access at http://localhost:5173
```

### Using the CLI

After building the backend, you can also use the CLI:

```bash
cd backend
./target/release/mira

# Or with cargo
cargo run --bin mira
```

The CLI provides the same features as the web UI in your terminal. See [CLI.md](./CLI.md) for the full command reference.

## Architecture

```
+------------------+     +------------------+     +------------------+
|     Frontend     |     |     Backend      |     |     Qdrant       |
|   React + Vite   | <-> |   Rust + Axum    | <-> |  Vector Search   |
|   Monaco Editor  |     |  OpenAI GPT-5.1  |     |   3 Collections  |
+------------------+     +--------+---------+     +------------------+
                                  |
                         +--------v---------+
                         |      SQLite      |
                         |    70+ Tables    |
                         +------------------+
```

## Requirements

### Docker Deployment
- Docker 24+ with Docker Compose v2
- OpenAI API key (GPT-5.1)

### Development
- Rust 1.91+
- Node.js 18+
- SQLite 3.35+
- Qdrant 1.12+

## Configuration

Key environment variables (set in `.env`):

| Variable | Required | Description |
|----------|----------|-------------|
| `OPENAI_API_KEY` | Yes | OpenAI API key for GPT-5.1 |
| `MODEL_ROUTER_ENABLED` | No | Enable 4-tier model routing (default: true) |
| `BUDGET_DAILY_LIMIT_USD` | No | Daily spending limit (default: 5.0) |
| `BUDGET_MONTHLY_LIMIT_USD` | No | Monthly spending limit (default: 150.0) |

See `backend/.env.example` for all options.

## Documentation

| Document | Description |
|----------|-------------|
| [CLI.md](./CLI.md) | CLI cheat sheet and reference |
| [DEPLOYMENT.md](./DEPLOYMENT.md) | Deployment guide (Docker, systemd, nginx) |
| [USERGUIDE.md](./USERGUIDE.md) | User and developer guide |
| [CLAUDE.md](./CLAUDE.md) | Guide for AI assistants |
| [backend/WHITEPAPER.md](./backend/WHITEPAPER.md) | Backend technical architecture |
| [frontend/WHITEPAPER.md](./frontend/WHITEPAPER.md) | Frontend technical architecture |

## Technology Stack

**Backend**: Rust, Axum, Tokio, SQLite, Qdrant, OpenAI GPT-5.1

**Frontend**: React 18, TypeScript, Vite, Zustand, Monaco Editor, Tailwind CSS

## License

MIT License - see [LICENSE](./LICENSE) for details.

## Contributing

1. Fork the repository
2. Use the [development setup](#option-2-development-setup)
3. See [CLAUDE.md](./CLAUDE.md) for coding conventions
4. Submit a pull request

## Support

- Review [DEPLOYMENT.md](./DEPLOYMENT.md) for setup issues
- Check [PROGRESS.md](./PROGRESS.md) for technical decisions
- Open an issue with reproduction steps
