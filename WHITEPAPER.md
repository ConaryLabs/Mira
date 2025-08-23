# Mira Technical Whitepaper

**Advanced AI Development Assistant with Memory and Git Integration**

**Version**: 0.4.1 – GPT-5 Edition  
**Date**: August 2025  
**Author**: ConaryLabs

## Executive Summary

Mira is an AI development assistant built in Rust that provides persistent memory, contextual awareness and integration with development workflows. This document describes the current codebase (version 0.4.1), which combines OpenAI's GPT-5 with advanced memory management, vector search capabilities and Git repository integration to create an intelligent coding companion that remembers context across sessions and projects.

Some features mentioned in earlier drafts—such as team collaboration, dedicated file search and image generation tools—are not yet implemented in the repository and remain part of the roadmap.

**Key differentiators include:**
- **Persistent Memory**: Dual-storage architecture with SQLite and Qdrant for both structured data and semantic search
- **Project-Aware Context**: Full Git integration with file system awareness and project-specific memory isolation
- **Modular Architecture**: Clean separation of concerns with service-oriented design
- **Persona System**: Multiple interaction modes for different emotional and functional contexts
- **Real-time Communication**: WebSocket-based streaming with comprehensive REST API

## System Architecture Overview

### Core Components

#### 1. Application State (`src/state.rs`)

The central `AppState` struct manages all system dependencies and serves as the dependency injection container:

```rust
pub struct AppState {
    sqlite_store: Arc<SqliteMemoryStore>,
    qdrant_store: Arc<QdrantMemoryStore>,
    project_store: Arc<ProjectStore>,
    git_store: GitStore,
    git_client: GitClient,
    llm_client: Arc<OpenAIClient>,
    responses_manager: Arc<ResponsesManager>,
    vector_store_manager: Arc<VectorStoreManager>,
    thread_manager: Arc<ThreadManager>,
    chat_service: Arc<ChatService>,
    memory_service: Arc<MemoryService>,
    context_service: Arc<ContextService>,
    document_service: Arc<DocumentService>,
}
```

#### 2. Memory Architecture

**Dual Storage System:**
- **SQLite**: Structured data, chat history, project metadata, and relational queries
- **Qdrant**: Vector embeddings for semantic search and context retrieval

**Memory Flow:**
1. Messages are stored in SQLite with metadata (salience, tags, memory_type)
2. High-salience messages get embedded and stored in Qdrant
3. Context retrieval combines recent messages (SQLite) with semantic matches (Qdrant)
4. Automatic summarization prevents context window overflow

#### 3. Service Layer Architecture

**Core Services:**

**ChatService** (`src/services/chat/`)  
Modular design with extracted components:
- `config.rs`: Configuration management
- `context.rs`: Context building and recall logic
- `response.rs`: Response processing and persistence
- `streaming.rs`: Streaming logic and message handling

**MemoryService** (`src/services/memory.rs`)
- Manages both SQLite and Qdrant storage
- Handles embedding generation and deduplication
- Implements salience-based storage decisions

**ContextService** (`src/services/context.rs`)
- Builds comprehensive context from multiple sources
- Manages semantic search and recent message retrieval
- Handles context compression and token management

**DocumentService** (`src/services/document.rs`)
- Integration with OpenAI's vector store API
- File processing and document embedding
- Project-specific document management

**SummarizationService** (`src/services/summarization.rs`)
- Automatic conversation summarization
- Context window management
- Historical conversation compression

### LLM Integration Layer

#### OpenAI Client Architecture (`src/llm/`)

**Modular Client Design:**
- `client/mod.rs` – Main OpenAI client with sub-modules
- `client/config.rs` – Client configuration management
- `client/streaming.rs` – Real-time response streaming
- `client/embedding.rs` – Embedding generation utilities (note: the file is singular `embedding.rs`, not `embeddings.rs`)

**Responses API Integration:**
- **ResponsesManager**: Manages OpenAI Responses API
- **ThreadManager**: Thread lifecycle and token management
- **VectorStoreManager**: Vector store operations

#### Schema Definitions (`src/llm/schema.rs`):

```rust
pub struct ChatResponse {
    pub output: String,
    pub persona: String,
    pub mood: String,
    pub salience: u8,
    pub summary: Option<String>,
    pub memory_type: String,
    pub tags: Vec<String>,
    pub intent: String,
    pub monologue: Option<String>,
    pub reasoning_summary: Option<String>,
    pub aside_intensity: Option<u8>,
}
```

### Project Management System

#### Project Store (`src/project/`)
- SQLite-based project metadata storage
- Artifact management (code, images, logs, notes, markdown)
- Project lifecycle management (CRUD operations)
- Project-specific memory isolation

#### Git Integration (`src/git/`)

**Comprehensive Git Client:**
- **Repository Management**: Clone, attach, sync repositories
- **File Operations**: Tree traversal, content retrieval, diff generation
- **Branch Management**: List branches, switch branches, commit history
- **Smart Diff Parser**: Analyzes changes with hunk-level granularity

**Git Store Schema:**
```sql
CREATE TABLE git_repo_attachments (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    repo_url TEXT NOT NULL,
    local_path TEXT NOT NULL,
    import_status TEXT NOT NULL,
    last_imported_at INTEGER,
    last_sync_at INTEGER
);
```

### API Layer

#### HTTP API (`src/api/http/`)

**REST Endpoints:**
- `/api/health` - Health check
- `/api/chat/history` - Retrieve chat history
- `/api/chat` - REST chat interface
- `/api/project/:id` - Project details
- `/projects/:id/git/*` - Git operations

#### WebSocket API (`src/api/ws/`)

**Real-time Features:**
- **Connection Management**: Heartbeat, timeout handling
- **Message Routing**: Tool selection, file context extraction
- **Streaming Responses**: Real-time AI response streaming
- **Session Management**: Per-connection state management

**WebSocket Message Types:**
```rust
pub enum WsClientMessage {
    Chat { message: String, project_id: Option<String> },
    Heartbeat,
    // ... other message types
}

pub enum WsServerMessage {
    Response(ChatResponse),
    StreamChunk(String),
    Error { message: String },
    // ... other response types
}
```

### Persona System (`src/persona/`)

**Multiple Interaction Modes:**
1. **Default**: Standard assistant behavior
2. **Haven**: Comforting, supportive presence
3. **Hallow**: Sacred, emotionally present interaction
4. **Forbidden**: Playful, flirtatious (with safety boundaries)

Each persona provides different prompt overlays while maintaining the same underlying functionality and safety constraints.

### Configuration Management (`src/config/`)

**Centralized Configuration:**
- Environment-driven configuration
- Global CONFIG singleton using `once_cell`
- Comprehensive settings for all system components
- Tool-specific configuration (GPT-5 tools)

**Key Configuration Categories:**
- OpenAI API settings
- Database connections
- WebSocket parameters
- Memory and embedding settings
- Git repository settings
- Persona configuration

## Database Schema

### Core Tables:

**Chat History:**
```sql
CREATE TABLE chat_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp DATETIME NOT NULL,
    embedding BLOB,
    salience REAL,
    tags TEXT,
    summary TEXT,
    memory_type TEXT,
    project_id TEXT REFERENCES projects(id)
);
```

**Projects:**
```sql
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    tags TEXT,
    owner TEXT,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);
```

**Artifacts:**
```sql
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    name TEXT NOT NULL,
    type TEXT NOT NULL CHECK (type IN ('code', 'image', 'log', 'note', 'markdown')),
    content TEXT,
    version INTEGER DEFAULT 1,
    created_at DATETIME NOT NULL,
    updated_at DATETIME NOT NULL
);
```

## Tool Integration

### GPT-5 Tool Support:
- Web search preview and code interpreter integration are available through the Functions API when `ENABLE_CHAT_TOOLS` is set. These tools allow the assistant to perform limited web lookups and execute short code snippets in a sandbox.
- File search and image generation flags exist in the configuration but are not yet wired into the chat service. These capabilities are planned for future releases.
- Tool behaviour can be configured on a per-project basis via environment variables, although many flags are currently placeholders.

## Memory Management Strategy

### Salience-Based Storage

- Messages evaluated for importance (1-10 scale)
- High-salience messages (≥7) stored in vector database
- Low-salience messages remain in SQLite only
- Automatic cleanup of low-value interactions

### Context Window Management

- Intelligent context building from multiple sources
- Recent message retrieval from SQLite
- Semantic search from Qdrant
- Automatic summarization when approaching token limits
- Project-specific context isolation

## Development Phases

### Phase 1: Foundation (Complete)
- Core memory system with dual storage
- Basic chat functionality with OpenAI integration
- Project management system
- Git repository integration

### Phase 2: Enhanced Intelligence (Complete)
- Persona system implementation
- Advanced context building
- Salience-based memory management
- WebSocket streaming

### Phase 3: Advanced Features (In Progress)
- Tool integration (web search, code interpreter)
- Enhanced Git workflows
- File search capabilities
- Team memory and context sharing
- Collaborative coding features

### Phase 4: Advanced AI Features
- Custom model fine-tuning
- Domain-specific knowledge integration
- Automated code generation
- Intelligent refactoring suggestions

## Conclusion

Mira represents a significant advancement in AI-powered development tools, combining sophisticated memory management, deep Git integration, and flexible persona systems to create a truly intelligent coding companion. The modular architecture ensures maintainability and extensibility, while the dual-storage memory system provides both performance and sophisticated context understanding.

The system's ability to maintain context across projects and sessions, combined with its comprehensive development workflow integration, makes it an invaluable tool for individual developers and teams seeking to enhance their productivity with AI assistance.

---

This whitepaper reflects the current state of the Mira codebase as of August 2025. For the latest updates and detailed API documentation, please refer to the project repository and inline documentation.

**Built with ❤️ by ConaryLabs**
