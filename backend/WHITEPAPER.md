# Mira Backend: LLM Architecture Whitepaper

**Audience:** LLM model authors, backend integrators, and AI system architects  
**Purpose:** Complete technical reference for Mira's Rust-based AI coding assistant backend  
**Version:** 1.1 (Post-Operation Engine Refactor)

---

## Executive Summary

Mira is a sophisticated AI coding assistant backend that orchestrates two specialized LLMs in a complementary architecture:
- **GPT-5** (Responses API): Conversation, analysis, planning, and high-level reasoning
- **DeepSeek Reasoner**: Focused code generation and technical implementation

The system features real-time WebSocket streaming, comprehensive memory management with both structured (SQLite) and vector (Qdrant) storage, relationship tracking, and intelligent context gathering. All components are built in Rust for performance, type safety, and reliability.

**Key Innovation:** Rather than treating LLMs as interchangeable utilities, Mira delegates work based on task suitability. GPT-5 handles the "thinking" and DeepSeek handles the "coding" - each playing to their strengths.

**Version 1.1 Updates:**
- **Modular Operation Engine**: Refactored from monolithic `engine.rs` into focused modules (`orchestration`, `lifecycle`, `artifacts`, `delegation`, `context`, `events`)
- **Enhanced Error Handling**: All operation failures now properly emit `Failed` events through error handling wrapper
- **Comprehensive Database Schema**: Expanded `operations` and `artifacts` tables with extensive metadata fields for future expansion
- **Improved Constructor**: OperationEngine now builds all sub-components internally for cleaner instantiation

---

## Table of Contents

1. [System Architecture](#1-system-architecture)
2. [LLM Orchestration Strategy](#2-llm-orchestration-strategy)
3. [Operation Engine](#3-operation-engine)
4. [Memory Systems](#4-memory-systems)
5. [Context Gathering Pipeline](#5-context-gathering-pipeline)
6. [Provider Implementations](#6-provider-implementations)
7. [WebSocket Protocol](#7-websocket-protocol)
8. [Data Flow Examples](#8-data-flow-examples)
9. [Database Schema](#9-database-schema)
10. [Configuration & Deployment](#10-configuration--deployment)

---

## 1. System Architecture

### 1.1 Component Overview

```
┌────────────────────────────────────────────────────────────┐
│                      WebSocket Layer                            │
│  (Real-time bidirectional communication with frontend)          │
└────────────────────┬───────────────────────────────────────┘
                     │
┌────────────────────▼───────────────────────────────────────┐
│                   Unified Chat Handler                          │
│  - Request parsing & validation                                 │
│  - Context gathering (memory + code + relationships)            │
│  - Prompt building                                              │
│  - Response streaming & artifact handling                       │
└────────────────────┬───────────────────────────────────────┘
                     │
         ┌───────────┴────────────┐
         │                        │
┌────────▼────────┐      ┌───────▼────────┐
│  Operation      │      │  Simple Chat   │
│  Engine         │      │  (Direct GPT-5)│
│  (GPT-5 →       │      └────────────────┘
│   DeepSeek)     │
└────────┬────────┘
         │
    ┌────┴───────────────────┐
    │                          │
┌───▼──────────          ┌────────▼─────────┐
│  GPT-5   │          │  DeepSeek       │
│ Provider │          │  Provider       │
│          │          │                 │
│ • Stream │          │ • Code gen      │
│ • Tools  │          │ • Refactor      │
│ • Anal.  │          │ • Debug         │
└───┬──────┘          └─────────┬───────┘
    │                          │
    └─────────┬────────────────┘
               │
┌──────────────▼──────────────────────────────────────────────┐
│                    Storage Layer                                │
│                                                                  │
│  ┌──────────────┐  ┌─────────────┐  ┌─────────────────────┐   │
│  │   SQLite     │  │   Qdrant    │  │   Git Integration   │   │
│  │              │  │             │  │                     │   │
│  │ • Operations │  │ • Embeddings│  │ • File trees        │   │
│  │ • Messages   │  │ • Semantic  │  │ • Code context      │   │
│  │ • Analysis   │  │   search    │  │ • Project structure │   │
│  │ • Artifacts  │  │ • Multi-head│  │                     │   │
│  │ • Relations  │  │   routing   │  │                     │   │
│  └──────────────┘  └─────────────┘  └─────────────────────┘   │
└──────────────────────────────────────────────────────────────┘
```

### 1.2 Core Design Principles

**1. Separation of Concerns**
- Each module has a single, well-defined responsibility
- Clean interfaces between layers
- No circular dependencies

**2. Type Safety First**
- Rust's type system prevents entire classes of bugs
- Explicit error handling with `Result<T, E>`
- Strong typing for database operations via sqlx

**3. Real-time Streaming**
- WebSocket-based bidirectional communication
- Server-Sent Events (SSE) for LLM streaming
- Cancellation token support for graceful interruption

**4. Dual Storage Strategy**
- SQLite for structured data (operations, messages, analysis)
- Qdrant for semantic search (embeddings across multiple "heads")

**5. Context-Aware Responses**
- Memory recall (recent + semantic hybrid search)
- Code intelligence (function/class-level understanding)
- Relationship tracking (user preferences, patterns, facts)
- Rolling summaries (10-message and 100-message windows)

---

## 2. LLM Orchestration Strategy

### 2.1 The Delegation Model

Mira uses **capability-based delegation** rather than treating LLMs as interchangeable:

```rust
┌──────────────────────────────────────────────────────┐
│  User Request: "Add error handling to auth.rs"          │
└────────────────────┬─────────────────────────────────┘
                     │
              ┌──────▼──────┐
              │   GPT-5     │
              │             │
              │  Analyzes:  │
              │  • Intent   │
              │  • Context  │
              │  • Plan     │
              └──────┬──────┘
                     │
                     │ [Tool Call: refactor_code]
                     │
              ┌──────▼──────────┐
              │   DeepSeek      │
              │                 │
              │  Generates:     │
              │  • Actual code  │
              │  • Tests        │
              │  • Docs         │
              └──────┬──────────┘
                     │
              ┌──────▼──────────┐
              │  create_artifact│
              │                 │
              │  Produces:      │
              │  • File path    │
              │  • Full code    │
              │  • Metadata     │
              └─────────────────┘
```

### 2.2 Tool-Based Orchestration

GPT-5 receives three delegation tools:
1. `generate_code` - Create new files from scratch
2. `refactor_code` - Modify existing code
3. `debug_code` - Fix errors and bugs

**PLUS** one immediate tool:
- `create_artifact` - Handled by GPT-5 itself for direct artifact creation

When GPT-5 calls a delegation tool, the Operation Engine:
1. Pauses GPT-5 streaming
2. Sends the tool call to DeepSeek with full context
3. Streams DeepSeek's response back to frontend
4. Captures artifacts produced
5. Continues or completes the operation

### 2.3 Why This Architecture?

**GPT-5 Strengths:**
- Natural conversation
- Task analysis and planning
- Error interpretation
- Architectural decisions
- Multi-turn context management via Responses API

**DeepSeek Strengths:**
- Code generation quality
- Technical implementation details
- Performance optimization
- Deep reasoning for complex algorithms

**Result:** Each model does what it does best, with clean handoffs between them.

---

## 3. Operation Engine

The Operation Engine orchestrates complex coding workflows. It's the heart of Mira's coding capabilities.

### 3.1 Modular Architecture

The engine has been refactored into focused modules for maintainability:

```rust
pub struct OperationEngine {
    lifecycle_manager: LifecycleManager,
    artifact_manager: ArtifactManager,
    orchestrator: Orchestrator,
}
```

**Sub-Components:**
- **`orchestration.rs`** - Main `run_operation()` logic with error handling
- **`lifecycle.rs`** - Status transitions and operation CRUD
- **`artifacts.rs`** - Artifact creation and retrieval
- **`delegation.rs`** - DeepSeek delegation with context
- **`context.rs`** - Context gathering (memory, code, relationships)
- **`events.rs`** - Event types emitted during operations

### 3.2 Operation Lifecycle

```
PENDING → STARTED → DELEGATING → GENERATING → COMPLETED
   ↓          ↓           ↓            ↓           ↓
FAILED    FAILED      FAILED       FAILED      [SUCCESS]
```

**Lifecycle Manager** handles state transitions:
- Creates operations with `create_operation()`
- Updates status with `update_status()`
- Marks completion with `complete_operation()`
- Records failures with `fail_operation()`
- Emits events for each transition

### 3.3 Key Methods

**`OperationEngine::new()`**
```rust
pub fn new(
    db: Arc<SqlitePool>,
    gpt5: Gpt5Provider,
    deepseek: DeepSeekProvider,
    memory_service: Arc<MemoryService>,
    relationship_service: Arc<RelationshipService>,
    git_client: GitClient,
    code_intelligence: Arc<CodeIntelligenceService>,
) -> Self
```

Constructor builds all sub-components internally.

**`create_operation()`**
```rust
pub async fn create_operation(
    &self,
    session_id: String,
    kind: String,
    user_message: String,
) -> Result<Operation>
```

Delegates to `LifecycleManager` to create a new operation record.

**`run_operation()`**
```rust
pub async fn run_operation(
    &self,
    operation_id: &str,
    session_id: &str,
    user_content: &str,
    project_id: Option<&str>,
    cancel_token: Option<CancellationToken>,
    event_tx: &mpsc::Sender<OperationEngineEvent>,
) -> Result<()>
```

Main entry point - delegates to `Orchestrator` which:
1. Stores user message in memory
2. Loads memory context via `ContextBuilder`
3. Loads file tree and code intelligence
4. Builds system prompt with full context
5. Streams GPT-5 response with delegation tools
6. Handles tool calls by delegating to `DelegationHandler`
7. Captures artifacts via `ArtifactManager`
8. Updates status via `LifecycleManager`
9. Emits events throughout the process

**Error Handling Wrapper:**
The orchestrator wraps all logic to ensure that ANY error (cancellation, API failures, etc.) properly emits a `Failed` event before propagating the error.

### 3.4 Tool Call Processing

When GPT-5 invokes a delegation tool, the **DelegationHandler** processes it:

```rust
pub async fn delegate_to_deepseek(
    &self,
    tool_name: &str,
    tool_args: Value,
    cancel_token: Option<CancellationToken>,
    file_tree: Option<&Vec<FileNode>>,
    code_context: Option<&Vec<MemoryEntry>>,
    recall_context: &RecallContext,
) -> Result<Value>
```

**Process:**
1. Parse tool name and arguments
2. Build rich prompt with:
   - Tool arguments (file path, description, requirements)
   - File tree context (project structure)
   - Code intelligence (relevant functions/classes)
   - Memory context (past conversations)
3. Stream DeepSeek response
4. Parse artifacts from response using regex
5. Return structured result

**Supported Tools:**
- `generate_code` - Create new files from scratch
- `refactor_code` - Modify existing code
- `debug_code` - Fix errors and bugs
- `create_artifact` - Handled by GPT-5 directly (no delegation)

All delegation results are captured by the **ArtifactManager** which stores them in the database and emits events to the frontend.
```

### 3.5 State Management

Operations maintain state through `OperationStatus` managed by the `LifecycleManager`:
- `pending` - Just created, not yet started
- `started` - Operation execution began
- `delegating` - Waiting for DeepSeek to complete tool call
- `generating` - DeepSeek producing code
- `completed` - Success with artifacts
- `failed` - Terminal error state

All state transitions are:
- Persisted to SQLite `operations` table with timestamps
- Logged in `operation_events` table with sequence numbers
- Emitted as `OperationEngineEvent` via event channel
- Sent to frontend via WebSocket

### 3.6 Artifact Creation

The **ArtifactManager** handles all artifact operations:

```rust
pub struct Artifact {
    pub id: String,
    pub operation_id: String,
    pub kind: String,                   // "code", "document", "diagram"
    pub file_path: Option<String>,
    pub content: String,
    pub content_hash: String,           // SHA-256 for deduplication
    pub language: Option<String>,
    
    #[sqlx(rename = "diff_from_previous")]
    pub diff: Option<String>,
    
    pub created_at: i64,
}
```

**Note:** The database schema includes extensive additional fields for future expansion:
- `preview` - First N lines for quick display
- `previous_artifact_id` - Links to prior version
- `is_new_file` - Whether this creates a new file
- `related_files`, `dependencies` - JSON context arrays
- `generated_by`, `generation_time_ms` - Generation metadata
- `context_tokens`, `output_tokens` - Token usage
- `completed_at`, `applied_at` - Lifecycle timestamps

The Rust struct exposes only the core fields currently needed. See `migrations/20251016_unified_baseline.sql` for complete schema.

**Artifact Lifecycle:**
1. DeepSeek generates code with `<artifact>` tags
2. Regex parser extracts path, content, metadata
3. ArtifactManager computes SHA-256 hash
4. Stored in `artifacts` table linked to operation
5. `ArtifactCompleted` event emitted
6. Frontend displays in Artifact Viewer
7. User can save to disk via file write API

---

## 4. Memory Systems

Mira uses a hybrid memory architecture combining structured storage (SQLite) and semantic search (Qdrant).

### 4.1 Memory Types

**Episodic Memory** - Individual messages and their context  
**Semantic Memory** - Embeddings for similarity search  
**Relationship Memory** - User preferences, facts, patterns  
**Rolling Summaries** - Compressed conversation context (10 & 100 message windows)

### 4.2 MemoryService Architecture

```rust
pub struct MemoryService {
    db: SqlitePool,
    embedding_service: Arc<EmbeddingService>,
    recall_engine: Arc<RecallEngine>,
    storage_service: Arc<StorageService>,
    code_intel: Arc<CodeIntelligenceService>,
}
```

**Key Components:**
- `StorageService` - SQLite operations for messages, analysis, relationships
- `EmbeddingService` - OpenAI embeddings generation and Qdrant storage
- `RecallEngine` - Hybrid search combining recency + semantic similarity
- `CodeIntelligenceService` - File tree, function/class extraction

### 4.3 Message Storage Pipeline

```
User Message
    │
    ├─> Store in SQLite (messages table)
    │
    ├─> Analyze with GPT-4o-mini
    │   ├─> Extract: intent, entities, topics, salience
    │   └─> Store in message_analysis table
    │
    ├─> Generate Embedding (if salience >= threshold)
    │   ├─> Create 3072-dim vector via OpenAI
    │   └─> Store in Qdrant with multiple heads:
    │       ├─> general (always)
    │       ├─> code (if code detected)
    │       ├─> facts (if factual content)
    │       └─> preferences (if user preferences)
    │
    └─> Update Rolling Summaries
        ├─> 10-message window (recent context)
        └─> 100-message window (session context)
```

### 4.4 Recall Engine

The `RecallEngine` implements hybrid search:

```rust
pub async fn recall_relevant_context(
    &self,
    query: &str,
    conversation_id: Uuid,
    params: RecallParams,
) -> Result<Vec<MessageWithAnalysis>>
```

**Search Strategy:**
1. **Recent Messages** - Last N messages (chronological)
2. **Semantic Search** - Qdrant vector similarity
3. **Merge & Deduplicate** - Combine results
4. **Rank** - Score by relevance + recency

**Qdrant Multi-Head Routing:**
```rust
let heads = match query_analysis {
    Contains("code") => vec!["code", "general"],
    Contains("preference") => vec!["preferences", "general"],
    Contains("fact") => vec!["facts", "general"],
    _ => vec!["general"]
};
```

### 4.5 Relationship Tracking

Relationships store structured facts about the user:

```sql
CREATE TABLE relationships (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    subject TEXT NOT NULL,
    relation_type TEXT NOT NULL,  -- preference, fact, expertise, etc.
    object TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    metadata JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

**Examples:**
- `("user", "prefers", "Rust over Python")`
- `("user", "works_at", "Anthropic")`
- `("user", "expertise", "machine learning")`

Relationships are:
- Extracted during message analysis
- Queried during context gathering
- Used to personalize responses

### 4.6 Rolling Summaries

Summaries provide compressed context for LLM prompts:

**10-Message Summary:**
- Window: Last 10 messages
- Purpose: Immediate recent context
- Generation: After each message
- Storage: `conversation_summaries` table

**100-Message Summary:**
- Window: Last 100 messages  
- Purpose: Session-level context
- Generation: Every 10 messages
- Storage: Same table, different type

**Implementation:**
```rust
pub async fn generate_rolling_summary(
    &self,
    conversation_id: Uuid,
    window_size: i32,
) -> Result<String>
```

Summaries are included in system prompts to provide historical context without overwhelming the context window.

---

## 5. Context Gathering Pipeline

Before each LLM call, Mira assembles comprehensive context through the `UnifiedPromptBuilder`.

### 5.1 Context Sources

```rust
pub struct MessageContext {
    // Memory context
    pub rolling_summary_10: Option<String>,
    pub rolling_summary_100: Option<String>,
    pub recalled_messages: Vec<MessageWithAnalysis>,
    pub relationships: Vec<Relationship>,
    
    // Code context
    pub file_tree: Option<String>,
    pub relevant_code: Vec<CodeSnippet>,
    pub function_definitions: Vec<FunctionDef>,
    
    // Operation context
    pub recent_artifacts: Vec<Artifact>,
    pub error_logs: Option<String>,
}
```

### 5.2 Gathering Process

**Phase 1: Memory Recall**
```rust
let recalled = recall_engine.recall_relevant_context(
    &user_message,
    conversation_id,
    RecallParams {
        max_recent: 5,
        max_semantic: 10,
        min_similarity: 0.7,
    }
).await?;
```

**Phase 2: Code Intelligence**
```rust
if message_mentions_code() {
    let tree = code_intel.get_file_tree(repo_path).await?;
    let functions = code_intel.extract_functions(&relevant_files).await?;
}
```

**Phase 3: Relationship Loading**
```rust
let relationships = storage.get_relationships(user_id, limit).await?;
```

**Phase 4: Summary Retrieval**
```rust
let summary_10 = storage.get_latest_summary(
    conversation_id,
    SummaryType::Rolling10
).await?;

let summary_100 = storage.get_latest_summary(
    conversation_id,
    SummaryType::Rolling100
).await?;
```

### 5.3 Prompt Assembly

The `UnifiedPromptBuilder` combines all context sources:

```rust
pub fn build_system_prompt(&self, context: &MessageContext) -> String {
    let mut prompt = String::new();
    
    // 1. Base system instructions
    prompt.push_str(&self.base_instructions);
    
    // 2. Conversation context
    if let Some(summary) = &context.rolling_summary_100 {
        prompt.push_str("[SESSION CONTEXT]\n");
        prompt.push_str(summary);
    }
    
    // 3. Recent context
    if let Some(summary) = &context.rolling_summary_10 {
        prompt.push_str("[RECENT CONTEXT]\n");
        prompt.push_str(summary);
    }
    
    // 4. Recalled messages
    if !context.recalled_messages.is_empty() {
        prompt.push_str("[RELEVANT HISTORY]\n");
        for msg in &context.recalled_messages {
            prompt.push_str(&format!("{}: {}\n", msg.role, msg.content));
        }
    }
    
    // 5. Relationships
    if !context.relationships.is_empty() {
        prompt.push_str("[USER CONTEXT]\n");
        for rel in &context.relationships {
            prompt.push_str(&format!("- {}\n", rel.to_natural_language()));
        }
    }
    
    // 6. Code context
    if let Some(tree) = &context.file_tree {
        prompt.push_str("[PROJECT STRUCTURE]\n");
        prompt.push_str(tree);
    }
    
    // 7. Recent artifacts
    if !context.recent_artifacts.is_empty() {
        prompt.push_str("[RECENT WORK]\n");
        for artifact in &context.recent_artifacts {
            prompt.push_str(&format!("File: {}\n", artifact.path));
        }
    }
    
    prompt
}
```

### 5.4 Context Window Management

To prevent exceeding LLM context limits:

1. **Prioritize** - Most recent and relevant first
2. **Truncate** - Remove oldest recalled messages if needed
3. **Summarize** - Use rolling summaries instead of full history
4. **Compress** - Code snippets over full files

**Token Budget Allocation:**
- System prompt: ~2K tokens
- User message: ~1K tokens
- Context: ~8K tokens
- Response: ~4K tokens
- Total: ~15K tokens (well under GPT-5's 100K limit)

---

## 6. Provider Implementations

### 6.1 LlmProvider Trait

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn create(
        &self,
        messages: Vec<Message>,
        system: String,
        tools: Option<Vec<Value>>,
    ) -> Result<Response>;
    
    async fn stream(
        &self,
        messages: Vec<Message>,
        system: String,
        sender: UnboundedSender<StreamChunk>,
        cancel_token: CancellationToken,
    ) -> Result<()>;
    
    fn as_any(&self) -> &dyn Any;
}
```

### 6.2 OpenAI Provider (GPT-5 via Responses API)

**Key Features:**
- Multi-turn context via Response objects
- Streaming support with SSE
- Tool calling (JSON mode)
- Cancellation token support

**Implementation Highlights:**
```rust
pub struct OpenAiProvider {
    client: Client,
    model: String,
    api_key: String,
}

impl OpenAiProvider {
    pub async fn stream(
        &self,
        messages: Vec<Message>,
        system: String,
        sender: UnboundedSender<StreamChunk>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let response = self.client
            .post("https://api.openai.com/v1/responses")
            .json(&request_body)
            .send()
            .await?;
            
        let mut stream = response.bytes_stream();
        
        while let Some(chunk) = stream.next().await {
            if cancel_token.is_cancelled() {
                return Ok(());  // Graceful cancellation
            }
            
            let parsed = self.parse_sse_chunk(&chunk?)?;
            sender.send(parsed)?;
        }
        
        Ok(())
    }
}
```

**Responses API Benefits:**
- Maintains conversation state server-side
- Reduces prompt redundancy
- Supports complex multi-turn dialogues
- Built-in tool calling

### 6.3 DeepSeek Provider

**Key Features:**
- Optimized for code generation
- Reasoning model for complex problems
- Compatible with OpenAI API format
- Cost-effective for bulk operations

**Implementation:**
```rust
pub struct DeepSeekProvider {
    client: Client,
    model: String,
    api_key: String,
    base_url: String,  // https://api.deepseek.com
}

impl DeepSeekProvider {
    pub async fn generate_code(
        &self,
        prompt: String,
        context: CodeContext,
    ) -> Result<String> {
        let messages = vec![
            Message {
                role: "system".into(),
                content: self.build_code_gen_prompt(&context),
            },
            Message {
                role: "user".into(),
                content: prompt,
            },
        ];
        
        let response = self.create(messages, String::new(), None).await?;
        Ok(response.content)
    }
}
```

**DeepSeek Prompt Engineering:**
- Emphasize code quality and best practices
- Include relevant documentation excerpts
- Provide architectural constraints
- Request specific output format (artifacts)

### 6.4 Embedding Provider (OpenAI)

```rust
pub struct EmbeddingService {
    client: Client,
    api_key: String,
    model: String,  // "text-embedding-3-large"
    qdrant: QdrantClient,
}

impl EmbeddingService {
    pub async fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        let response = self.client
            .post("https://api.openai.com/v1/embeddings")
            .json(&json!({
                "model": self.model,
                "input": text,
            }))
            .send()
            .await?;
            
        let data: EmbeddingResponse = response.json().await?;
        Ok(data.data[0].embedding.clone())
    }
    
    pub async fn store_embedding(
        &self,
        point_id: Uuid,
        vector: Vec<f32>,
        heads: Vec<&str>,
        metadata: HashMap<String, Value>,
    ) -> Result<()> {
        for head in heads {
            let collection = format!("embeddings_{}", head);
            
            self.qdrant.upsert_points(
                collection,
                vec![PointStruct {
                    id: point_id.into(),
                    vector: vector.clone().into(),
                    payload: metadata.clone().into(),
                }],
                None,
            ).await?;
        }
        
        Ok(())
    }
}
```

### 6.5 Provider Configuration

Providers are configured via environment variables:

```env
# OpenAI (GPT-5)
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-5-0314

# DeepSeek
DEEPSEEK_API_KEY=...
DEEPSEEK_MODEL=deepseek-reasoner

# Embeddings
EMBEDDING_MODEL=text-embedding-3-large
EMBEDDING_DIMENSIONS=3072
```

---

## 7. WebSocket Protocol

### 7.1 Message Format

**Client → Server:**
```json
{
  "type": "chat_request",
  "conversation_id": "uuid",
  "message": "user message text",
  "context": {
    "file_path": "optional/path.rs",
    "selection": "optional selected text"
  }
}
```

**Server → Client:**
```json
{
  "type": "chat_response",
  "content": "streaming text chunk",
  "finished": false
}
```

```json
{
  "type": "artifact",
  "path": "src/main.rs",
  "content": "complete file content",
  "language": "rust"
}
```

```json
{
  "type": "operation_status",
  "operation_id": "uuid",
  "status": "delegating",
  "message": "Delegating to DeepSeek for code generation..."
}
```

```json
{
  "type": "error",
  "message": "Error description",
  "code": "ERROR_CODE"
}
```

### 7.2 Connection Lifecycle

```
Client                          Server
  │                               │
  ├── WS Connect ─────────────────>│
  │<─── Connection Accepted ───────┤
  │                               │
  ├── chat_request ───────────────>│
  │                               ├── Process request
  │                               ├── Gather context
  │                               ├── Call GPT-5
  │<─── chat_response (stream) ────┤
  │<─── chat_response (stream) ────┤
  │<─── operation_status ───────────┤
  │                               ├── Delegate to DeepSeek
  │<─── chat_response (stream) ────┤
  │<─── artifact ───────────────────┤
  │<─── chat_response (finished) ───┤
  │                               │
  ├── cancel_request ─────────────>│
  │<─── operation_cancelled ────────┤
  │                               │
  ├── WS Close ───────────────────>│
  │<─── Connection Closed ──────────┤
```

### 7.3 Cancellation Support

Users can cancel long-running operations:

```json
// Client sends:
{
  "type": "cancel_operation",
  "operation_id": "uuid"
}

// Server responds:
{
  "type": "operation_cancelled",
  "operation_id": "uuid"
}
```

**Implementation:**
```rust
pub async fn handle_cancel(
    &self,
    operation_id: Uuid,
) -> Result<()> {
    if let Some(token) = self.active_operations.get(&operation_id) {
        token.cancel();  // Triggers cancellation in stream loop
    }
    
    self.storage.update_operation_status(
        operation_id,
        OperationStatus::Cancelled,
    ).await?;
    
    Ok(())
}
```

### 7.4 Error Handling

All errors are caught and sent to client:

```rust
match process_request(req).await {
    Ok(response) => send_response(response),
    Err(e) => send_error(ErrorResponse {
        type: "error",
        message: e.to_string(),
        code: error_code(&e),
    }),
}
```

**Error Codes:**
- `VALIDATION_ERROR` - Invalid request format
- `CONTEXT_ERROR` - Failed to gather context
- `LLM_ERROR` - Provider API failure
- `STORAGE_ERROR` - Database operation failed
- `TIMEOUT_ERROR` - Operation exceeded time limit

### 7.5 Connection Management

```rust
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<Uuid, Connection>>>,
}

impl ConnectionManager {
    pub async fn add_connection(
        &self,
        user_id: Uuid,
        ws: WebSocket,
    ) -> Result<()> {
        let (tx, rx) = ws.split();
        
        let conn = Connection {
            user_id,
            sender: tx,
            active_operations: HashMap::new(),
        };
        
        self.connections.write().await.insert(user_id, conn);
        
        // Start receive loop
        tokio::spawn(self.handle_messages(user_id, rx));
        
        Ok(())
    }
    
    pub async fn broadcast(
        &self,
        user_id: Uuid,
        message: WsMessage,
    ) -> Result<()> {
        if let Some(conn) = self.connections.read().await.get(&user_id) {
            conn.sender.send(message).await?;
        }
        Ok(())
    }
}
```

---

## 8. Data Flow Examples

### 8.1 Simple Chat Flow

```
User: "Explain Rust ownership"

1. WebSocket receives message
2. UnifiedChatHandler::handle_message()
3. Gather context:
   - Rolling summaries (10, 100)
   - Recalled messages (semantic search: "rust ownership")
   - Relationships (user expertise level)
4. Build prompt with context
5. Call GPT-5 Provider (simple chat, no tools)
6. Stream response chunks to WebSocket
7. Store message in SQLite
8. Analyze message with GPT-4o-mini
9. Generate embedding if salience >= threshold
10. Update rolling summaries
```

### 8.2 Code Generation Operation Flow

```
User: "Create a user authentication module with JWT"

1. WebSocket receives message
2. UnifiedChatHandler routes to OperationEngine
3. OperationEngine::create_operation()
4. Status: ANALYZING
5. Gather comprehensive context:
   - Project file tree
   - Existing auth code
   - Dependencies (Cargo.toml)
   - Recalled similar operations
   - Rolling summaries
6. Build GPT-5 prompt with delegation tools
7. Call GPT-5 Provider::stream()
8. GPT-5 responds with:
   {
     "tool_call": {
       "name": "generate_code",
       "arguments": {
         "file_path": "src/auth/jwt.rs",
         "description": "JWT authentication with refresh tokens",
         "requirements": ["secure token generation", "expiry handling", ...]
       }
     }
   }
9. Status: DELEGATING
10. OperationEngine::delegate_to_deepseek()
11. Build DeepSeek prompt:
    - Tool arguments
    - File tree context
    - Existing code patterns
    - GPT-5's analysis summary
12. Call DeepSeek Provider::stream()
13. Status: GENERATING
14. Parse artifacts from DeepSeek response
15. Store artifacts in database
16. Stream to WebSocket in real-time
17. Status: COMPLETED
18. Continue GPT-5 streaming (summary of what was done)
19. Store all messages, generate embeddings
20. Update summaries
```

### 8.3 Memory Recall Flow

```
User asks: "What did we discuss about error handling last week?"

1. Recall Engine receives query
2. Analyze query with GPT-4o-mini:
   - Intent: "retrieve past conversation"
   - Topic: "error handling"
   - Temporal: "last week"
3. Hybrid search:
   a) Recent Search:
      - Filter: created_at > (now - 7 days)
      - Order: DESC
      - Limit: 10
   b) Semantic Search:
      - Generate query embedding
      - Search Qdrant heads: ["general", "code"]
      - Filter: created_at > (now - 7 days)
      - Limit: 10
      - Min similarity: 0.7
4. Merge results, deduplicate
5. Rank by: (similarity * 0.7) + (recency * 0.3)
6. Return top 5 messages with analysis
7. Build response referencing specific messages
8. Stream to user
```

### 8.4 Artifact Creation Flow

```
DeepSeek generates code with artifacts:

Response text: "Here's the authentication module:

<artifact path="src/auth/jwt.rs" language="rust">
use jsonwebtoken::{encode, decode, Header, Validation};
// ... full code ...
</artifact>

<artifact path="tests/auth_test.rs" language="rust">
#[cfg(test)]
mod tests {
    // ... test code ...
}
</artifact>"

1. OperationEngine receives streamed response
2. Accumulates text in buffer
3. Regex matches artifact patterns
4. For each artifact:
   a) Extract: path, language, content
   b) Store in artifacts table:
      - operation_id
      - message_id
      - path
      - content
      - language
      - created_at
   c) Send artifact message to WebSocket
5. Mark operation as COMPLETED
6. Link all artifacts to operation
```

### 8.5 Rolling Summary Generation

```
After 10th message in conversation:

1. Storage service triggers summary generation
2. MemoryService::generate_rolling_summary()
3. Fetch last 10 messages from database
4. Build summary prompt:
   "Summarize the following conversation focusing on:
    - Key topics discussed
    - Decisions made
    - Action items
    - Technical context
    
    Messages:
    [... 10 messages ...]"
5. Call GPT-4o-mini (cheap, fast)
6. Receive summary: "User and assistant discussed..."
7. Store in conversation_summaries:
   - conversation_id
   - summary_type: Rolling10
   - content: summary text
   - message_count: 10
   - created_at
8. Repeat process for 100-message summary every 10 messages
```

---

## 9. Database Schema

### 9.1 Core Tables

**messages**
```sql
CREATE TABLE messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
    content TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    metadata JSONB,
    
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX idx_messages_conversation ON messages(conversation_id, created_at DESC);
CREATE INDEX idx_messages_created ON messages(created_at DESC);
```

**message_analysis**
```sql
CREATE TABLE message_analysis (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    message_id UUID NOT NULL UNIQUE,
    intent TEXT,
    entities JSONB,
    topics TEXT[],
    sentiment TEXT,
    salience REAL NOT NULL DEFAULT 0.5,
    analysis_metadata JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE CASCADE
);

CREATE INDEX idx_analysis_salience ON message_analysis(salience DESC);
CREATE INDEX idx_analysis_message ON message_analysis(message_id);
```

**operations**
```sql
CREATE TABLE operations (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    kind TEXT NOT NULL,                    -- "code_generation", "refactor", etc.
    status TEXT NOT NULL,                  -- "pending", "started", "delegating", etc.
    
    -- Timing
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    completed_at INTEGER,
    
    -- Input
    user_message TEXT NOT NULL,
    context_snapshot TEXT,                 -- JSON snapshot of context
    
    -- Analysis & Routing
    complexity_score REAL,
    delegated_to TEXT,                     -- e.g., "deepseek"
    primary_model TEXT,                    -- e.g., "gpt-5"
    delegation_reason TEXT,
    
    -- GPT-5 Responses API Tracking
    response_id TEXT,
    parent_response_id TEXT,
    parent_operation_id TEXT,
    
    -- Code-specific context
    target_language TEXT,
    target_framework TEXT,
    operation_intent TEXT,
    files_affected TEXT,                   -- JSON array
    
    -- Results
    result TEXT,
    error TEXT,
    
    -- Cost Tracking
    tokens_input INTEGER,
    tokens_output INTEGER,
    tokens_reasoning INTEGER,
    cost_usd REAL,
    delegate_calls INTEGER DEFAULT 0,
    
    -- Metadata
    metadata TEXT                          -- JSON
);

CREATE INDEX idx_operations_session ON operations(session_id, created_at DESC);
CREATE INDEX idx_operations_status ON operations(status, created_at DESC);
CREATE INDEX idx_operations_kind ON operations(kind, created_at DESC);
```

**operation_events**
```sql
CREATE TABLE operation_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL,
    event_type TEXT NOT NULL,              -- "started", "delegated", "completed", etc.
    created_at INTEGER NOT NULL,
    sequence_number INTEGER NOT NULL,
    event_data TEXT,                       -- JSON
    
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE
);

CREATE INDEX idx_operation_events_operation ON operation_events(
    operation_id, 
    sequence_number ASC
);
```

**artifacts**
```sql
CREATE TABLE artifacts (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    
    -- Core identification
    kind TEXT NOT NULL,                    -- "code", "document", "diagram"
    file_path TEXT,
    content TEXT NOT NULL,
    preview TEXT,
    
    -- Content tracking
    language TEXT,
    content_hash TEXT,                     -- SHA-256
    previous_artifact_id TEXT,
    is_new_file INTEGER DEFAULT 1,         -- SQLite boolean (0/1)
    diff_from_previous TEXT,
    
    -- Context JSON fields (stored as TEXT)
    related_files TEXT,                    -- JSON array
    dependencies TEXT,                     -- JSON array
    project_context TEXT,                  -- JSON object
    user_requirements TEXT,                -- JSON object
    constraints TEXT,                      -- JSON array
    
    -- Generation metadata
    generated_by TEXT,                     -- "deepseek", "gpt5"
    generation_time_ms INTEGER,
    context_tokens INTEGER,
    output_tokens INTEGER,
    
    -- Lifecycle timestamps
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    applied_at INTEGER,
    
    -- Additional metadata
    metadata TEXT,                         -- JSON object
    
    FOREIGN KEY (operation_id) REFERENCES operations(id) ON DELETE CASCADE,
    FOREIGN KEY (previous_artifact_id) REFERENCES artifacts(id) ON DELETE SET NULL
);

CREATE INDEX idx_artifacts_operation ON artifacts(operation_id, created_at);
CREATE INDEX idx_artifacts_path ON artifacts(file_path);
CREATE INDEX idx_artifacts_hash ON artifacts(content_hash);
CREATE INDEX idx_artifacts_language ON artifacts(language, created_at);
CREATE INDEX idx_artifacts_kind ON artifacts(kind, created_at);
CREATE INDEX idx_artifacts_previous ON artifacts(previous_artifact_id);
```

**Note:** The Rust structs (`Operation`, `OperationEvent`, `Artifact`) map to these tables using sqlx `FromRow` derive macro. The `Artifact` struct currently exposes only core fields for application use, with the comprehensive schema supporting future expansion.

**relationships**
```sql
CREATE TABLE relationships (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL,
    subject TEXT NOT NULL,
    relation_type TEXT NOT NULL,
    object TEXT NOT NULL,
    confidence REAL DEFAULT 1.0,
    source_message_id UUID,
    metadata JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (source_message_id) REFERENCES messages(id) ON DELETE SET NULL,
    UNIQUE(user_id, subject, relation_type, object)
);

CREATE INDEX idx_relationships_user ON relationships(user_id, created_at DESC);
CREATE INDEX idx_relationships_type ON relationships(relation_type);
```

**conversation_summaries**
```sql
CREATE TABLE conversation_summaries (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL,
    summary_type TEXT NOT NULL CHECK (summary_type IN ('rolling_10', 'rolling_100')),
    content TEXT NOT NULL,
    message_count INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    metadata JSONB,
    
    FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
);

CREATE INDEX idx_summaries_conversation ON conversation_summaries(
    conversation_id, 
    summary_type, 
    created_at DESC
);
```

### 9.2 Qdrant Collections

**embeddings_general**
```yaml
Collection: embeddings_general
Vector Size: 3072
Distance: Cosine
Payload Schema:
  - message_id: UUID
  - conversation_id: UUID
  - user_id: UUID
  - content: String (indexed)
  - role: String
  - topics: Array<String>
  - created_at: Timestamp
```

**embeddings_code**
```yaml
Collection: embeddings_code
Vector Size: 3072
Distance: Cosine
Payload Schema:
  - message_id: UUID
  - conversation_id: UUID
  - file_path: String (indexed)
  - language: String
  - function_name: String (optional)
  - code_type: String  # function, class, module
  - created_at: Timestamp
```

**embeddings_facts**
```yaml
Collection: embeddings_facts
Vector Size: 3072
Distance: Cosine
Payload Schema:
  - message_id: UUID
  - fact_type: String  # technical, personal, project
  - entities: Array<String>
  - created_at: Timestamp
```

**embeddings_preferences**
```yaml
Collection: embeddings_preferences
Vector Size: 3072
Distance: Cosine
Payload Schema:
  - user_id: UUID
  - preference_type: String  # tool, style, workflow
  - category: String
  - created_at: Timestamp
```

### 9.3 Migration Strategy

Migrations are managed via sqlx:

```bash
sqlx migrate add create_operations_table
sqlx migrate run
```

**Migration Best Practices:**
- Always use transactions
- Include rollback scripts
- Test on copy of production data
- Monitor performance impact

---

## 10. Configuration & Deployment

### 10.1 Environment Variables

```env
# Server Configuration
MIRA_HOST=0.0.0.0
MIRA_PORT=8080
MIRA_ENV=production  # development, staging, production

# Database
DATABASE_URL=sqlite://mira.db
MIRA_SQLITE_MAX_CONNECTIONS=10

# Qdrant
QDRANT_URL=http://localhost:6333
QDRANT_API_KEY=optional_key

# OpenAI
OPENAI_API_KEY=sk-...
OPENAI_MODEL=gpt-5-0314
OPENAI_EMBEDDING_MODEL=text-embedding-3-large

# DeepSeek
DEEPSEEK_API_KEY=...
DEEPSEEK_MODEL=deepseek-reasoner
DEEPSEEK_BASE_URL=https://api.deepseek.com

# Memory Configuration
SALIENCE_MIN_FOR_EMBED=0.6
EMBED_HEADS=general,code,facts,preferences
MAX_RECALLED_MESSAGES=10
SUMMARY_GENERATION_INTERVAL=10

# Git Integration
GIT_ENABLED=true
DEFAULT_REPO_PATH=/path/to/repo

# Logging
RUST_LOG=info,mira_backend=debug
LOG_FORMAT=json  # json or pretty
```

### 10.2 Docker Deployment

**Dockerfile:**
```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    libsqlite3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/mira-backend /usr/local/bin/
CMD ["mira-backend"]
```

**docker-compose.yml:**
```yaml
version: '3.8'

services:
  mira:
    build: .
    ports:
      - "8080:8080"
    environment:
      - DATABASE_URL=sqlite:///data/mira.db
      - QDRANT_URL=http://qdrant:6333
    volumes:
      - ./data:/data
      - ./repos:/repos
    depends_on:
      - qdrant

  qdrant:
    image: qdrant/qdrant:latest
    ports:
      - "6333:6333"
    volumes:
      - qdrant_data:/qdrant/storage

volumes:
  qdrant_data:
```

### 10.3 Database Initialization

```rust
pub async fn initialize_database() -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect(&env::var("DATABASE_URL")?).await?;
    
    // Run migrations
    sqlx::migrate!("./migrations").run(&pool).await?;
    
    // Enable WAL mode for better concurrency
    sqlx::query("PRAGMA journal_mode=WAL")
        .execute(&pool).await?;
    
    Ok(pool)
}
```

### 10.4 Qdrant Initialization

```rust
pub async fn initialize_qdrant() -> Result<QdrantClient> {
    let client = QdrantClient::from_url(&env::var("QDRANT_URL")?).build()?;
    
    // Create collections if they don't exist
    for collection in ["general", "code", "facts", "preferences"] {
        let name = format!("embeddings_{}", collection);
        
        if !client.collection_exists(&name).await? {
            client.create_collection(
                &name,
                VectorParams {
                    size: 3072,
                    distance: Distance::Cosine,
                    ..Default::default()
                },
            ).await?;
            
            // Create payload indexes
            client.create_field_index(
                &name,
                "content",
                FieldType::Text,
                None,
                None,
            ).await?;
        }
    }
    
    Ok(client)
}
```

### 10.5 Monitoring & Logging

**Structured Logging:**
```rust
use tracing::{info, error, debug};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logging() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

// Usage
info!(operation_id = %op_id, status = "delegating", "Delegating to DeepSeek");
error!(error = %e, "Failed to generate embedding");
```

**Metrics (Future Enhancement):**
- Operation latency
- LLM API response times
- Database query performance
- WebSocket connection count
- Error rates by type

### 10.6 Scaling Considerations

**Vertical Scaling:**
- Increase `MIRA_SQLITE_MAX_CONNECTIONS`
- Add more CPU cores for concurrent operations
- Expand memory for larger context windows

**Horizontal Scaling:**
- Run multiple Mira instances behind load balancer
- Shared SQLite database (with proper locking)
- Qdrant cluster for distributed vector search
- Session stickiness for WebSocket connections

**Performance Optimization:**
- Enable SQLite WAL mode (Write-Ahead Logging)
- Index frequently queried columns
- Cache frequently accessed data (Redis)
- Batch embedding generation
- Async/await everywhere for non-blocking I/O

### 10.7 Backup & Recovery

**SQLite Backup:**
```bash
# Hot backup with WAL mode
sqlite3 mira.db ".backup mira_backup.db"

# Scheduled backups (cron)
0 2 * * * /usr/local/bin/backup_mira.sh
```

**Qdrant Backup:**
```bash
# Snapshot entire Qdrant instance
curl -X POST http://localhost:6333/snapshots

# Download snapshot
curl http://localhost:6333/snapshots/{snapshot_name} > qdrant_backup.snapshot
```

**Recovery:**
1. Restore SQLite database from backup
2. Restore Qdrant snapshots
3. Verify data integrity
4. Rebuild indexes if necessary

### 10.8 Security Considerations

**API Key Management:**
- Store in environment variables (never in code)
- Use secrets management (e.g., AWS Secrets Manager)
- Rotate keys regularly

**Data Privacy:**
- Encrypt SQLite database at rest (optional)
- Use HTTPS for all external API calls
- Sanitize logs (no sensitive data)

**Access Control:**
- Implement user authentication
- Rate limiting on WebSocket connections
- Validate all inputs

### 10.9 Observability

**Health Check Endpoint:**
```rust
pub async fn health_check(
    db: Extension<SqlitePool>,
    qdrant: Extension<QdrantClient>,
) -> Result<Json<HealthStatus>, StatusCode> {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&*db).await.is_ok();
    let qdrant_ok = qdrant.health_check().await.is_ok();
    
    Ok(Json(HealthStatus {
        status: if db_ok && qdrant_ok { "healthy" } else { "degraded" },
        database: db_ok,
        vector_store: qdrant_ok,
        timestamp: Utc::now(),
    }))
}
```

**Logging Levels:**
- ERROR: Critical failures requiring immediate attention
- WARN: Degraded functionality but system operational
- INFO: Important state changes and operations
- DEBUG: Detailed execution flow for troubleshooting
- TRACE: Very verbose, for deep debugging

### 10.10 Troubleshooting Guide

**Common Issues:**

1. **Tests failing with OperationEngine::new() signature mismatch**
   - The engine constructor requires:
     ```rust
     OperationEngine::new(
         db: Arc<SqlitePool>,
         gpt5: Gpt5Provider,
         deepseek: DeepSeekProvider,
         memory_service: Arc<MemoryService>,
         relationship_service: Arc<RelationshipService>,
         git_client: GitClient,
         code_intelligence: Arc<CodeIntelligenceService>,
     )
     ```
   - Update test instantiation to include all 7 parameters
   - The engine builds sub-components internally (no manual assembly needed)

2. **Database locks**
   - Enable WAL mode: `PRAGMA journal_mode=WAL`
   - Increase connection pool size
   - Check for long-running transactions

3. **Qdrant connection timeouts**
   - Verify Qdrant is running and accessible
   - Check network configuration
   - Increase timeout values

4. **High memory usage**
   - Reduce `MAX_RECALLED_MESSAGES`
   - Implement context window truncation
   - Monitor for memory leaks in long-running operations

5. **Slow embedding generation**
   - Batch embeddings where possible
   - Increase `SALIENCE_MIN_FOR_EMBED` threshold
   - Consider caching frequent queries

6. **WebSocket disconnections**
   - Implement ping/pong heartbeat
   - Add reconnection logic in frontend
   - Check for network issues

7. **Operation fails silently**
   - Check that error handling wrapper in `orchestration.rs` is working
   - Verify `Failed` events are being emitted
   - Check event channel connection
   - Enable TRACE logging for detailed flow

**Debug Commands:**
```bash
# Check SQLite integrity
sqlite3 mira.db "PRAGMA integrity_check;"

# Qdrant health check
curl http://localhost:6333/health

# Test WebSocket connection
websocat ws://localhost:8080/ws

# View logs in real-time
tail -f logs/mira.log | jq
```

---

## Conclusion

Mira's architecture represents a sophisticated balance between performance, maintainability, and extensibility. Key strengths include:

1. **Specialized LLM Orchestration**: GPT-5 for reasoning, DeepSeek for implementation
2. **Modular Operation Engine**: Focused sub-components (Orchestrator, LifecycleManager, ArtifactManager, DelegationHandler, ContextBuilder) for clean separation of concerns
3. **Comprehensive Memory**: Hybrid search combining recency and semantic similarity
4. **Real-time Streaming**: WebSocket-based bidirectional communication with robust error handling
5. **Type Safety**: Rust's guarantees prevent entire classes of bugs
6. **Dual Storage**: SQLite for structure, Qdrant for semantic search

**Future Enhancements:**
- Multi-user support with proper isolation
- Enhanced code intelligence (LSP integration)
- Custom fine-tuned models for specific domains
- Advanced caching strategies
- Distributed deployment architecture

For questions, contributions, or architectural discussions, refer to the source code in the repository. Key entry points:
- `src/operations/engine/mod.rs` - Modular operation engine
- `src/operations/engine/orchestration.rs` - Main operation orchestration logic
- `src/operations/engine/lifecycle.rs` - State management
- `src/operations/engine/artifacts.rs` - Artifact handling
- `src/api/ws/chat/unified_handler.rs` - WebSocket message routing
- `src/memory/service/mod.rs` - Memory architecture

The implementation is designed to be readable and well-documented, with extensive inline comments explaining architectural decisions.

---

**Document Metadata:**
- Version: 1.1
- Last Updated: Post-Operation Engine Refactor
- Status: Production-ready
- License: Proprietary
