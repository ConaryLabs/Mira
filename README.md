# Mira

**AI-Powered Coding Assistant with Intelligent Multi-LLM Orchestration**

Mira is a sophisticated AI coding assistant that combines the reasoning capabilities of GPT-5 with the code generation prowess of DeepSeek, backed by a comprehensive memory system and real-time streaming architecture.

## Overview

Unlike traditional AI assistants that use a single LLM for all tasks, Mira orchestrates multiple specialized models:
- **GPT-5** handles conversation, analysis, planning, and high-level reasoning
- **DeepSeek** focuses on code generation and implementation
- Each model works on what it does best, delegated intelligently by the system

Built with **Rust** (backend) and **React + TypeScript** (frontend) for performance, type safety, and real-time responsiveness.

## Architecture

```
┌─────────────────────────────────────────────┐
│   React + TypeScript Frontend (Vite)       │
│   • Real-time WebSocket communication      │
│   • Monaco code editor                     │
│   • Zustand state management               │
└──────────────────┬──────────────────────────┘
                   │ WebSocket (port 3001)
┌──────────────────▼──────────────────────────┐
│   Rust Backend (Axum + Tokio)              │
│   ┌─────────────────────────────────────┐  │
│   │  Unified Chat Handler               │  │
│   │  • Message routing                  │  │
│   │  • Context gathering                │  │
│   └─────────┬───────────────────────────┘  │
│             │                                │
│   ┌─────────▼──────────┐  ┌──────────────┐ │
│   │   GPT-5            │  │  DeepSeek    │ │
│   │   (Reasoning)      │─►│  (Code Gen)  │ │
│   └────────────────────┘  └──────────────┘ │
│                                              │
│   ┌──────────────────────────────────────┐ │
│   │  Hybrid Memory System                │ │
│   │  • SQLite (structured data)          │ │
│   │  • Qdrant (vector embeddings)        │ │
│   │  • 5-head embeddings                 │ │
│   │  • Recall engine (recent + semantic) │ │
│   └──────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

## Repository Structure

```
mira/
├── backend/          # Rust backend
│   ├── src/          # Source code
│   │   ├── api/      # WebSocket handlers
│   │   ├── operations/ # Operation engine
│   │   ├── memory/   # Memory systems
│   │   ├── llm/      # LLM providers
│   │   └── git/      # Git integration
│   ├── tests/        # Integration tests (127+ tests)
│   ├── migrations/   # Database migrations
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

## Key Features

- **Intelligent LLM Orchestration** - GPT-5 for reasoning, DeepSeek for code generation
- **Hybrid Memory System** - SQLite + Qdrant with multi-head embeddings (semantic, code, summary, documents, relationships)
- **Real-time Streaming** - WebSocket-based bidirectional communication with cancellation support
- **Context-Aware** - Gathers recent messages, semantic search results, file trees, and code intelligence
- **Git Integration** - Clone, import, sync repositories; file tree navigation; diff parsing
- **Code Intelligence** - Function/class extraction, semantic code search, project structure analysis
- **Operation Tracking** - Complex multi-step workflows with lifecycle management (PENDING → STARTED → DELEGATING → COMPLETED)
- **Artifact Management** - Code blocks from LLM can be saved/applied to files via Monaco editor
- **Integrated Terminal** - Full xterm.js terminal emulator with real-time PTY-based shell execution, multiple sessions, project-scoped working directories

## Recent Improvements (November 2025)

### Session 2: Integrated Terminal
- **Full terminal emulator** - xterm.js with themed styling and real-time I/O
- **PTY-based execution** - portable-pty for native shell support
- **WebSocket streaming** - bidirectional communication with base64 encoding
- **Multiple sessions** - tab-based switching, project-scoped directories
- **Right-side panel** - drag-to-resize, traditional IDE layout
- **Session persistence** - SQLite storage, proper cleanup on close
- **React fixes** - resolved Hooks violation, stale closure issues

### Session 1: Comprehensive Codebase Housecleaning (25 tasks)

**Code Quality:**
- **Eliminated 700+ lines** of duplicated code
- **Created 14 new focused modules** for better organization
- **Refactored config system** from 445-line monolith to 7 domain-specific configs
- **Split prompt builder** from 612 lines into 5 focused modules
- **Consolidated frontend message handlers** with shared artifact utilities

**Testing & Documentation:**
- **Added 62 new tests** (45 frontend + 17 backend) - all passing
- **Created STATE_BOUNDARIES.md** - frontend architecture documentation
- **Created test helpers** for cleaner test configuration
- **Catalogued 20 technical debt items** in ISSUES_TO_CREATE.md

**Architecture:**
- **Improved message router** - extracted common patterns, reduced duplication
- **Better error handling** - proper logging instead of silent failures
- **Simplified state management** - removed unused state, clearer boundaries
- **Tool builder pattern** - DRY code for LLM tool definitions

See **[HOUSECLEANING_SUMMARY.md](./HOUSECLEANING_SUMMARY.md)** for Session 1 details and **[PROGRESS.md](./PROGRESS.md)** for complete session history.

## Prerequisites

### Backend
- **Rust 1.75+** (install via [rustup](https://rustup.rs/))
- **SQLite 3.35+**
- **Qdrant** vector database running on `localhost:6333`
- **API Keys**: OpenAI (GPT-5 + embeddings), DeepSeek

### Frontend
- **Node.js 18+**
- **npm** or **yarn**

## Quick Start

### 1. Start Qdrant (Vector Database)

```bash
# Using Docker
docker run -p 6333:6333 qdrant/qdrant:latest
```

### 2. Setup Backend

```bash
cd backend

# Install dependencies
cargo build

# Configure environment
cp .env.example .env
# Edit .env with your API keys:
#   - OPENAI_API_KEY
#   - DEEPSEEK_API_KEY
#   - QDRANT_URL (default: http://localhost:6333)

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

The frontend will be available at `http://localhost:5173` and will proxy WebSocket connections to the backend on port 3001.

## Development Commands

### Backend (Rust)

```bash
cd backend

# Development
cargo run                    # Start server
RUST_LOG=debug cargo run     # With debug logging
cargo watch -x run           # Hot reload (requires cargo-watch)

# Testing
cargo test                   # Run all tests
cargo test --test operation_engine_test  # Specific test
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
DATABASE_URL=sqlite://mira.db
QDRANT_URL=http://localhost:6333

# LLM Providers
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-5-0314
DEEPSEEK_API_KEY=...
DEEPSEEK_MODEL=deepseek-reasoner

# Embeddings
OPENAI_EMBEDDING_MODEL=text-embedding-3-large
SALIENCE_MIN_FOR_EMBED=0.6

# Memory
MAX_RECALLED_MESSAGES=10
USE_ROLLING_SUMMARIES=true
```

See `backend/.env.example` for complete configuration options.

## Documentation

### Architecture & Development
- **[backend/README.md](./backend/README.md)** - Comprehensive backend documentation
- **[backend/WHITEPAPER.md](./backend/WHITEPAPER.md)** - Detailed architectural reference
- **[CLAUDE.md](./CLAUDE.md)** - Guide for AI assistants working with this codebase
- **[frontend/docs/STATE_BOUNDARIES.md](./frontend/docs/STATE_BOUNDARIES.md)** - Frontend state management architecture

### Project Management
- **[HOUSECLEANING_SUMMARY.md](./HOUSECLEANING_SUMMARY.md)** - Recent codebase refactoring (Nov 2025)
- **[ISSUES_TO_CREATE.md](./ISSUES_TO_CREATE.md)** - Catalogued technical debt and improvements

## Technology Stack

### Backend
- **Language**: Rust (edition 2024)
- **Web Framework**: Axum (WebSocket support)
- **Async Runtime**: Tokio
- **Database**: SQLite (with sqlx)
- **Vector DB**: Qdrant (with qdrant-client)
- **LLM APIs**: OpenAI (GPT-5), DeepSeek
- **Git**: git2 (libgit2 bindings)

### Frontend
- **Language**: TypeScript
- **Framework**: React 18
- **Build Tool**: Vite
- **State Management**: Zustand
- **Code Editor**: Monaco Editor
- **Styling**: Tailwind CSS
- **Testing**: Vitest + React Testing Library
- **WebSocket**: Native WebSocket API

## Architecture Highlights

### Operation Engine
Complex workflows are tracked through state transitions:
```
PENDING → STARTED → DELEGATING → GENERATING → COMPLETED
                                               ↓
                                          FAILED
```

### Memory System
Three-tier memory architecture:
1. **Recent Memory** - Last N messages (chronological)
2. **Semantic Memory** - Vector search across 5 embedding heads
3. **Rolling Summaries** - Compressed context (10-msg and 100-msg windows)

### Embedding Heads
- `semantic` - General conversation
- `code` - Programming content
- `summary` - Conversation summaries
- `documents` - Project documentation
- `relationship` - User preferences and patterns

## Testing

- **Backend**: 18 test suites, 144+ tests (including 17 new tool builder tests)
- **Frontend**: 45+ tests with Vitest and React Testing Library (artifact utilities, component tests)
- **Test Coverage**: Critical paths for artifact creation, tool building, message routing
- **Test Helpers**: Shared utilities for API key configuration in `backend/tests/common/`

Run all tests:
```bash
# Backend (all tests)
cd backend && cargo test

# Backend (specific test suite)
cargo test --test tool_builder_test

# Frontend (all tests)
cd frontend && npm test

# Frontend (specific test)
npm test -- src/utils/__tests__/artifact.test.ts

# Frontend (watch mode)
npm run test:watch
```

## Troubleshooting

**Backend won't start:**
- Ensure Qdrant is running on `localhost:6333`
- Check SQLite database permissions
- Verify API keys in `.env`

**Frontend can't connect:**
- Ensure backend is running on port 3001
- Check browser console for WebSocket errors
- Verify proxy configuration in `vite.config.js`

**Memory/embedding issues:**
- Check `SALIENCE_MIN_FOR_EMBED` threshold
- Verify Qdrant collections are created
- Run `backend/scripts/reset_embeddings.sh` if needed

## License

Proprietary

## Support

For issues or questions:
1. Check the troubleshooting sections in backend/README.md
2. Review architecture documentation in backend/WHITEPAPER.md
3. Open an issue with detailed reproduction steps
