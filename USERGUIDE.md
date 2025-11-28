# Mira User Guide

A comprehensive guide for users and developers of the Mira AI coding assistant.

## Table of Contents

- [Introduction](#introduction)
- [Quick Start](#quick-start)
- [Web Interface](#web-interface)
- [Projects and Workspaces](#projects-and-workspaces)
- [Code Intelligence](#code-intelligence)
- [AI Capabilities](#ai-capabilities)
- [WebSocket API Reference](#websocket-api-reference)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)

---

## Introduction

Mira is an AI-powered coding assistant that combines GPT 5.1 with a hybrid memory system (SQLite + Qdrant vector database) for intelligent, context-aware code assistance. Key capabilities include:

- **Conversational Coding**: Chat with an AI that understands your codebase
- **Code Intelligence**: Semantic search, complexity analysis, and pattern detection
- **Git Integration**: Full repository history, blame, and co-change analysis
- **Persistent Memory**: Conversations and context preserved across sessions
- **39 Built-in Tools**: File operations, git commands, web search, code analysis
- **Project Guidelines**: Per-project instructions that persist across conversations
- **Task Tracking**: Track work items that persist across sessions

### Architecture Overview

```
Frontend (React + TypeScript)
        |
  WebSocket (:3001)
        |
Backend (Rust + Axum)
        |
   +----+----+----+
   |         |    |
SQLite   Qdrant  GPT 5.1
(50 tbl) (3 coll) (OpenAI)
```

---

## Quick Start

### Prerequisites

| Component | Version | Purpose |
|-----------|---------|---------|
| Rust | 1.91+ | Backend |
| Node.js | 18+ | Frontend |
| SQLite | 3.35+ | Structured storage |
| Qdrant | 1.16+ | Vector embeddings |
| OpenAI API Key | - | GPT 5.1 + embeddings |

### Installation

#### 1. Start Qdrant (Vector Database)

```bash
# Using bundled binary
cd backend
./bin/qdrant --config-path ./config/config.yaml

# Or using Docker
docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest
```

#### 2. Setup Backend

```bash
cd backend

# Configure environment
cp .env.example .env
# Edit .env and set OPENAI_API_KEY=sk-your-key

# Build and run migrations
cargo build
sqlx migrate run

# Start server (WebSocket on port 3001)
cargo run
```

#### 3. Setup Frontend

```bash
cd frontend

npm install
npm run dev
```

Open `http://localhost:5173` in your browser.

---

## Web Interface

### Layout Overview

The interface consists of three main areas:

```
+------------------+-------------------+------------------+
|     Header       |                   |                  |
+------------------+                   |                  |
|                  |                   |                  |
|   Chat Area      |   Artifact Panel  |  Activity Panel  |
|                  |   (code editor)   |  (reasoning/     |
|                  |                   |   tasks/tools)   |
|                  |                   |                  |
+------------------+-------------------+------------------+
|   File Browser   |                   |                  |
+------------------+-------------------+------------------+
```

### Chat Area

The primary interface for interacting with Mira.

**Features:**
- Real-time streaming responses with cancellation support
- Markdown rendering with syntax highlighting
- Code blocks can be saved as artifacts
- Message history preserved across sessions

**Usage:**
- Type your message and press Enter or click Send
- Click the stop button to cancel a response in progress
- Use the project selector to attach repository context

### Activity Panel

Real-time view of AI operations (toggle with the activity button in header).

**Sections:**
1. **Reasoning**: Shows the AI's thinking process and planning
2. **Tasks**: Displays task progress with status indicators
3. **Tool Executions**: Live feed of tool calls with expandable details

### Artifact Panel

Code editor for viewing and managing AI-generated code.

**Features:**
- Monaco Editor with full syntax highlighting
- Diff view comparing original vs modified code
- Save artifacts to project files
- Copy code to clipboard

### File Browser

Navigate project files with code intelligence indicators.

**Semantic Tags:**
- Test file indicator (beaker icon)
- Quality issues count (warning icon)
- Complexity score (gauge icon)
- Analyzed status (checkmark)

**Usage:**
- Click folders to expand/collapse
- Click files to view content
- Toggle semantic tags with the eye icon

### Intelligence Panel

Access code intelligence features via tabs:

| Tab | Purpose |
|-----|---------|
| Search | Semantic code search |
| Co-change | Files frequently modified together |
| Builds | Build errors and history |
| Tools | Synthesized tools and patterns |

---

## Projects and Workspaces

### Creating a Project

1. Click **Projects** in the sidebar
2. Click **Create Project**
3. Enter project name and optional description
4. Click **Create**

### Attaching a Codebase

Link a git repository to your project for full code intelligence:

1. Open your project
2. Click **Attach Codebase**
3. Enter repository path or URL
4. Click **Attach**

Once attached, Mira can:
- Search and read your code
- Analyze complexity and patterns
- Track git history and blame
- Detect co-change patterns

### Project Settings

Configure per-project guidelines:

1. Click **Settings** on any project card
2. Edit the guidelines in markdown format
3. Click **Save Guidelines**

Guidelines are automatically included in every conversation about that project. Use them for:
- Coding standards and conventions
- Architecture decisions
- Framework preferences
- Team workflows

**Example Guidelines:**
```markdown
# Project Guidelines

## Stack
- Backend: Rust with Axum
- Frontend: React + TypeScript
- Database: PostgreSQL

## Code Style
- Use functional components with hooks
- Prefer async/await over callbacks
- Write tests for all new features

## Important Notes
- Never commit .env files
- Run `cargo fmt` before committing
```

### Task Tracking

Mira can track work items that persist across sessions:

**Creating Tasks:**
- Ask Mira to create a task: "Create a task to implement user authentication"
- Mira uses `manage_project_task` tool to track it

**Task Features:**
- Priority levels: low, medium, high, critical
- Progress notes and status updates
- Links to artifacts and commits
- Visible in system context for continuity

---

## Code Intelligence

### Semantic Search

Find code by meaning, not just keywords:

1. Open **Intelligence Panel** > **Search** tab
2. Enter natural language query (e.g., "authentication middleware")
3. View semantically relevant results

**Examples:**
- "error handling utilities" - finds error handlers and validators
- "database connection pool" - finds connection management code
- "user validation" - finds input validation functions

### Co-Change Suggestions

Discover files frequently modified together:

1. Select a file in the browser
2. Open **Intelligence Panel** > **Co-change** tab
3. See related files with confidence scores

**Use Cases:**
- Find files you might forget to update
- Understand implicit dependencies
- Identify tightly coupled code

### Build Errors Panel

Track and resolve build errors:

**Tabs:**
- **Errors**: Unresolved build errors with details
- **Builds**: Recent build history with status
- **Stats**: Build success rate and metrics

### Tools Dashboard

View synthesized tools and patterns:

**Tabs:**
- **Tools**: Auto-generated tools from your codebase
- **Patterns**: Detected coding patterns
- **Stats**: Tool effectiveness metrics

---

## AI Capabilities

Mira has access to 39 built-in tools organized by category.

### File Operations (8 tools)

| Tool | Description |
|------|-------------|
| `read_project_file` | Read one or more project files |
| `write_project_file` | Write content to project files |
| `write_file` | Write to any file on the system |
| `edit_project_file` | Search/replace edits to existing files |
| `search_codebase` | Search for patterns across project |
| `list_project_files` | List directory contents |
| `get_file_summary` | Quick overview without full read (saves 80-90% tokens) |
| `get_file_structure` | Extract symbols without reading source |

### Code Generation (3 tools)

| Tool | Description |
|------|-------------|
| `generate_code` | Create new code files from scratch |
| `refactor_code` | Modify and improve existing code |
| `debug_code` | Fix bugs and errors |

### External Tools (3 tools)

| Tool | Description |
|------|-------------|
| `web_search` | Search web for docs, examples, error solutions |
| `fetch_url` | Fetch and extract content from URLs |
| `execute_command` | Run shell commands (full system access) |

### Git Analysis (10 tools)

| Tool | Description |
|------|-------------|
| `git_history` | Commit history with filtering |
| `git_blame` | Line-by-line authorship |
| `git_diff` | Compare commits or branches |
| `git_file_history` | All commits affecting a file |
| `git_branches` | Branch listing with status |
| `git_show_commit` | Detailed commit information |
| `git_file_at_commit` | File content at specific commit |
| `git_recent_changes` | Recently modified files |
| `git_contributors` | Author statistics |
| `git_status` | Working tree status |

### Code Intelligence (12 tools)

| Tool | Description |
|------|-------------|
| `find_function` | Find function definitions by name/pattern |
| `find_class_or_struct` | Find type definitions |
| `search_code_semantic` | Natural language code search |
| `find_imports` | Find where symbols are imported |
| `analyze_dependencies` | Analyze project dependencies |
| `get_complexity_hotspots` | Find high-complexity code |
| `get_quality_issues` | Get code quality problems |
| `get_file_symbols` | List all symbols in a file |
| `find_tests_for_code` | Find related tests |
| `get_codebase_stats` | Overall codebase metrics |
| `find_callers` | Find function call sites |
| `get_element_definition` | Get full code element details |

### Project Management (2 tools)

| Tool | Description |
|------|-------------|
| `manage_project_task` | Create/update/complete persistent tasks |
| `manage_project_guidelines` | Get/set/append project guidelines |

### Skills System (1 tool)

| Tool | Description |
|------|-------------|
| `activate_skill` | Activate specialized modes: refactoring, testing, debugging, documentation |

---

## WebSocket API Reference

Connect to Mira programmatically via WebSocket at `ws://localhost:3001/ws`.

### Message Format

All messages are JSON with a `type` field:

```json
{
  "type": "project_command",
  "method": "project.list",
  "params": {}
}
```

### Project Commands

#### List Projects
```json
// Request
{"type": "project_command", "method": "project.list", "params": {}}

// Response
{"type": "data", "data": {"projects": [...]}}
```

#### Create Project
```json
// Request
{
  "type": "project_command",
  "method": "project.create",
  "params": {
    "name": "My Project",
    "description": "Optional description"
  }
}
```

#### Get Project
```json
{"type": "project_command", "method": "project.get", "params": {"project_id": "uuid"}}
```

#### Update Project
```json
{
  "type": "project_command",
  "method": "project.update",
  "params": {
    "project_id": "uuid",
    "name": "New Name",
    "description": "New description"
  }
}
```

#### Delete Project
```json
{"type": "project_command", "method": "project.delete", "params": {"project_id": "uuid"}}
```

### Guidelines Commands

#### Get Guidelines
```json
{"type": "project_command", "method": "guidelines.get", "params": {"project_id": "uuid"}}

// Response
{"type": "data", "data": {"type": "guidelines", "content": "...", "file_path": "..."}}
```

#### Set Guidelines
```json
{
  "type": "project_command",
  "method": "guidelines.set",
  "params": {
    "project_id": "uuid",
    "content": "# Guidelines\n\n..."
  }
}
```

#### Delete Guidelines
```json
{"type": "project_command", "method": "guidelines.delete", "params": {"project_id": "uuid"}}
```

### Artifact Commands

#### List Artifacts
```json
{"type": "project_command", "method": "artifact.list", "params": {"project_id": "uuid"}}
```

#### Create Artifact
```json
{
  "type": "project_command",
  "method": "artifact.create",
  "params": {
    "project_id": "uuid",
    "file_path": "src/utils.ts",
    "content": "export function...",
    "language": "typescript"
  }
}
```

#### Get Artifact
```json
{"type": "project_command", "method": "artifact.get", "params": {"artifact_id": "uuid"}}
```

### Code Intelligence Commands

#### Semantic Search
```json
{
  "type": "code_command",
  "method": "code.semantic_search",
  "params": {
    "project_id": "uuid",
    "query": "authentication middleware",
    "limit": 10
  }
}
```

#### Get Co-change Patterns
```json
{
  "type": "code_command",
  "method": "code.cochange",
  "params": {
    "project_id": "uuid",
    "file_path": "src/auth.ts"
  }
}
```

#### Get Complexity Hotspots
```json
{
  "type": "code_command",
  "method": "code.complexity_hotspots",
  "params": {
    "project_id": "uuid",
    "min_complexity": 10,
    "limit": 10
  }
}
```

#### Get Build Errors
```json
{"type": "code_command", "method": "code.build_errors", "params": {"project_id": "uuid"}}
```

#### Get Budget Status
```json
{"type": "code_command", "method": "code.budget_status", "params": {}}
```

### Memory Commands

#### Save Memory
```json
{
  "type": "memory_command",
  "method": "memory.save",
  "params": {
    "content": "Important context...",
    "session_id": "session-uuid"
  }
}
```

#### Search Memory
```json
{
  "type": "memory_command",
  "method": "memory.search",
  "params": {
    "query": "authentication flow",
    "limit": 10
  }
}
```

#### Get Recent Messages
```json
{
  "type": "memory_command",
  "method": "memory.get_recent",
  "params": {
    "session_id": "session-uuid",
    "limit": 50
  }
}
```

### Git Commands

#### Attach Repository
```json
{
  "type": "git_command",
  "method": "git.attach",
  "params": {
    "project_id": "uuid",
    "path": "/path/to/repo"
  }
}
```

#### Clone Repository
```json
{
  "type": "git_command",
  "method": "git.clone",
  "params": {
    "project_id": "uuid",
    "url": "https://github.com/user/repo.git"
  }
}
```

#### Get File Content
```json
{
  "type": "git_command",
  "method": "git.get_file",
  "params": {
    "project_id": "uuid",
    "path": "src/main.rs"
  }
}
```

### Filesystem Commands

#### List Directory
```json
{
  "type": "fs_command",
  "method": "fs.list",
  "params": {
    "path": "/home/user/project/src"
  }
}
```

#### Read File
```json
{
  "type": "fs_command",
  "method": "fs.read",
  "params": {
    "path": "/home/user/project/README.md"
  }
}
```

---

## Configuration

### Environment Variables

Key settings in `backend/.env`:

#### LLM Configuration

```bash
# OpenAI API key (required)
OPENAI_API_KEY=sk-proj-your-key-here

# Model settings
GPT5_MODEL=gpt-5.1
GPT5_REASONING_DEFAULT=medium  # low, medium, high
OPENAI_EMBEDDING_MODEL=text-embedding-3-large
```

#### Budget Management

```bash
# Spending limits (USD)
BUDGET_DAILY_LIMIT_USD=5.0
BUDGET_MONTHLY_LIMIT_USD=150.0
```

#### Cache Configuration

```bash
# LLM response cache
CACHE_ENABLED=true
CACHE_TTL_SECONDS=86400  # 24 hours
```

#### Database

```bash
DATABASE_URL=sqlite:./data/mira.db
QDRANT_URL=http://localhost:6334  # gRPC port
QDRANT_COLLECTION=mira
```

#### Server

```bash
MIRA_HOST=0.0.0.0
MIRA_PORT=3001
```

#### Memory System

```bash
# Context limits
MIRA_CONTEXT_RECENT_MESSAGES=50
MIRA_CONTEXT_SEMANTIC_MATCHES=25
MIRA_HISTORY_TOKEN_LIMIT=131072

# Embedding settings
MEM_SALIENCE_MIN_FOR_EMBED=0.5
MEM_DEDUP_SIM_THRESHOLD=0.97
```

#### Summarization

```bash
MIRA_ENABLE_SUMMARIZATION=true
MIRA_SUMMARIZE_AFTER_MESSAGES=20
MIRA_USE_ROLLING_SUMMARIES_IN_CONTEXT=true
```

### Reasoning Effort Levels

GPT 5.1 supports variable reasoning effort:

| Level | Use Case | Cost |
|-------|----------|------|
| `low` (minimum) | Simple queries, quick answers | Lowest |
| `medium` | General coding tasks | Moderate |
| `high` | Complex planning, architecture | Highest |

Mira automatically selects reasoning effort based on task complexity.

### Qdrant Configuration

Qdrant runs on two ports:
- **6333**: REST API (health checks, admin)
- **6334**: gRPC (client connections)

Configuration file at `backend/config/config.yaml`:

```yaml
storage:
  storage_path: ./data/qdrant

service:
  http_port: 6333
  grpc_port: 6334
```

---

## Troubleshooting

### Common Issues

#### Backend won't start

**Port already in use:**
```bash
lsof -i :3001
kill -9 <PID>
```

**Missing API key:**
```
Error: OPENAI_API_KEY not set
```
Ensure `.env` file exists with valid key.

#### Qdrant connection failed

**Check if Qdrant is running:**
```bash
curl http://localhost:6333/health
```

**Start Qdrant:**
```bash
cd backend
./bin/qdrant --config-path ./config/config.yaml
```

#### Database errors

**Reset database:**
```bash
cd backend
./scripts/db-reset.sh  # Full reset
```

**Run migrations:**
```bash
DATABASE_URL="sqlite:./data/mira.db" sqlx migrate run
```

### Debug Logging

Enable verbose logging:

```bash
# All debug logs
RUST_LOG=debug cargo run

# Specific module
RUST_LOG=mira_backend::operations=trace cargo run

# Memory system
RUST_LOG=mira_backend::memory=debug cargo run
```

### Database Reset Scripts

Located in `backend/scripts/`:

| Script | Purpose |
|--------|---------|
| `db-reset.sh` | Full reset (SQLite + Qdrant) |
| `db-reset-sqlite.sh` | Reset SQLite only |
| `db-reset-qdrant.sh` | Reset Qdrant only |
| `db-reset-test.sh` | Clean up test collections |

### Health Checks

**Backend:**
```bash
curl http://localhost:3001/health
```

**Qdrant:**
```bash
curl http://localhost:6333/health
```

**List Qdrant collections:**
```bash
curl http://localhost:6333/collections
```

---

## Additional Resources

- **[README.md](./README.md)** - Project overview and quick start
- **[CLAUDE.md](./CLAUDE.md)** - AI assistant development guide
- **[backend/WHITEPAPER.md](./backend/WHITEPAPER.md)** - Technical architecture
- **[PROGRESS.md](./PROGRESS.md)** - Development session history
- **[frontend/docs/STATE_BOUNDARIES.md](./frontend/docs/STATE_BOUNDARIES.md)** - Frontend state management

---

*Last updated: 2025-11-28*
