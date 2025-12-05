# Mira

**AI-Powered Coding Assistant with Gemini 3 Pro and Hybrid Memory**

Mira is a sophisticated AI coding assistant powered by Gemini 3 Pro with variable thinking levels, backed by a comprehensive memory system (SQLite + Qdrant) and real-time streaming architecture.

## Overview

Mira combines:
- **Gemini 3 Pro** for state-of-the-art reasoning with variable thinking levels (low/high)
- **Hybrid Memory System** with SQLite (structured) + Qdrant (vector) storage
- **Code Intelligence** for semantic understanding, call graph analysis, and pattern detection
- **Git Intelligence** for commit tracking, co-change patterns, and expertise scoring
- **Real-time Streaming** via WebSocket for interactive coding sessions

Built with **Rust** (backend) and **React + TypeScript** (frontend) for performance, type safety, and real-time responsiveness.

## Architecture

```
                        Frontend (React + TypeScript)
                        Real-time WebSocket, Monaco Editor
                                    |
                              WebSocket (3001)
                                    |
                        +-----------v-----------+
                        |    Rust Backend       |
                        |    (Axum + Tokio)     |
                        +-----------+-----------+
                                    |
            +-----------+-----------+-----------+-----------+
            |           |           |           |           |
      +-----v-----+ +---v---+ +-----v-----+ +---v---+ +-----v-----+
      | Operation | | Gemini| | Memory    | | Git   | | Code      |
      | Engine    | | 3 Pro | | Service   | | Intel | | Intel     |
      +-----------+ +-------+ +-----------+ +-------+ +-----------+
                                    |
                        +-----------+-----------+
                        |                       |
                  +-----v-----+           +-----v-----+
                  | SQLite    |           | Qdrant    |
                  | (50+ tbl) |           | (3 coll)  |
                  +-----------+           +-----------+
```

## Repository Structure

```
mira/
├── backend/          # Rust backend
│   ├── src/          # Source code
│   │   ├── api/      # WebSocket handlers
│   │   ├── operations/ # Operation engine
│   │   ├── memory/   # Memory systems
│   │   ├── llm/      # LLM providers (Gemini 3 Pro)
│   │   └── git/      # Git integration + intelligence
│   ├── tests/        # Integration tests (160+ tests)
│   ├── migrations/   # Database migrations (9 migrations)
│   └── README.md     # Detailed backend docs
│
└── frontend/         # React + TypeScript frontend
    ├── src/
    │   ├── components/ # React components
    │   ├── stores/     # Zustand state stores
    │   ├── services/   # Backend API client
    │   └── hooks/      # Custom React hooks
    └── package.json
```

## Implemented Features

### Core System
- **Gemini 3 Pro Integration** - Single-model architecture with variable thinking levels (low/high)
- **Budget Tracking** - Daily/monthly spending limits with per-request cost tracking
- **LLM Response Cache** - SHA-256 keyed cache targeting 80%+ hit rate
- **Real-time Streaming** - WebSocket-based bidirectional communication with cancellation support
- **Operation Tracking** - Complex multi-step workflows with lifecycle management

### Memory System
- **Hybrid Storage** - SQLite (50+ tables) + Qdrant (3 collections)
- **Multi-head Embeddings** - Separate vectors for code, conversation, and git content
- **Recall Engine** - Combines recent messages + semantic search + rolling summaries
- **Message Analysis** - Automatic extraction of mood, intent, topics, salience

### Code Intelligence (Milestone 2 Complete)
- **Semantic Graph** - Purpose, concepts, and domain labels for every code symbol
- **Call Graph** - Explicit caller-callee relationships with impact analysis
- **Design Pattern Detection** - Factory, Repository, Builder, Observer, etc.
- **Domain Clustering** - Group related code by semantic similarity
- **Analysis Caching** - SHA-256 keyed cache for LLM analysis results

### Git Intelligence (Milestone 3 Complete)
- **Commit Tracking** - Full commit history with file changes and metadata
- **Co-change Patterns** - Detect files frequently modified together (Jaccard confidence)
- **Blame Annotations** - Line-level blame with cache invalidation
- **Author Expertise** - Score developer expertise by file/domain (40% commits + 30% lines + 30% recency)
- **Historical Fixes** - Link error patterns to past fix commits

### Tools & Integration
- **39 Built-in Tools** - File operations, git commands, code analysis, web search, command execution
- **Git Analysis** - History, blame, diff, branches, contributors, status, commit inspection
- **Code Analysis** - Find functions/classes, semantic search, complexity analysis, quality issues, test discovery
- **External Tools** - Web search (DuckDuckGo), URL fetch with markdown conversion, sandboxed shell execution

### Frontend
- **Real-time Chat** - Streaming responses with cancellation
- **Activity Panel** - Live view of reasoning, tasks, and tool executions
- **Monaco Editor** - Embedded code editor for artifacts
- **File Browser** - Navigate and select files from projects
- **Project Management** - Create, import, and manage code projects

### Additional Features (All Milestones Complete)
- **Tool Synthesis** - Auto-generate custom tools from codebase patterns
- **Build System Integration** - Build/test tracking, error parsing, historical fix lookup
- **Reasoning Patterns** - Store and replay successful coding patterns
- **Context Oracle** - Unified context gathering from all 8 intelligence systems
- **Real-time File Watching** - Automatic code intelligence updates on file changes
- **Claude Code Feature Parity** - Slash commands, hooks, checkpoint/rewind, MCP support
- **System Context Detection** - Platform-aware commands (detects OS, package manager, shell, tools)
- **CLI Interface** - Full command-line interface with sudo approval prompts
- **Time Awareness** - LLM knows current date/time without user mentioning it

### Production Features
- **Health Endpoints** - `/health`, `/ready`, `/live` for load balancers and Kubernetes
- **Prometheus Metrics** - `/metrics` endpoint with request, LLM, and budget metrics
- **Rate Limiting** - Configurable request throttling per minute
- **Graceful Shutdown** - SIGTERM handling with connection draining

## Prerequisites

### Backend
- **Rust 1.91** (target version)
- **SQLite 3.35+**
- **Qdrant** running on `localhost:6334` (gRPC)
- **API Keys**: Google (Gemini 3 Pro + embeddings)

### Frontend
- **Node.js 18+**
- **npm** or **yarn**

## Quick Start

### 1. Start Qdrant (Vector Database)

```bash
# Using the bundled binary
./backend/bin/qdrant --config-path ./backend/config/qdrant.yaml

# Or using Docker
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest
```

### 2. Setup Backend

```bash
cd backend

# Install dependencies
cargo build

# Configure environment
cp .env.example .env
# Edit .env with your API keys:
#   - GOOGLE_API_KEY (for Gemini 3 Pro and embeddings)

# Run database migrations
sqlx migrate run

# Start the backend server (WebSocket on port 3001)
cargo run
```

### 3. Setup Frontend

```bash
cd frontend

# Install dependencies
npm install

# Start development server (proxies to backend on port 3001)
npm run dev
```

The frontend will be available at `http://localhost:5173`.

## Development Commands

### Backend (Rust)

```bash
cd backend

# Development
cargo run                    # Start server
RUST_LOG=debug cargo run     # With debug logging
cargo watch -x run           # Hot reload (requires cargo-watch)

# Testing
cargo test                   # Run all tests (160+ tests)
cargo test --test git_intelligence_test  # Specific test
cargo test -- --nocapture    # With output

# Code quality
cargo clippy                 # Linting
cargo fmt                    # Formatting

# Database
sqlx migrate run             # Apply migrations
sqlx migrate revert          # Revert last migration
```

### Frontend (React + TypeScript)

```bash
cd frontend

# Development
npm run dev                  # Start dev server
npm run build                # Production build
npm run preview              # Preview production build

# Testing
npm run test                 # Run tests
npm run test:watch           # Watch mode
npm run test:coverage        # With coverage

# Type checking
npm run type-check           # TypeScript type check
```

## Configuration

### Backend Environment Variables

Key variables in `backend/.env`:

```bash
# Server
MIRA_PORT=3001
MIRA_ENV=development

# Database
DATABASE_URL=sqlite://data/mira.db
QDRANT_URL=http://localhost:6334

# Google Gemini (LLM + Embeddings)
GOOGLE_API_KEY=...
GEMINI_MODEL=gemini-3-pro-preview
GEMINI_THINKING_LEVEL=high
GEMINI_EMBEDDING_MODEL=gemini-embedding-001

# Budget Management
BUDGET_DAILY_LIMIT_USD=5.0
BUDGET_MONTHLY_LIMIT_USD=150.0

# Cache Configuration
CACHE_ENABLED=true
CACHE_TTL_SECONDS=86400
```

## Documentation

- **[USERGUIDE.md](./USERGUIDE.md)** - Comprehensive user and developer guide
- **[CLAUDE.md](./CLAUDE.md)** - Guide for AI assistants working with this codebase
- **[backend/WHITEPAPER.md](./backend/WHITEPAPER.md)** - Technical architecture reference
- **[ROADMAP.md](./ROADMAP.md)** - Project roadmap and milestone details
- **[PROGRESS.md](./PROGRESS.md)** - Detailed technical progress log
- **[frontend/docs/STATE_BOUNDARIES.md](./frontend/docs/STATE_BOUNDARIES.md)** - Frontend state management

## Technology Stack

### Backend
- **Language**: Rust (edition 2024)
- **Web Framework**: Axum (WebSocket support)
- **Async Runtime**: Tokio
- **Database**: SQLite (with sqlx)
- **Vector DB**: Qdrant (with qdrant-client)
- **LLM**: Google Gemini 3 Pro + gemini-embedding-001

### Frontend
- **Language**: TypeScript
- **Framework**: React 18
- **Build Tool**: Vite
- **State Management**: Zustand
- **Code Editor**: Monaco Editor
- **Styling**: Tailwind CSS
- **Testing**: Vitest + React Testing Library

## Testing

- **Backend**: 160+ tests across 17 test suites
- **Frontend**: 350+ tests with 96% pass rate
- Run all tests:

```bash
# Backend
cd backend && DATABASE_URL="sqlite://data/mira.db" cargo test

# Frontend
cd frontend && npm test
```

## License

Proprietary

## Support

For issues or questions:
1. Review [PROGRESS.md](./PROGRESS.md) for technical decisions and troubleshooting
2. Check [ISSUES_TO_CREATE.md](./ISSUES_TO_CREATE.md) for known technical debt
3. Open an issue with detailed reproduction steps
