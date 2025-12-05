# Mira User Guide

A comprehensive guide for users and developers of the Mira AI coding assistant.

## Table of Contents

- [Introduction](#introduction)
- [Quick Start](#quick-start)
- [Web Interface](#web-interface)
- [Command Line Interface](#command-line-interface)
- [Projects and Workspaces](#projects-and-workspaces)
- [Code Intelligence](#code-intelligence)
- [AI Capabilities](#ai-capabilities)
- [Slash Commands](#slash-commands)
- [Hooks System](#hooks-system)
- [Checkpoint and Rewind](#checkpoint-and-rewind)
- [MCP Support](#mcp-support)
- [WebSocket API Reference](#websocket-api-reference)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)

---

## Introduction

Mira is an AI-powered coding assistant that combines Gemini 3 Pro with a hybrid memory system (SQLite + Qdrant vector database) for intelligent, context-aware code assistance. Key capabilities include:

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
SQLite   Qdrant  Gemini 3 Pro
(50 tbl) (3 coll) (Google)
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
| Google API Key | - | Gemini 3 Pro + embeddings |

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
# Edit .env and set GOOGLE_API_KEY=your-google-api-key

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

## Command Line Interface

Mira provides a full-featured command-line interface for terminal-based workflows.

### Installation

The CLI is built as part of the backend:

```bash
cd backend
cargo build --release
```

The binary is located at `target/release/mira`.

### Basic Usage

```bash
# Start interactive REPL
mira

# Send a single prompt (non-interactive)
mira -p "explain this error"

# Continue the most recent session
mira -c

# Resume a specific session
mira -r session-id

# Show session picker
mira -r
```

### Command Line Options

| Option | Description |
|--------|-------------|
| `-p, --print` | Non-interactive mode - send prompt, print response, exit |
| `-c, --continue` | Continue the most recent conversation |
| `-r, --resume [ID]` | Resume a session by ID, or show picker if no ID |
| `--project <PATH>` | Set project root directory (auto-detected if not specified) |
| `-v, --verbose` | Show tool executions and reasoning |
| `--show-thinking` | Display thinking/reasoning tokens |
| `--output-format <FMT>` | Output format: `text`, `json`, `stream-json` |
| `--tools <LIST>` | Filter available tools (comma-separated) |
| `--fork <ID>` | Fork from an existing session |
| `--system-prompt <TEXT>` | Override the system prompt |
| `--append-system-prompt <TEXT>` | Append to system prompt |
| `--max-turns <N>` | Maximum turns for non-interactive mode (default: 10) |
| `--no-color` | Disable colored output |
| `--backend-url <URL>` | Backend WebSocket URL (default: ws://localhost:3001/ws) |

### Interactive Mode (REPL)

When started without `-p`, Mira runs an interactive REPL:

```
$ mira
Mira v0.1.0 - AI Coding Assistant
Session: abc123 | Project: ~/myproject

> What files handle authentication?

[Response streams here...]

> /commands
Available commands:
  /review - Code review prompt
  /test   - Generate tests

> exit
```

**REPL Features:**
- Real-time streaming responses
- Session persistence across restarts
- Project context auto-detection (git repos)
- Slash commands from `.mira/commands/`
- Sudo approval prompts for dangerous operations

### Session Management

Sessions preserve conversation history:

```bash
# List recent sessions
mira -r
# Shows picker:
#   1. [abc123] 2 hours ago - "Fix auth bug"
#   2. [def456] 1 day ago - "Add user validation"

# Resume specific session
mira -r abc123

# Continue most recent
mira -c

# Fork a session (creates new session with same history)
mira --fork abc123
```

### Project Context

The CLI auto-detects project context from git repositories:

```bash
# Auto-detect from current directory
cd ~/myproject
mira

# Explicitly set project root
mira --project ~/myproject
```

When a project is detected:
- File operations are relative to project root
- Git history is available for analysis
- Code intelligence features are enabled
- Project guidelines are loaded from `.mira/`

### Output Formats

```bash
# Human-readable (default)
mira -p "explain this code"

# Structured JSON
mira -p "explain this code" --output-format json

# Streaming JSON (newline-delimited)
mira -p "explain this code" --output-format stream-json
```

JSON output is useful for scripting and integrations.

### Tool Filtering

Restrict which tools the AI can use:

```bash
# Only allow read operations
mira --tools read_project_file,search_codebase

# Allow git tools only
mira --tools "git_*"
```

### Custom System Prompts

```bash
# Override system prompt entirely
mira --system-prompt "You are a Python expert"

# Append to default system prompt
mira --append-system-prompt "Always use type hints"
```

### Examples

```bash
# Quick code explanation
mira -p "explain src/auth.rs"

# Interactive debugging session
mira -c
> I'm seeing an error in the auth module
> [paste error]

# Generate tests for a file
mira -p "generate tests for src/utils.rs" --output-format json

# Code review with verbose output
mira -v -p "review the changes in this PR"

# Fork session for experimentation
mira --fork abc123
> Let's try a different approach...
```

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

### Built-in Commands

Mira provides several built-in slash commands accessible from the chat:

| Command | Description |
|---------|-------------|
| `/commands` | List all available custom slash commands |
| `/reload-commands` | Hot-reload commands from disk |
| `/checkpoints` | List recent checkpoints for the session |
| `/rewind <id>` | Restore files to a checkpoint state |
| `/mcp` | List connected MCP servers and their tools |

---

## Slash Commands

Create custom reusable prompts using markdown files.

### Overview

Slash commands let you define frequently-used prompts that can be invoked with `/command-name`. Commands are markdown files with optional `$ARGUMENTS` placeholders.

### Command Locations

| Location | Scope | Priority |
|----------|-------|----------|
| `.mira/commands/` | Project-specific | Higher |
| `~/.mira/commands/` | User-global | Lower |

Project commands take precedence over user commands with the same name.

### Creating Commands

1. Create a directory: `mkdir -p .mira/commands`
2. Create a markdown file: `.mira/commands/review.md`

```markdown
# Code Review

Review the following code for:
- Security vulnerabilities
- Performance issues
- Code style violations
- Missing error handling

$ARGUMENTS
```

3. Use in chat: `/review <paste your code here>`

### Command Features

**Arguments**: Use `$ARGUMENTS` as a placeholder for user input:
```markdown
# Explain Code
Explain this code in detail:

$ARGUMENTS
```

**Description Extraction**: The first `#` header becomes the command description shown in `/commands`.

**Namespacing**: Nested folders create namespaced commands:
```
.mira/commands/
  git/
    pr.md      -> /git:pr
    commit.md  -> /git:commit
  test/
    unit.md    -> /test:unit
```

### Example Commands

**Refactor Command** (`.mira/commands/refactor.md`):
```markdown
# Refactor Code

Refactor this code to improve:
1. Readability
2. Performance
3. Maintainability

Keep the same functionality. Explain your changes.

$ARGUMENTS
```

**Test Generator** (`.mira/commands/test.md`):
```markdown
# Generate Tests

Generate comprehensive unit tests for:

$ARGUMENTS

Include:
- Happy path tests
- Edge cases
- Error handling tests
```

### Managing Commands

- `/commands` - View all available commands with descriptions
- `/reload-commands` - Reload after adding/modifying command files

---

## Hooks System

Execute shell commands before or after tool operations.

### Overview

Hooks let you run custom scripts at specific points in Mira's tool execution pipeline. Use them for:
- Running tests before file writes
- Formatting code after edits
- Logging tool usage
- Blocking dangerous operations

### Configuration

Create `.mira/hooks.json` (project) or `~/.mira/hooks.json` (user):

```json
{
  "hooks": [
    {
      "name": "pre-write-test",
      "trigger": "pre_tool_use",
      "tool_pattern": "write_*",
      "command": "cargo test",
      "timeout_ms": 60000,
      "on_failure": "block"
    }
  ]
}
```

### Hook Properties

| Property | Required | Description |
|----------|----------|-------------|
| `name` | Yes | Unique identifier for the hook |
| `trigger` | Yes | When to run: `pre_tool_use` or `post_tool_use` |
| `tool_pattern` | No | Tool name pattern (supports `*` wildcard) |
| `command` | Yes | Shell command to execute |
| `timeout_ms` | No | Timeout in milliseconds (default: 30000) |
| `on_failure` | No | Action on failure: `block`, `warn`, `ignore` (default: `warn`) |

### Trigger Types

| Trigger | Description |
|---------|-------------|
| `pre_tool_use` | Runs before tool execution |
| `post_tool_use` | Runs after tool execution |

### Pattern Matching

Tool patterns support wildcards:
- `write_*` matches `write_file`, `write_project_file`
- `git_*` matches all git tools
- `*` matches all tools
- Exact name matches specific tool

### Environment Variables

Hooks receive context via environment variables:

| Variable | Available | Description |
|----------|-----------|-------------|
| `MIRA_TOOL_NAME` | Both | Name of the tool being executed |
| `MIRA_TOOL_ARGS` | Both | JSON string of tool arguments |
| `MIRA_TOOL_SUCCESS` | Post only | "true" or "false" |
| `MIRA_TOOL_RESULT` | Post only | JSON string of tool result |

### On-Failure Actions

| Action | Description |
|--------|-------------|
| `block` | Abort tool execution (pre-hooks only) |
| `warn` | Log warning, continue execution |
| `ignore` | Silently continue |

### Example Configurations

**Run Tests Before File Writes:**
```json
{
  "hooks": [
    {
      "name": "test-before-write",
      "trigger": "pre_tool_use",
      "tool_pattern": "write_*",
      "command": "npm test",
      "timeout_ms": 120000,
      "on_failure": "block"
    }
  ]
}
```

**Format Code After Edits:**
```json
{
  "hooks": [
    {
      "name": "format-after-edit",
      "trigger": "post_tool_use",
      "tool_pattern": "edit_*",
      "command": "prettier --write .",
      "on_failure": "warn"
    }
  ]
}
```

**Log All Tool Executions:**
```json
{
  "hooks": [
    {
      "name": "log-tools",
      "trigger": "post_tool_use",
      "command": "echo \"Tool: $MIRA_TOOL_NAME, Success: $MIRA_TOOL_SUCCESS\" >> /tmp/mira.log",
      "on_failure": "ignore"
    }
  ]
}
```

---

## Checkpoint and Rewind

Snapshot file state before modifications for easy rollback.

### Overview

Mira automatically creates checkpoints before modifying files. If something goes wrong, you can rewind to a previous state.

### How It Works

1. **Automatic Snapshots**: Before any file-modifying tool (`write_file`, `edit_file`, `delete_file`, etc.), Mira snapshots the file's current content
2. **Session-Scoped**: Checkpoints are tied to your session
3. **SHA-256 Hashing**: File content is hashed for integrity verification

### Using Checkpoints

**List Checkpoints:**
```
/checkpoints
```

Output:
```
**Checkpoints (most recent first):**

1. `a1b2c3d4` - 14:32:15 (2 files) - Before write_file
2. `e5f6g7h8` - 14:30:42 (1 files) - Before edit_file
3. `i9j0k1l2` - 14:28:10 (3 files) - Before delete_file

Use `/rewind <checkpoint-id>` to restore to a checkpoint.
```

**Restore a Checkpoint:**
```
/rewind a1b2c3d4
```

You can use partial IDs - Mira will match the prefix:
```
/rewind a1b2    # Matches a1b2c3d4
```

### What Gets Restored

When you rewind:
- **Existing files**: Content restored to checkpoint state
- **New files**: Deleted (they didn't exist at checkpoint)
- **Deleted files**: Recreated with original content

### Checkpoint Details

Each checkpoint records:
- Session ID
- Operation ID (which operation triggered it)
- Tool name (e.g., `write_file`)
- File paths and content
- Timestamp
- SHA-256 hash of content

### Automatic Cleanup

Old checkpoints are automatically cleaned up to prevent storage bloat. Recent checkpoints are preserved per session.

---

## MCP Support

Connect external tools via Model Context Protocol (MCP).

### Overview

MCP (Model Context Protocol) is a standard protocol for tool integration. Mira can connect to MCP servers that provide additional tools, extending its capabilities.

### Configuration

Create `.mira/mcp.json` (project) or `~/.mira/mcp.json` (user):

```json
{
  "servers": [
    {
      "name": "filesystem",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem"],
      "env": {
        "ALLOWED_PATHS": "/home/user/projects"
      }
    }
  ]
}
```

### Server Configuration

| Property | Required | Description |
|----------|----------|-------------|
| `name` | Yes | Unique server identifier |
| `command` | Yes* | Command to spawn server |
| `args` | No | Command arguments |
| `url` | Yes* | HTTP URL for remote servers |
| `env` | No | Environment variables |
| `timeout_ms` | No | Request timeout (default: 30000) |

*Either `command` or `url` required.

### Transport Types

**Stdio (Spawned Process):**
```json
{
  "name": "local-server",
  "command": "node",
  "args": ["./mcp-server.js"]
}
```

**HTTP (Remote Server):**
```json
{
  "name": "remote-server",
  "url": "http://localhost:3002/mcp"
}
```

### Viewing MCP Status

Use `/mcp` to see connected servers and available tools:

```
/mcp
```

Output:
```
**MCP Servers (2 connected):**

**filesystem** (5 tools)
  - `read_file`: Read file contents
  - `write_file`: Write to a file
  - `list_directory`: List directory contents
  - `create_directory`: Create a directory
  - `delete_file`: Delete a file

**github** (3 tools)
  - `search_repos`: Search GitHub repositories
  - `get_issues`: Get repository issues
  - `create_pr`: Create a pull request
```

### Using MCP Tools

MCP tools are automatically available to Mira. They appear with the naming convention `mcp__<server>__<tool>`:

- `mcp__filesystem__read_file`
- `mcp__github__search_repos`

Mira will use these tools when appropriate for your requests.

### Popular MCP Servers

| Server | Purpose |
|--------|---------|
| `@anthropic/mcp-server-filesystem` | File system operations |
| `@anthropic/mcp-server-github` | GitHub integration |
| `@anthropic/mcp-server-sqlite` | SQLite database access |
| `@anthropic/mcp-server-brave` | Web search via Brave |

### Example Configurations

**Filesystem Access:**
```json
{
  "servers": [
    {
      "name": "filesystem",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem"],
      "env": {
        "ALLOWED_PATHS": "/home/user/projects,/tmp"
      }
    }
  ]
}
```

**GitHub Integration:**
```json
{
  "servers": [
    {
      "name": "github",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-github"],
      "env": {
        "GITHUB_TOKEN": "ghp_your_token_here"
      }
    }
  ]
}
```

**Multiple Servers:**
```json
{
  "servers": [
    {
      "name": "filesystem",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-filesystem"]
    },
    {
      "name": "github",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-github"],
      "env": {
        "GITHUB_TOKEN": "ghp_xxx"
      }
    },
    {
      "name": "search",
      "command": "npx",
      "args": ["-y", "@anthropic/mcp-server-brave"],
      "env": {
        "BRAVE_API_KEY": "xxx"
      }
    }
  ]
}
```

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
# Google API key (required)
GOOGLE_API_KEY=your-google-api-key

# Model settings
GEMINI_MODEL=gemini-3-pro-preview
GEMINI_THINKING_LEVEL=high  # low, high
GEMINI_EMBEDDING_MODEL=gemini-embedding-001
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

### Thinking Levels

Gemini 3 Pro supports variable thinking levels:

| Level | Use Case | Cost |
|-------|----------|------|
| `low` | Simple queries, quick answers | Lower |
| `high` | Complex planning, architecture | Higher |

Mira automatically selects thinking level based on task complexity.

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
Error: GOOGLE_API_KEY not set
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

*Last updated: 2025-12-05*
