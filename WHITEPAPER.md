# Mira Backend - Architecture Reference Document
## Complete Directory and File Documentation

---

## Project Structure

```
src/
├── api/                    # WebSocket API layer
│   ├── ws/                 # WebSocket handlers
│   │   ├── chat/           # Chat connection management
│   │   ├── chat_tools/     # Tool execution framework
│   │   └── [handlers]      # Individual message handlers
│   └── [core files]        # Error types, shared types
├── bin/                    # Binary executables
├── config/                 # Configuration management
├── git/                    # Git repository operations
│   └── client/             # Git client implementation
├── llm/                    # Language model integration
│   ├── chat_service/       # High-level chat orchestration
│   ├── client/             # OpenAI API client
│   ├── responses/          # Response management
│   └── streaming/          # SSE stream processing
├── memory/                 # Memory system
│   ├── context/            # Context building service
│   ├── core/               # Core types and traits
│   ├── features/           # Memory features (decay, scoring, etc.)
│   ├── recall/             # Memory retrieval
│   └── storage/            # Storage backends (SQLite, Qdrant)
├── mira_import/            # Import utilities
├── persona/                # AI personalities
├── project/                # Project management
├── prompt/                 # Prompt construction
├── tools/                  # Tool implementations
└── [root files]            # Main, lib, state, utils
```

**Total:** 27 directories, 111 files

---

## Detailed File Listing

### Root Files (`/src/`)
- `lib.rs` - Library module exports
- `main.rs` - Application entry point, server initialization
- `state.rs` - Global application state management
- `test_tools_access.rs` - Test utilities for tools
- `utils.rs` - Shared utility functions

### API Module (`/src/api/`)

#### Core API Files
- `error.rs` - API error types and handling
- `mod.rs` - Module exports
- `types.rs` - Shared API data structures

#### WebSocket Handlers (`/src/api/ws/`)
- `mod.rs` - WebSocket setup and routing
- `message.rs` - Message type definitions
- `files.rs` - File upload/download operations
- `git.rs` - Git repository operations
- `memory.rs` - Memory CRUD operations
- `project.rs` - Project management operations

#### Chat Subsystem (`/src/api/ws/chat/`)
- `connection.rs` - WebSocket connection lifecycle
- `heartbeat.rs` - Keep-alive ping/pong mechanism
- `message_router.rs` - Message routing to handlers
- `mod.rs` - Chat module exports

#### Chat Tools (`/src/api/ws/chat_tools/`)
- `mod.rs` - Tool framework (currently stub)

### Binary (`/src/bin/`)
- `mira_import.rs` - CLI tool for importing chat history

### Configuration (`/src/config/`)
- `mod.rs` - Environment configuration parsing

### Git Module (`/src/git/`)

#### Core Git Files
- `mod.rs` - Module exports
- `store.rs` - Git repository database operations
- `types.rs` - Git data structures

#### Git Client (`/src/git/client/`)
- `mod.rs` - Main git client
- `branch_manager.rs` - Branch operations
- `diff_parser.rs` - Diff parsing and analysis
- `operations.rs` - Core git operations
- `tree_builder.rs` - File tree construction

### LLM Module (`/src/llm/`)

#### Core LLM Files
- `mod.rs` - Module exports
- `chat.rs` - Low-level chat completion methods
- `classification.rs` - Content classification
- `client_helpers.rs` - Helper utilities
- `embeddings.rs` - Embedding types and utilities
- `emotional_weight.rs` - Sentiment analysis
- `intent.rs` - Intent detection
- `memory_eval.rs` - Memory relevance evaluation
- `moderation.rs` - Content moderation
- `schema.rs` - OpenAI API schemas

#### Chat Service (`/src/llm/chat_service/`)
- `mod.rs` - High-level chat orchestration
- `config.rs` - Chat configuration
- `context.rs` - Context building
- `response.rs` - Response processing

#### Client (`/src/llm/client/`)
- `mod.rs` - OpenAI client implementation
- `config.rs` - Client configuration
- `embedding.rs` - Embedding API with batch support
- `responses.rs` - Response parsing
- `streaming.rs` - Stream processing

#### Response Management (`/src/llm/responses/`)
- `mod.rs` - Response module exports
- `image.rs` - Image generation
- `manager.rs` - Response orchestration
- `thread.rs` - Thread management
- `types.rs` - Response types
- `vector_store.rs` - Vector store operations

#### Streaming (`/src/llm/streaming/`)
- `mod.rs` - Streaming orchestrator
- `processor.rs` - Stream processing logic
- `request.rs` - Request building

### Memory System (`/src/memory/`)

#### Core Memory Files
- `mod.rs` - Module exports
- `service.rs` - Main memory service

#### Context Service (`/src/memory/context/`)
- `mod.rs` - Module exports
- `service.rs` - Context building service

#### Core Types (`/src/memory/core/`)
- `mod.rs` - Core module exports
- `config.rs` - Memory configuration
- `traits.rs` - Memory system traits
- `types.rs` - Core data structures

#### Features (`/src/memory/features/`)
- `mod.rs` - Feature exports
- `classification.rs` - Content classification and routing
- `decay.rs` - Time-based salience decay
- `decay_scheduler.rs` - Background decay task
- `embedding.rs` - Batch embedding management
- `memory_types.rs` - Memory-specific types
- `salience.rs` - Importance scoring
- `scoring.rs` - Composite relevance scoring
- `session.rs` - Session management
- `summarization.rs` - Rolling summary generation
- `summarizer.rs` - Text summarization

#### Recall (`/src/memory/recall/`)
- `mod.rs` - Recall exports
- `parallel_recall.rs` - Concurrent retrieval
- `recall.rs` - Context retrieval orchestration

#### Storage (`/src/memory/storage/`)

##### SQLite Backend (`/src/memory/storage/sqlite/`)
- `mod.rs` - SQLite module exports
- `migration.rs` - Database migrations
- `query.rs` - Query builders
- `store.rs` - SQL operations

##### Qdrant Backend (`/src/memory/storage/qdrant/`)
- `mod.rs` - Qdrant module exports
- `mapping.rs` - Data mapping
- `multi_store.rs` - Multi-collection management
- `search.rs` - Vector similarity search
- `store.rs` - Single collection operations

### Import (`/src/mira_import/`)
- `mod.rs` - Import module exports
- `openai.rs` - ChatGPT export parser
- `schema.rs` - Import data structures
- `writer.rs` - Database writer

### Personas (`/src/persona/`)
- `mod.rs` - Persona selection logic
- `default.rs` - Professional persona
- `forbidden.rs` - Unrestricted persona
- `hallow.rs` - Creative persona
- `haven.rs` - Supportive persona

### Project (`/src/project/`)
- `mod.rs` - Module exports
- `store.rs` - Project CRUD operations
- `types.rs` - Project data structures

### Prompt (`/src/prompt/`)
- `mod.rs` - Module exports
- `builder.rs` - System prompt construction

### Tools (`/src/tools/`)
- `mod.rs` - Module exports
- `definitions.rs` - Tool definitions
- `document.rs` - Document processing
- `executor.rs` - Tool execution framework
- `file_context.rs` - File context management
- `file_search.rs` - File search service
- `message_handler.rs` - Tool message handling
- `prompt_builder.rs` - Tool-aware prompts

---

## Module Purpose Reference

| Module | Files | Purpose |
|--------|-------|---------|
| `api/ws` | 11 | WebSocket communication layer |
| `llm` | 24 | OpenAI integration and AI operations |
| `memory` | 26 | Memory storage, retrieval, and processing |
| `tools` | 8 | Tool execution and file operations |
| `git` | 8 | Git repository management |
| `persona` | 5 | AI personality definitions |
| `project` | 3 | Project and artifact management |
| `config` | 1 | Configuration management |
| `prompt` | 2 | Prompt building |
| `mira_import` | 4 | Data import utilities |

---

## Key Architectural Components

### Storage Architecture
- **SQLite**: Metadata, session data, full-text search
- **Qdrant**: Vector embeddings in 3 collections:
  - `semantic` - General conversation embeddings
  - `code` - Programming-related content
  - `summary` - Compressed conversation summaries

### Service Architecture
Services are Arc-wrapped and shared via AppState:
- Memory service handles all memory operations
- LLM client manages OpenAI interactions
- Git client handles repository operations
- Project store manages projects/artifacts

### Message Flow
1. WebSocket message arrives at `/api/ws/`
2. Router (`message_router.rs`) dispatches to handler
3. Handler validates and calls appropriate service
4. Service performs business logic
5. Response sent back via WebSocket

---

## Configuration Points

All configuration is centralized in `/src/config/mod.rs`:
- API keys (OpenAI, etc.)
- Model configurations (GPT-5, embeddings)
- Feature flags (tools, decay, summaries)
- System parameters (ports, timeouts, limits)
- Database paths and connections

Environment variables are prefixed with `MIRA_` for system config.

---

*This document serves as a complete reference for the Mira Backend file structure and organization.*
