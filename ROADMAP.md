# ROADMAP.md

**Mira: Next-Generation AI Coding Assistant**

Vision document for transforming Mira into a web-based development environment that combines conversational AI with powerful code manipulation capabilities.

---

## Vision Statement

Mira will become the ultimate AI-powered development companion by combining:

- **Conversational Intelligence** - Natural dialogue about any topic, like claude.ai
- **Code Manipulation Power** - Direct file/terminal access and tool execution, like Claude Code CLI
- **Enhanced Context Awareness** - Leveraging Mira's superior memory systems (Qdrant embeddings + SQLite) for deeper code understanding
- **Visual Feedback** - Split-view web interface where users chat naturally on the left while watching Mira work in real-time on the right

**Core Philosophy**: Enable developers to have natural conversations with an AI that can truly understand and manipulate their codebase, accessible from anywhere via web browser.

---

## Current State Analysis

### What Mira Has Today

**Strengths:**
- Dual LLM orchestration (GPT-5 for reasoning, DeepSeek for code generation)
- Hybrid memory system (SQLite + Qdrant with 5-head embeddings)
- Code intelligence service (function/class extraction, semantic analysis)
- Git integration (clone, import, sync, branch management)
- Real-time WebSocket streaming architecture
- Artifact system (code blocks from LLM saved/applied to files)
- React + TypeScript frontend with Monaco editor
- Rust backend with proven performance

**Capabilities:**
- Intelligent conversation with context retention
- Code generation with delegation to specialized models
- Semantic search across conversations and code
- Project-aware responses with file tree context
- Multi-head embeddings for different content types

### What's Missing for the Vision

**Critical Gaps:**
1. **Remote Machine Connectivity** - No SSH connection to VPS/dev machines
2. **File Operations UI** - No visual file browser or direct file access
3. **Terminal Integration** - No embedded terminal for command execution
4. **Tool Execution** - LLM cannot directly read/write files or run commands
5. **Split-View Interface** - Current UI doesn't show real-time work visualization
6. **Direct Manipulation** - All changes must be manually copied from artifacts

**The Problem**: Mira can generate great code and have intelligent conversations, but cannot autonomously manipulate the actual development environment like Claude Code can.

---

## Target State

### User Experience

**Split-View Interface:**
```
┌─────────────────────────────────────────────────────┐
│  Chat (Left Panel)        │  Terminal/Files (Right) │
│  ─────────────────────    │  ──────────────────────│
│  User: "Add user auth"    │  $ ssh user@vps        │
│                            │  Connected to dev-box  │
│  Mira: "I'll implement    │                         │
│  JWT authentication.       │  📁 src/               │
│  First, I'll create the   │    📁 auth/            │
│  auth service..."          │      📄 jwt.ts         │
│                            │      📄 middleware.ts  │
│  [Streaming response...]   │                         │
│                            │  $ npm install jsonwebtoken
│  [Mira is working...]      │  + jsonwebtoken@9.0.0  │
│                            │                         │
│  Mira: "Done! I've added  │  $ git status          │
│  JWT auth with middleware │  modified: src/auth/   │
│  and tests."               │  new: tests/auth/      │
└─────────────────────────────────────────────────────┘
```

**Key Features:**
- User chats naturally on the left
- Real-time terminal/file activity shown on the right
- Visual feedback of every action Mira takes
- File tree browser shows current project structure
- Terminal displays command execution and output
- Seamless integration between conversation and action

### Core Capabilities

**1. Direct File Operations**
- Read files from remote machine
- Create new files with content
- Edit existing files (search/replace, line edits)
- Delete files/directories
- All changes visible in real-time file tree

**2. Terminal Command Execution**
- Run bash commands on remote machine
- Execute build tools (npm, cargo, make, etc.)
- Run tests and see results
- Install dependencies
- Real-time streaming of command output

**3. Git Operations**
- Clone repositories
- Create branches
- Commit changes with descriptive messages
- Push to remote
- View diffs and status
- All git operations shown in terminal

**4. Code Search and Navigation**
- Search codebase with grep/ripgrep
- Find file patterns with glob
- Navigate to definitions
- Analyze project structure
- All powered by Mira's code intelligence

**5. Enhanced Context Awareness**
- Leverage Qdrant embeddings for semantic code understanding
- Use SQLite for structured code metadata
- Code intelligence integration for smarter suggestions
- Memory of previous operations and patterns
- Context-aware tool selection

### Technical Architecture

**Frontend Enhancements:**
```
React Frontend (New Components)
├── SplitViewLayout - Main container
│   ├── ChatPanel (Left) - Existing chat interface
│   └── WorkspacePanel (Right) - New
│       ├── TerminalEmulator (xterm.js) - Live terminal
│       ├── FileTreeBrowser - Remote file explorer
│       ├── FileViewer (Monaco) - Quick file preview
│       └── ActivityFeed - Real-time action log
```

**Backend Enhancements:**
```
Rust Backend (New Services)
├── SSH Connection Manager
│   ├── Connection pooling
│   ├── Session management
│   └── Authentication (key-based)
├── File System Service
│   ├── Remote file operations
│   ├── Path resolution
│   └── Permission checking
├── Command Execution Service
│   ├── Shell command runner
│   ├── Output streaming
│   └── Process management
├── Tool Registry (Enhanced)
│   ├── File read/write/edit tools
│   ├── Command execution tools
│   ├── Git operation tools
│   └── Code search tools
└── Context Enhancement
    ├── Code intelligence integration
    ├── Semantic search for relevant context
    └── Memory-aware tool suggestions
```

**Data Flow:**
```
User Chat → GPT-5 (Reasoning) → Tool Calls → Backend Services
                                      ↓
                        ┌─────────────┴──────────────┐
                        │                            │
                   SSH Service              Code Intelligence
                        │                            │
                   Remote Machine              Qdrant + SQLite
                        │                            │
                   Execute → Stream Output → Frontend Terminal
                        │                            │
                   Update Files → Sync → File Tree Browser
```

---

## Phased Implementation Plan

### Phase 1: Foundation (Months 1-2)

**Goal**: Establish remote connectivity and basic file operations

**Deliverables:**
1. SSH Connection Management
   - WebSocket-based SSH client integration
   - Connection persistence and reconnection
   - Authentication with SSH keys
   - Session state management

2. File System API
   - Read file contents from remote machine
   - List directory contents
   - Basic file metadata (size, modified date, permissions)
   - Path normalization and validation

3. Terminal UI Component
   - Integrate xterm.js terminal emulator
   - WebSocket bridge for terminal I/O
   - Basic command execution
   - Output streaming to frontend

4. Split-View Layout
   - Responsive split-pane layout
   - Resizable panels
   - Chat on left, terminal on right
   - Basic file tree viewer

**Success Criteria:**
- User can connect to remote VPS via Mira
- Basic terminal commands work (ls, cat, pwd, etc.)
- File tree shows remote directory structure
- Split view is functional and responsive

### Phase 2: Tool Execution (Months 3-4)

**Goal**: Enable LLM to autonomously perform file and command operations

**Deliverables:**
1. File Operation Tools
   - `read_file` - Read contents of remote file
   - `write_file` - Create/overwrite file with content
   - `edit_file` - Search/replace edits to existing file
   - `list_files` - Directory listing with patterns
   - `delete_file` - Remove files/directories

2. Command Execution Tools
   - `execute_command` - Run shell commands
   - `run_script` - Execute multi-line scripts
   - `install_package` - Package manager operations
   - `run_tests` - Test runner execution

3. Git Operation Tools
   - `git_status` - Working tree status
   - `git_diff` - View changes
   - `git_commit` - Create commits
   - `git_push` - Push to remote
   - `git_branch` - Branch operations

4. Real-Time Feedback
   - Stream command output to terminal
   - Update file tree on file changes
   - Show git status in UI
   - Activity log of LLM actions

**Success Criteria:**
- GPT-5 can call tools to manipulate remote files
- User sees real-time terminal output
- File tree updates automatically
- Git operations work end-to-end

### Phase 3: Intelligence Enhancement (Months 5-6)

**Goal**: Leverage Mira's memory and code intelligence for superior performance

**Deliverables:**
1. Code Search Integration
   - `search_code` - Semantic code search via Qdrant
   - `find_definition` - Locate function/class definitions
   - `grep_files` - Pattern matching across codebase
   - `analyze_structure` - Project structure analysis

2. Context-Aware Tools
   - Pre-load relevant code context before operations
   - Use embeddings to find related code sections
   - Memory of previous edits for consistency
   - Suggest related files based on current task

3. Enhanced Code Intelligence
   - Parse code structure on file changes
   - Update embeddings in real-time
   - Track dependencies and imports
   - Detect code patterns and anti-patterns

4. Smart Suggestions
   - Proactive error detection
   - Test coverage suggestions
   - Refactoring opportunities
   - Security vulnerability warnings

**Success Criteria:**
- LLM uses code intelligence for better decisions
- Context includes semantically relevant code
- Mira suggests improvements proactively
- Operations are faster and more accurate

### Phase 4: Polish & Scale (Months 7-8)

**Goal**: Production-ready features and multi-machine support

**Deliverables:**
1. Multi-Machine Support
   - Connect to multiple remote machines
   - Switch between machines in UI
   - Per-machine session persistence
   - Cross-machine file operations

2. Advanced Features
   - File content search and preview
   - Integrated diff viewer
   - Collaborative editing
   - Operation history and replay

3. Performance Optimization
   - Connection pooling
   - Caching of file metadata
   - Incremental file tree updates
   - Optimized streaming protocols

4. Security Hardening
   - Command sandboxing
   - Permission verification
   - Audit logging
   - Rate limiting

**Success Criteria:**
- Support 3+ simultaneous connections
- Sub-100ms operation latency
- Zero security vulnerabilities
- Production-ready deployment

---

## Technical Considerations

### Security

**SSH Authentication:**
- Use SSH key pairs (never passwords)
- Store private keys securely (encrypted at rest)
- Per-user key management
- Key rotation policies

**Command Sandboxing:**
- Whitelist allowed commands in production
- Restrict dangerous operations (rm -rf /, etc.)
- Filesystem access controls
- Resource limits (CPU, memory, network)

**Data Protection:**
- Encrypt all WebSocket traffic (WSS)
- No logging of sensitive data
- Session isolation between users
- Secure credential storage

### Performance

**Connection Management:**
- Connection pooling for SSH sessions
- Automatic reconnection on disconnect
- Heartbeat/keepalive mechanisms
- Graceful degradation on network issues

**Streaming Optimization:**
- Binary WebSocket frames for terminal data
- Chunked file transfers for large files
- Incremental file tree updates
- Debounced UI updates

**Caching Strategy:**
- Cache file metadata locally
- Cache directory listings
- Cache code intelligence results
- Invalidation on file changes

### Scalability

**Single Machine (Phase 1-3):**
- One SSH connection per user session
- Local state management
- Direct WebSocket communication

**Multi-Machine (Phase 4):**
- Connection multiplexing
- Shared state across machines
- Cross-machine context awareness
- Distributed file search

### Technology Choices

**Terminal Emulator:**
- **xterm.js** - Industry standard, full VT100 emulation, WebGL rendering
- Alternatives: term.js, hterm

**SSH Client:**
- **Backend**: ssh2 (if Node.js) or russh (if Rust)
- WebSocket bridge for browser connectivity
- Consider WebRTC for lower latency (future)

**File Tree:**
- **react-complex-tree** or custom implementation
- Lazy loading for large directories
- Virtual scrolling for performance

**WebSocket Protocol:**
- JSON for command/control messages
- Binary for terminal data streams
- Multiplexed channels for different data types

---

## Enhanced by Mira's Unique Strengths

### Superior Memory System

**How It Helps:**
- Remember patterns from previous coding sessions
- Recall similar problems and their solutions
- Learn user's coding style and preferences
- Maintain context across long development sessions

**Integration:**
- Store tool execution history in SQLite
- Embed successful code patterns in Qdrant
- Semantic search for "how did I solve X before?"
- Relationship engine tracks user preferences

### Multi-Head Embeddings

**Code Head:**
- Embed every file as it's edited
- Semantic search for related code
- Find similar implementations
- Detect code duplication

**Document Head:**
- Embed README, docs, comments
- Understand project architecture
- Find relevant documentation
- Suggest documentation updates

**Relationship Head:**
- Learn user's workflow patterns
- Adapt to coding style
- Remember project conventions
- Personalized suggestions

### Code Intelligence

**Function/Class Extraction:**
- Parse code structure on every edit
- Track dependencies automatically
- Update embeddings incrementally
- Fast symbol search

**Project Understanding:**
- Build comprehensive project graph
- Understand module relationships
- Detect architectural patterns
- Suggest refactoring opportunities

### Dual LLM Architecture

**GPT-5 (Reasoning):**
- Understands user intent
- Plans multi-step operations
- Decides which tools to use
- Handles errors and retries

**DeepSeek (Code Generation):**
- Generates high-quality code
- Follows established patterns
- Optimized for code synthesis
- Fast and cost-effective

---

## Success Metrics

### User Experience Metrics

**Conversational Quality:**
- User can discuss any topic naturally (like claude.ai)
- Mira understands context across long conversations
- Natural language commands work reliably

**Tool Execution Quality:**
- All file operations work correctly
- Command execution is reliable
- Git operations succeed consistently
- Real-time feedback is accurate

**Visual Feedback:**
- User sees every action Mira takes
- Terminal output is live and accurate
- File tree updates in real-time
- No hidden or mysterious operations

### Technical Metrics

**Performance:**
- File read latency < 200ms
- Command execution starts < 100ms
- Terminal rendering at 60 FPS
- File tree loads < 500ms for 1000 files

**Reliability:**
- SSH connection uptime > 99%
- Tool execution success rate > 95%
- Zero data loss on operations
- Graceful error recovery

**Intelligence:**
- Context relevance score > 0.8
- Code search precision > 90%
- Suggested operations accuracy > 80%
- Memory recall accuracy > 85%

### Business Metrics

**Adoption:**
- Daily active users
- Session duration
- Operations per session
- User retention rate

**Value Delivered:**
- Code written per session
- Problems solved per day
- Time saved vs manual coding
- User satisfaction score

---

## Competitive Advantages

### vs Claude Code CLI

**Mira's Advantages:**
- Web-based (accessible from anywhere)
- Superior memory and code intelligence
- Multi-head semantic understanding
- Visual feedback in browser
- Multi-machine support (future)

**Claude Code's Advantages:**
- Native OS integration
- Local file access
- Lower latency
- Offline capable

**Strategy**: Focus on web accessibility and enhanced intelligence to differentiate.

### vs claude.ai

**Mira's Advantages:**
- Direct code manipulation
- Tool execution capabilities
- Project-aware context
- Real-time development environment
- Code intelligence integration

**claude.ai's Advantages:**
- Simpler interface
- Broader use cases
- No setup required
- Faster response (no tool execution)

**Strategy**: Position as "claude.ai for developers" - same conversational quality plus coding superpowers.

### vs GitHub Copilot

**Mira's Advantages:**
- Full project understanding
- Multi-file operations
- Terminal command execution
- Conversational interface
- Memory of past sessions

**Copilot's Advantages:**
- IDE integration
- Line-by-line suggestions
- Extremely fast
- Works offline

**Strategy**: Complement rather than replace Copilot - use Mira for high-level tasks, Copilot for autocomplete.

---

## Open Questions & Decisions Needed

### Technical Decisions

1. **Terminal Protocol**: WebSocket vs WebRTC for terminal streaming?
   - WebSocket: Simpler, proven, good enough latency
   - WebRTC: Lower latency, peer-to-peer, more complex

2. **SSH Backend**: Rust (russh) vs Node.js (ssh2)?
   - Rust: Better performance, type safety, consistent with backend
   - Node.js: More mature libraries, easier to debug

3. **File Operations**: Stream vs bulk transfer for large files?
   - Stream: Better for huge files, more complex
   - Bulk: Simpler, good enough for most files

4. **State Management**: Where to maintain file tree state?
   - Backend: Source of truth, requires sync
   - Frontend: Faster UI, requires invalidation strategy
   - Hybrid: Best of both, more complexity

### Product Decisions

1. **Pricing Model**: How to monetize?
   - Usage-based (per operation)
   - Subscription tiers
   - Free for open source, paid for private

2. **Target Users**: Who are we building for?
   - Professional developers
   - Hobbyists and learners
   - Teams and organizations

3. **Deployment**: How do users run Mira?
   - Cloud-hosted SaaS
   - Self-hosted on VPS
   - Hybrid (frontend SaaS, backend self-hosted)

4. **Access Control**: How to handle permissions?
   - Trust LLM with full access
   - User approval for dangerous operations
   - Configurable permission levels

---

## Next Steps

### Immediate Actions (Next 2 Weeks)

1. **Prototype Terminal Integration**
   - Spike: xterm.js in Mira frontend
   - Spike: WebSocket terminal bridge
   - Prove basic concept works

2. **Design SSH Architecture**
   - Choose Rust vs Node.js for SSH
   - Design connection management
   - Plan authentication flow

3. **Define Tool Schema**
   - List all required tools (file, command, git, search)
   - Design tool parameter schemas
   - Plan tool execution pipeline

4. **UI Mockups**
   - Design split-view layout
   - Design file tree browser
   - Design activity feed
   - User flow diagrams

### Research & Validation

1. **User Interviews**
   - Talk to potential users
   - Validate problem/solution fit
   - Understand workflow needs

2. **Technical Spikes**
   - SSH performance testing
   - Terminal rendering performance
   - File tree scalability
   - WebSocket vs WebRTC benchmarks

3. **Security Review**
   - Threat modeling
   - Security best practices
   - Compliance requirements

---

## Conclusion

Mira has the potential to become the definitive AI coding assistant by combining the best of conversational AI (claude.ai) with the power of direct code manipulation (Claude Code), all enhanced by our superior memory and code intelligence systems.

The roadmap is ambitious but achievable with phased execution over 8 months. Each phase delivers incremental value while building toward the complete vision.

**The North Star**: A developer opens Mira in their browser, chats naturally about their coding problem, and watches as Mira autonomously implements the solution with intelligence, transparency, and precision.

Let's build the future of AI-assisted development.

---

**Last Updated**: 2025-11-15
**Status**: Vision Document (Pre-Implementation)
**Owner**: Peter (Founder)
