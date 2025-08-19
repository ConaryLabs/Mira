# Mira Backend

High-performance Rust backend for the Mira AI Assistant, featuring GPT-5 integration, WebSocket real-time communication, vector memory storage, and Git repository management.

## Features

- 🤖 **GPT-5 Integration** - Advanced AI with tool support (web search, code interpreter, file search, image generation)
- 🔄 **WebSocket Chat** - Real-time streaming responses with mood and metadata
- 🧠 **Dual Memory System** - SQLite for conversation history + Qdrant for semantic search
- 📁 **Project Management** - Full CRUD operations with artifact support
- 🔀 **Git Integration** - Clone, sync, and manage Git repositories
- 🎭 **Persona System** - Multiple AI personalities with dynamic switching
- 📊 **Context Building** - Intelligent retrieval with summarization
- 🔧 **Tool Extensions** - Extensible tool system via trait-based design

## Tech Stack

- **Language**: Rust (async/await)
- **Web Framework**: Axum
- **Database**: SQLite (via SQLx) + Qdrant (vector store)
- **AI**: OpenAI GPT-5 API
- **WebSocket**: Tokio + Axum-ws
- **Git**: git2-rs
- **Async Runtime**: Tokio

## Prerequisites

- Rust 1.75+
- SQLite 3
- Qdrant running on port 6333
- OpenAI API key with GPT-5 access
- Git

## Installation

```bash
# Clone the repository
git clone https://github.com/ConaryLabs/Mira.git
cd Mira/backend

# Set up environment variables
cp .env.example .env
# Edit .env with your configuration

# Install dependencies and build
cargo build --release

# Run database migrations
sqlx migrate run

# Start the server
cargo run --release
```

## Configuration

### Environment Variables (.env)

```bash
# OpenAI Configuration
OPENAI_API_KEY=your_key_here
MIRA_MODEL=gpt-5
MIRA_VERBOSITY=high
MIRA_REASONING_EFFORT=high
MIRA_MAX_OUTPUT_TOKENS=128000

# Database
DATABASE_URL=sqlite:./mira.db
SQLITE_MAX_CONNECTIONS=10

# Qdrant Vector Store
QDRANT_URL=http://localhost:6333
QDRANT_COLLECTION=mira-memory
QDRANT_EMBEDDING_DIM=3072

# Session
MIRA_SESSION_ID=peter-eternal
MIRA_DEFAULT_PERSONA=default

# GPT-5 Tools (Phase 3)
ENABLE_WEB_SEARCH=true
ENABLE_CODE_INTERPRETER=true
ENABLE_FILE_SEARCH=true
ENABLE_IMAGE_GENERATION=true

# Server
MIRA_HOST=0.0.0.0
MIRA_PORT=8080

# Git
GIT_REPOS_DIR=./repos
```

## Architecture

```
src/
├── api/
│   ├── ws/              # WebSocket handlers
│   │   ├── chat.rs      # Main chat handler
│   │   ├── chat_tools.rs # GPT-5 tool integration
│   │   └── message.rs   # Message types
│   └── http/            # REST endpoints
│       ├── git/         # Git operations
│       └── project.rs   # Project management
├── services/
│   ├── chat.rs          # Chat service
│   ├── chat_with_tools.rs # Tool extension trait
│   ├── memory.rs        # Memory management
│   └── summarization.rs # Context summarization
├── llm/
│   ├── client.rs        # OpenAI client
│   ├── responses/       # GPT-5 Responses API
│   └── embeddings.rs    # Vector embeddings
├── memory/
│   ├── sqlite/          # Conversation storage
│   └── qdrant/          # Vector search
├── persona/             # AI personalities
├── git/                 # Repository management
└── state.rs            # Application state
```

## API Endpoints

### WebSocket
- `ws://localhost:8080/ws/chat` - Real-time chat with tool support

### REST API

#### Projects
- `GET /api/projects` - List all projects
- `POST /api/projects` - Create project
- `GET /api/project/:id` - Get project details
- `PUT /api/projects/:id` - Update project
- `DELETE /api/projects/:id` - Delete project

#### Git Operations
- `POST /api/projects/:id/git/attach` - Attach Git repo
- `GET /api/projects/:id/git/repos` - List attached repos
- `POST /api/projects/:id/git/files/:attachment_id/content/:path` - Update file
- `GET /api/projects/:id/git/branches/:attachment_id` - List branches
- `GET /api/projects/:id/git/commits/:attachment_id` - Get commit history

#### Artifacts
- `POST /api/artifacts` - Create artifact
- `GET /api/artifacts/:id` - Get artifact
- `PUT /api/artifacts/:id` - Update artifact
- `DELETE /api/artifacts/:id` - Delete artifact

#### Chat History
- `GET /api/chat/history` - Get conversation history

## GPT-5 Tool Integration

The backend supports GPT-5 tools through an extension trait system:

```rust
use crate::services::chat_with_tools::ChatServiceToolExt;

// Extends ChatService with tool support
let response = chat_service.chat_with_tools(
    session_id,
    message,
    project_id,
    file_context
).await?;
```

### Supported Tools
- **Web Search** - Internet search integration
- **Code Interpreter** - Python code execution
- **File Search** - Document retrieval
- **Image Generation** - DALL-E integration

### Tool Response Flow
1. Client sends message via WebSocket
2. Backend checks enabled tools
3. Executes relevant tools based on context
4. Streams results back with citations
5. Frontend displays rich tool results

## WebSocket Protocol

### Client → Server
```json
{
  "type": "chat",
  "content": "message text",
  "project_id": "optional-project-id",
  "metadata": {
    "file_path": "optional/file/path",
    "attachment_id": "optional-id"
  }
}
```

### Server → Client
- `chunk` - Streaming content chunks
- `complete` - Message completion with metadata
- `status` - Processing status updates
- `tool_result` - Tool execution results
- `citation` - Source citations
- `done` - End of stream marker

## Memory System

### Dual Storage
1. **SQLite** - Structured conversation history
   - Messages with timestamps
   - Metadata and tags
   - Session management

2. **Qdrant** - Vector similarity search
   - Semantic memory retrieval
   - Context building
   - Relevant history lookup

### Context Building Pipeline
1. Fetch recent messages (SQLite)
2. Generate embedding for current message
3. Search similar memories (Qdrant)
4. Build recall context
5. Summarize if needed
6. Pass to GPT-5

## Development

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run tests
cargo test

# Check code
cargo clippy

# Format code
cargo fmt

# Watch for changes
cargo watch -x run
```

## Database Migrations

```bash
# Create new migration
sqlx migrate add <name>

# Run migrations
sqlx migrate run

# Revert migration
sqlx migrate revert
```

## Performance Tuning

### Connection Pools
- SQLite: 10 connections (configurable)
- Qdrant: Connection reuse
- HTTP: Keep-alive enabled

### Memory Optimization
- Streaming responses to minimize memory
- Chunked file processing
- Lazy loading for large datasets

### Concurrency
- Tokio async runtime
- Arc/Mutex for shared state
- Lock-free message passing where possible

## Monitoring

### Logs
- Structured logging with tracing
- Configurable log levels
- Request/response tracking

### Health Check
```bash
curl http://localhost:8080/health
```

## Deployment

### Docker
```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/mira-backend /usr/local/bin/
CMD ["mira-backend"]
```

### Systemd Service
```ini
[Unit]
Description=Mira Backend
After=network.target

[Service]
Type=simple
User=mira
WorkingDirectory=/opt/mira
ExecStart=/opt/mira/mira-backend
Restart=on-failure
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
```

## Troubleshooting

### Common Issues

1. **Qdrant Connection Failed**
   ```bash
   # Ensure Qdrant is running
   docker run -p 6333:6333 qdrant/qdrant
   ```

2. **SQLite Locked**
   ```bash
   # Check for hanging processes
   fuser mira.db
   ```

3. **WebSocket Upgrade Failed**
   - Check CORS settings
   - Verify WebSocket headers
   - Ensure no proxy interference

4. **Tool Results Not Showing**
   - Verify environment variables are set
   - Check OpenAI API limits
   - Review logs for tool execution

## Contributing

1. Fork the repository
2. Create a feature branch
3. Write tests for new features
4. Ensure `cargo clippy` passes
5. Submit a Pull Request

## License

Proprietary - ConaryLabs

## Support

For issues and questions:
- Open an issue on GitHub
- Check logs in `RUST_LOG=debug` mode
- Contact the development team
