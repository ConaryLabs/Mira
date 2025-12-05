# Mira

**AI-Powered Coding Assistant with Hybrid Memory**

Mira is an AI coding assistant powered by Google Gemini with variable thinking levels, backed by a comprehensive memory system and real-time streaming architecture. It remembers your codebase, understands your patterns, and helps you code more effectively.

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
# Edit .env and add your GOOGLE_API_KEY

# Start all services
docker compose up -d

# Access Mira at http://localhost:8080
```

**Get a Google API key**: https://makersuite.google.com/app/apikey

### Option 2: Development Setup

For contributors or those who prefer native installation.

```bash
# 1. Start Qdrant (vector database)
docker run -d -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest

# 2. Setup backend
cd backend
cp .env.example .env
# Edit .env with your GOOGLE_API_KEY
cargo build --release
./target/release/mira-backend

# 3. Setup frontend (new terminal)
cd frontend
npm install
npm run dev

# Access at http://localhost:5173
```

## Architecture

```
+------------------+     +------------------+     +------------------+
|     Frontend     |     |     Backend      |     |     Qdrant       |
|   React + Vite   | <-> |   Rust + Axum    | <-> |  Vector Search   |
|   Monaco Editor  |     |   Gemini 3 Pro   |     |   3 Collections  |
+------------------+     +--------+---------+     +------------------+
                                  |
                         +--------v---------+
                         |      SQLite      |
                         |    50+ Tables    |
                         +------------------+
```

## Requirements

### Docker Deployment
- Docker 24+ with Docker Compose v2
- Google API key (Gemini)

### Development
- Rust 1.91+
- Node.js 18+
- SQLite 3.35+
- Qdrant 1.12+

## Configuration

Key environment variables (set in `.env`):

| Variable | Required | Description |
|----------|----------|-------------|
| `GOOGLE_API_KEY` | Yes | Google API key for Gemini |
| `GEMINI_THINKING_LEVEL` | No | `low` or `high` (default: high) |
| `BUDGET_DAILY_LIMIT_USD` | No | Daily spending limit (default: 5.0) |
| `BUDGET_MONTHLY_LIMIT_USD` | No | Monthly spending limit (default: 150.0) |

See `backend/.env.example` for all options.

## Documentation

| Document | Description |
|----------|-------------|
| [DEPLOYMENT.md](./DEPLOYMENT.md) | Deployment guide (Docker, systemd, nginx) |
| [USERGUIDE.md](./USERGUIDE.md) | User and developer guide |
| [CLAUDE.md](./CLAUDE.md) | Guide for AI assistants |
| [backend/WHITEPAPER.md](./backend/WHITEPAPER.md) | Technical architecture |

## Technology Stack

**Backend**: Rust, Axum, Tokio, SQLite, Qdrant, Google Gemini

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
