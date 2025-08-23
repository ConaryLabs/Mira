# Mira

**Advanced AI Development Assistant with Memory and Git Integration**

Mira is a sophisticated AI-powered development assistant built in Rust that combines persistent memory, contextual awareness, and seamless integration with development workflows. Version 0.4.1 features GPT-5 integration, dual memory storage, advanced Git operations, and a flexible persona system.

## ✨ Key Features

### 🧠 Persistent Memory System
- **Dual Storage Architecture**: SQLite for structured data, Qdrant for semantic search
- **Salience-Based Storage**: Intelligent message prioritization (1-10 scale)
- **Context Preservation**: Maintains conversation context across sessions
- **Automatic Summarization**: Prevents context window overflow

### 🔄 Advanced Git Integration
- **Repository Management**: Clone, attach, and sync repositories
- **File Operations**: Tree traversal, content retrieval, diff generation
- **Branch Management**: List branches, switch branches, commit history
- **Smart Diff Parser**: Analyzes changes with hunk-level granularity

### ⚡ Real-Time Communication
- **WebSocket Streaming**: Real-time AI responses
- **REST API**: Complete HTTP API for all operations
- **Session Management**: Per-connection state with heartbeat monitoring

### 🏗 Project-Centric Architecture
- **Project Isolation**: Separate memory and context per project
- **Artifact Management**: Store code, documents, images and notes associated with a project

### 🎭 Multi-Persona System
- **Default**: Standard assistant behavior
- **Haven**: Comforting, supportive presence
- **Hallow**: Sacred, emotionally present interaction
- **Forbidden**: Playful, flirtatious (with safety boundaries)

## 🚀 Quick Start

### Prerequisites
- **Rust 1.70+** - [Install Rust](https://rustup.rs/)
- **OpenAI API Key** - [Get API access](https://platform.openai.com/)
- **Qdrant** (optional) - [Run locally](https://qdrant.tech/) or use cloud

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/ConaryLabs/Mira.git
   cd Mira
   ```

2. Set up environment variables:
   ```bash
   cp .env.example .env
   # Edit .env with your OpenAI API key and other settings
   ```

3. Start Qdrant (optional for semantic search):
   ```bash
   docker run -p 6333:6333 qdrant/qdrant
   ```

4. Build and run:
   ```bash
   cargo build --release
   cargo run
   ```

5. Test the connection:
   ```bash
   curl http://localhost:3001/health
   ```

## ⚙️ Configuration

Mira uses environment variables for configuration. Key settings include:

### Core Configuration
```bash
# OpenAI Configuration
OPENAI_API_KEY=your_api_key_here
MIRA_MODEL=gpt-5
MIRA_MAX_OUTPUT_TOKENS=128000

# Database
DATABASE_URL=sqlite:./mira.db
QDRANT_URL=http://localhost:6333
QDRANT_COLLECTION=mira-memory

# Server
MIRA_HOST=0.0.0.0 
MIRA_PORT=3001

# Memory Settings
MIRA_HISTORY_MESSAGE_CAP=50
MIRA_ENABLE_VECTOR_SEARCH=true
MIRA_MIN_SALIENCE_FOR_QDRANT=7.0

# Git Integration
GIT_REPOS_DIR=./repos
```

### Tool Configuration (GPT-5 Features)
```bash
# Tool integration (GPT-5 Functions API)
ENABLE_CHAT_TOOLS=false

# Reserved for future features
ENABLE_WEB_SEARCH=true # reserved
ENABLE_CODE_INTERPRETER=true # reserved
ENABLE_FILE_SEARCH=true # not implemented yet
ENABLE_IMAGE_GENERATION=true # not implemented yet
```

See `.env.example` for the complete configuration reference.

## 🏗 Architecture Overview

### Directory Structure
```
mira/
├── src/
│   ├── api/                  # HTTP and WebSocket API handlers
│   │   ├── http/            # REST API endpoints
│   │   └── ws/              # WebSocket handlers
│   ├── config/              # Configuration management
│   ├── git/                 # Git integration layer
│   ├── llm/                 # OpenAI client and schema
│   ├── memory/              # Dual storage system
│   │   ├── sqlite/          # SQLite operations
│   │   └── qdrant/          # Vector store operations
│   ├── persona/             # Multi-persona system
│   ├── project/             # Project management
│   ├── services/            # Business logic layer
│   │   ├── chat/            # Modular chat service
│   │   ├── memory.rs        # Memory service
│   │   ├── context.rs       # Context building
│   │   └── ...
│   ├── state.rs             # Application state
│   ├── lib.rs               # Library exports
│   └── main.rs              # Application entry point
├── tests/                   # Integration tests
├── Cargo.toml              # Rust dependencies
└── .env.example            # Configuration template
```

### Core Services

- **ChatService**: Modular chat processing with extracted components
- **MemoryService**: Manages SQLite and Qdrant storage
- **ContextService**: Builds comprehensive context from multiple sources
- **DocumentService**: Handles file processing and project integration
- **SummarizationService**: Automatic conversation compression

## 📡 API Reference

### WebSocket API

**Connect**: `ws://localhost:3001/ws/chat`

**Message Format**:
```json
{
  "type": "chat",
  "message": "Hello, Mira!",
  "project_id": "optional-project-uuid"
}
```

**Response Format**:
```json
{
  "output": "Hello! How can I assist you today?",
  "persona": "default",
  "mood": "helpful",
  "salience": 5,
  "tags": ["greeting"],
  "memory_type": "interaction"
}
```

### REST API

#### Chat Endpoints
```bash
# Send chat message
POST /api/chat
{
  "message": "Help me debug this code",
  "session_id": "my-session",
  "project_id": "optional-project-id"
}

# Get chat history
GET /api/chat/history?session_id=my-session&limit=10
```

#### Project Management
```bash
# Create project
POST /api/projects
{
  "name": "My Project",
  "description": "A cool project",
  "tags": ["rust", "ai"]
}

# Get project details
GET /api/project/{project_id}

# Attach Git repository
POST /api/projects/{project_id}/git/attach
{
  "repo_url": "https://github.com/user/repo.git"
}
```

#### Git Operations
```bash
# Get file tree
GET /api/projects/{project_id}/git/files/{attachment_id}/tree

# Get file content
GET /api/projects/{project_id}/git/files/{attachment_id}/content/src/main.rs

# List branches
GET /api/projects/{project_id}/git/branches/{attachment_id}

# Get commit history 
GET /api/projects/{project_id}/git/commits/{attachment_id}
```

## 🧪 Testing

Run the comprehensive test suite:

```bash
# All tests
cargo test

# Specific test module
cargo test test_project_system

# With output
cargo test -- --nocapture

# Generate test coverage
cargo tarpaulin --html
```

## 📋 Roadmap

### Current Version (v0.4.1)
- ✅ GPT-5 integration
- ✅ Dual memory storage
- ✅ Git integration
- ✅ WebSocket streaming
- ✅ Multi-persona system

### Planned Enhancements
- Enhanced Git workflows (more diff analytics, merge assistance)
- File search and image generation tools
- Basic team collaboration features (shared projects, role-based access)
- Custom model fine-tuning

### Longer-Term Ideas
- Multi-modal capabilities (images, audio)
- Advanced code generation and refactoring assistance
- CI/CD integration
- Enterprise features and robust authentication

## 📄 License

No LICENSE file is present in this repository. Licensing terms have not yet been finalized.

## 🆘 Support

- **Issues**: [GitHub Issues](https://github.com/ConaryLabs/Mira/issues)
- **Discussions**: [GitHub Discussions](https://github.com/ConaryLabs/Mira/discussions)
- **Documentation**: [Wiki](https://github.com/ConaryLabs/Mira/wiki)
- **Email**: support@conarylabs.com

## 🙏 Acknowledgments

- OpenAI for GPT-5 and embedding models
- Qdrant for vector storage capabilities
- Rust Community for excellent ecosystem
- Contributors who make this project possible

---

Built with ❤️ by ConaryLabs
