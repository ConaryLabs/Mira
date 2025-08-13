# Mira Backend v2.0 - GPT-5 Edition

> An advanced conversational AI system powered by OpenAI's GPT-5 and the unified Responses API

## Overview

Mira is a sophisticated AI assistant backend that leverages OpenAI's latest GPT-5 model through the unified `/v1/responses` endpoint. As of August 2025, Mira has been fully migrated to use GPT-5's advanced capabilities including:

- **Unified Responses API**: Single endpoint for text, images, and function calls
- **GPT-5 Language Model**: Enhanced reasoning and extended context (400k+ tokens)
- **GPT-Image-1**: Integrated image generation via the same API
- **Functions API**: Structured tool usage for memory evaluation
- **Vector Store Integration**: OpenAI's native vector search for document Q&A
- **Smart Memory System**: Automatic tagging and retrieval with semantic search

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   API Layer                         │
├─────────────────────────────────────────────────────┤
│  REST Handler (/chat)  │  WebSocket Handler (/ws)   │
└────────────┬───────────┴──────────┬─────────────────┘
             │                      │
             ▼                      ▼
┌─────────────────────────────────────────────────────┐
│                ChatService (Unified)                │
│  • Thread Management                                │
│  • Context Building                                 │
│  • Vector Store Search                              │
│  • GPT-5 Integration                                │
│  • Memory Evaluation                                │
└─────────────────────────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────────────────────────┐
│              OpenAI GPT-5 (/v1/responses)           │
│  • Text Generation                                  │
│  • Image Generation (GPT-Image-1)                   │
│  • Function Calls (evaluate_memory)                 │
│  • Vector Store API                                 │
└─────────────────────────────────────────────────────┘
```

## Features

### 🤖 GPT-5 Integration
- Full GPT-5 support via unified Responses API
- Configurable verbosity and reasoning depth
- Extended context window support (400k+ tokens)
- Structured JSON output with schema validation

### 🎨 Image Generation
- GPT-Image-1 model integration
- Multiple size and quality options
- Streaming response support
- Temporary URL management

### 🧠 Intelligent Memory System
- Automatic salience scoring (1-10)
- Semantic tagging and categorization
- Near-duplicate detection
- Role-scoped memory storage
- Qdrant vector database for semantic search

### 📚 Document Q&A
- OpenAI Vector Store integration
- Automatic document chunking and indexing
- Relevance-based retrieval
- Project-scoped document collections

### 💬 Unified Chat Experience
- REST and WebSocket endpoints
- Consistent response format
- Thread-based conversation management
- Automatic context enrichment

## Configuration

### Environment Variables

```bash
# Required
OPENAI_API_KEY=sk-...                  # Your OpenAI API key

# Optional - Model Configuration
MIRA_MODEL=gpt-5                       # Model to use (default: gpt-5)
MIRA_VERBOSITY=medium                  # Output verbosity: low/medium/high
MIRA_REASONING_EFFORT=medium           # Reasoning depth: minimal/medium/high
MIRA_MAX_OUTPUT_TOKENS=1024            # Maximum response tokens
MIRA_PERSONA=Default                   # Persona overlay to use

# Optional - Memory & History
MIRA_HISTORY_MESSAGE_CAP=24            # Max messages to keep in history
MIRA_HISTORY_TOKEN_LIMIT=8192          # Token budget for conversation history
MIRA_MAX_RETRIEVAL_TOKENS=2000         # Max tokens for retrieved documents

# Optional - Storage
QDRANT_URL=http://localhost:6333       # Qdrant vector database URL
QDRANT_COLLECTION=mira-memory          # Qdrant collection name

# Optional - Debugging
MIRA_DEBUG_LOGGING=false               # Enable verbose debug logs
```

## API Endpoints

### REST API

#### POST /chat
Send a chat message and receive a response.

```json
{
  "message": "Hello, how are you?",
  "project_id": "optional-project-id"
}
```

Response:
```json
{
  "output": "I'm doing well, thank you for asking!",
  "persona": "Default",
  "mood": "friendly",
  "salience": 5,
  "summary": "Greeting exchange",
  "memory_type": "event",
  "tags": ["greeting", "casual"],
  "intent": "social",
  "monologue": null,
  "reasoning_summary": null
}
```

### WebSocket API

#### /ws/chat
Real-time streaming chat interface.

Message types:
- `Chat`: Send a message
- `Command`: Execute commands (ping, set_project, get_status)
- `Status`: Status updates

## Installation

### Prerequisites
- Rust 1.75+
- SQLite 3
- Qdrant (optional, for vector search)
- OpenAI API key

### Building

```bash
# Clone the repository
git clone https://github.com/yourorg/mira-backend.git
cd mira-backend

# Build the project
cargo build --release

# Run migrations
cargo run --bin migrate

# Start the server
cargo run --release
```

### Docker

```bash
# Build the image
docker build -t mira-backend .

# Run the container
docker run -p 8080:8080 \
  -e OPENAI_API_KEY=your-key \
  -e QDRANT_URL=http://qdrant:6333 \
  mira-backend
```

## Testing

```bash
# Run all tests
cargo test

# Run with logging
RUST_LOG=debug cargo test

# Run specific test suite
cargo test test_chat_persistence
```

## Development

### Project Structure

```
src/
├── llm/                    # LLM integration layer
│   ├── client.rs          # OpenAI HTTP client
│   ├── memory_eval.rs     # Functions API for memory evaluation
│   ├── schema.rs          # Request/response schemas
│   └── responses/         # Responses API modules
│       ├── image.rs       # Image generation
│       ├── thread.rs      # Thread management
│       └── vector_store.rs # Vector store integration
├── services/              # Business logic
│   ├── chat.rs           # Unified chat service
│   ├── memory.rs         # Memory management
│   ├── context.rs        # Context building
│   └── document.rs       # Document processing
├── api/                   # API layer
│   ├── handlers.rs       # REST handlers
│   └── ws/               # WebSocket handlers
├── memory/               # Storage layer
│   ├── sqlite/           # SQLite persistence
│   └── qdrant/           # Vector search
└── main.rs              # Application entry point
```

### Adding New Features

1. **New Personas**: Add to `src/persona/mod.rs`
2. **New Functions**: Define schema in `src/llm/schema.rs`
3. **New Endpoints**: Add handlers in `src/handlers.rs`

## Performance

### Optimizations
- Token-based history trimming
- Near-duplicate detection
- Async/await throughout
- Connection pooling for databases
- Smart embedding decisions based on salience

### Benchmarks
- Average response time: ~1.5s
- Token throughput: 1000-2000 tokens/second
- Vector search: <100ms for 10k documents
- Memory evaluation: ~500ms

## Monitoring

The system provides comprehensive structured logging:

```
🚀 Processing chat message
📜 History: 12 messages after trimming
💭 Personal context: 3 recent, 2 semantic matches
📚 Vector store: 2 results, scores: [0.92, 0.87]
🤖 Calling GPT-5 with parameters: verbosity=medium, reasoning=medium, max_tokens=1024
✅ GPT-5 responded in 1.23s
📊 Token usage: prompt=1250, completion=487, total=1737
💬 Response: salience=8/10, mood=thoughtful, tags=["technical", "helpful"]
⏱️ Total processing time: 1.85s
```

## Migration from Previous Versions

### From v1.x (GPT-4/Claude)
1. Update all environment variables (see Configuration)
2. Run database migrations: `cargo run --bin migrate`
3. Update any custom integrations to use the new response format
4. Remove any references to deprecated models or endpoints

### Key Changes in v2.0
- ✅ Single unified API endpoint (`/v1/responses`)
- ✅ No more model-specific code paths
- ✅ Functions API replaces custom tool implementations
- ✅ Native vector store integration
- ✅ Enhanced structured output with schemas
- ✅ Comprehensive configuration via environment variables

## Contributing

Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details on our code of conduct and the process for submitting pull requests.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- OpenAI for GPT-5 and the unified Responses API
- Qdrant team for the excellent vector database
- Rust async ecosystem contributors

---

*As of August 2025, Mira is fully migrated to GPT-5 with the unified Responses API, providing a single coherent assistant backend with improved performance and maintainability.*
