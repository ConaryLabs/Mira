# Mira MCP Server Design

Design document for an MCP server that exposes Mira's SQLite database and Qdrant vector store to external clients like Claude Code.

## Overview

The `mira-mcp-server` binary will be a standalone MCP server that connects to Mira's existing database infrastructure and exposes it via the Model Context Protocol. This enables Claude Code (or any MCP client) to directly query Mira's data.

```
┌─────────────────┐     stdio/HTTP      ┌──────────────────┐
│   Claude Code   │◄──────────────────►│  mira-mcp-server │
└─────────────────┘                     └────────┬─────────┘
                                                 │
                            ┌────────────────────┼────────────────────┐
                            │                    │                    │
                            ▼                    ▼                    ▼
                     ┌──────────┐        ┌──────────────┐      ┌──────────┐
                     │  SQLite  │        │    Qdrant    │      │  OpenAI  │
                     │ mira.db  │        │ Collections  │      │ Embed API│
                     └──────────┘        └──────────────┘      └──────────┘
```

## Tool Categories

### 1. Session & Memory Tools

| Tool | Description | Parameters |
|------|-------------|------------|
| `list_sessions` | List chat sessions with filters | `limit`, `session_type`, `status` |
| `get_session` | Get session details by ID | `session_id` |
| `search_memories` | Full-text search in memory entries | `query`, `limit`, `session_id` |
| `get_recent_messages` | Get recent messages from a session | `session_id`, `limit` |

### 2. Semantic Search Tools (Qdrant)

| Tool | Description | Parameters |
|------|-------------|------------|
| `semantic_search` | Vector similarity search | `query`, `collection`, `limit`, `threshold` |
| `search_code` | Search code embeddings | `query`, `limit`, `file_filter` |
| `search_conversations` | Search conversation embeddings | `query`, `limit`, `session_id` |
| `search_git` | Search git-related embeddings | `query`, `limit` |

### 3. Code Intelligence Tools

| Tool | Description | Parameters |
|------|-------------|------------|
| `get_semantic_graph` | Get semantic relationships for a file | `file_path` |
| `get_call_graph` | Get function call relationships | `function_name`, `depth` |
| `find_cochange_patterns` | Find files that change together | `file_path`, `limit` |
| `get_design_patterns` | Get detected design patterns | `file_path` |
| `get_author_expertise` | Get expertise scores by file/area | `path_pattern` |

### 4. Project & Git Tools

| Tool | Description | Parameters |
|------|-------------|------------|
| `list_projects` | List tracked projects | `limit` |
| `get_project_files` | Get file tree for a project | `project_id`, `path` |
| `get_recent_commits` | Get recent commits | `project_id`, `limit`, `author` |
| `get_file_history` | Get change history for a file | `file_path`, `limit` |

### 5. Operations & Artifacts Tools

| Tool | Description | Parameters |
|------|-------------|------------|
| `list_operations` | List operations with filters | `session_id`, `status`, `limit` |
| `get_operation` | Get operation details | `operation_id` |
| `list_artifacts` | List generated artifacts | `operation_id`, `file_type` |
| `get_artifact` | Get artifact content | `artifact_id` |

### 6. Analytics Tools

| Tool | Description | Parameters |
|------|-------------|------------|
| `get_budget_status` | Get current budget usage | - |
| `get_cache_stats` | Get LLM cache hit rates | `time_range` |
| `get_tool_usage` | Get tool execution statistics | `limit` |

## Resources

MCP Resources provide read-only access to structured data:

| Resource URI | Description |
|--------------|-------------|
| `mira://schema` | Database schema definition |
| `mira://sessions/{id}` | Session details |
| `mira://projects/{id}/tree` | Project file tree |
| `mira://collections` | Qdrant collection list |
| `mira://budget` | Current budget status |

## Implementation

### File Structure

```
backend/
├── src/
│   └── bin/
│       └── mira_mcp_server.rs    # MCP server binary
├── src/
│   └── mcp_server/
│       ├── mod.rs                # Server setup
│       ├── tools/
│       │   ├── mod.rs
│       │   ├── sessions.rs       # Session/memory tools
│       │   ├── semantic.rs       # Qdrant search tools
│       │   ├── code_intel.rs     # Code intelligence tools
│       │   ├── projects.rs       # Project/git tools
│       │   ├── operations.rs     # Operations/artifacts tools
│       │   └── analytics.rs      # Budget/cache/stats tools
│       └── resources/
│           ├── mod.rs
│           └── schema.rs         # Resource handlers
```

### Dependencies

```toml
[dependencies]
rmcp = { version = "0.1", features = ["server", "macros"] }  # Official Rust MCP SDK
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite"] }
qdrant-client = "1.12"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "0.8"  # For JSON Schema generation
anyhow = "1"
tracing = "0.1"
```

### Server Skeleton

```rust
// backend/src/bin/mira_mcp_server.rs

use anyhow::Result;
use rmcp::{Server, ServerBuilder, tool};
use sqlx::SqlitePool;
use qdrant_client::Qdrant;
use std::sync::Arc;

struct MiraServer {
    db: SqlitePool,
    qdrant: Qdrant,
}

impl MiraServer {
    async fn new() -> Result<Self> {
        let db_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://data/mira.db".to_string());
        let qdrant_url = std::env::var("QDRANT_URL")
            .unwrap_or_else(|_| "http://localhost:6334".to_string());

        let db = SqlitePool::connect(&db_url).await?;
        let qdrant = Qdrant::from_url(&qdrant_url).build()?;

        Ok(Self { db, qdrant })
    }

    // === Session Tools ===

    #[tool(description = "List chat sessions with optional filters")]
    async fn list_sessions(
        &self,
        #[tool(param)]
        #[schemars(description = "Maximum number of sessions to return")]
        limit: Option<u32>,
        #[tool(param)]
        #[schemars(description = "Filter by session type: voice, codex")]
        session_type: Option<String>,
    ) -> Result<String, String> {
        let limit = limit.unwrap_or(20);

        let sessions = sqlx::query_as::<_, (String, String, String, i64)>(
            r#"SELECT id, name, session_type, last_active
               FROM chat_sessions
               WHERE ($1 IS NULL OR session_type = $1)
               ORDER BY last_active DESC
               LIMIT $2"#
        )
        .bind(&session_type)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        Ok(serde_json::to_string_pretty(&sessions).unwrap())
    }

    // === Semantic Search Tools ===

    #[tool(description = "Search for semantically similar content using vector embeddings")]
    async fn semantic_search(
        &self,
        #[tool(param)]
        #[schemars(description = "The search query text")]
        query: String,
        #[tool(param)]
        #[schemars(description = "Collection to search: code, conversation, git")]
        collection: String,
        #[tool(param)]
        #[schemars(description = "Maximum results to return")]
        limit: Option<u32>,
    ) -> Result<String, String> {
        // Would need to embed the query first via OpenAI
        // Then search Qdrant
        let limit = limit.unwrap_or(10);

        // Placeholder - real impl would embed and search
        Ok(format!("Would search '{}' in {} (limit {})", query, collection, limit))
    }

    // === Code Intelligence Tools ===

    #[tool(description = "Find files that frequently change together with the given file")]
    async fn find_cochange_patterns(
        &self,
        #[tool(param)]
        #[schemars(description = "File path to find co-change patterns for")]
        file_path: String,
        #[tool(param)]
        #[schemars(description = "Maximum patterns to return")]
        limit: Option<u32>,
    ) -> Result<String, String> {
        let limit = limit.unwrap_or(10);

        let patterns = sqlx::query_as::<_, (String, String, i64, f64)>(
            r#"SELECT file_a, file_b, co_change_count, confidence
               FROM file_cochange_patterns
               WHERE file_a = $1 OR file_b = $1
               ORDER BY confidence DESC
               LIMIT $2"#
        )
        .bind(&file_path)
        .bind(limit as i64)
        .fetch_all(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        Ok(serde_json::to_string_pretty(&patterns).unwrap())
    }

    // === Analytics Tools ===

    #[tool(description = "Get current budget usage and limits")]
    async fn get_budget_status(&self) -> Result<String, String> {
        let budget = sqlx::query_as::<_, (f64, f64, f64, f64)>(
            r#"SELECT daily_spent, daily_limit, monthly_spent, monthly_limit
               FROM budget_summary
               WHERE id = 'current'"#
        )
        .fetch_optional(&self.db)
        .await
        .map_err(|e| e.to_string())?;

        match budget {
            Some((daily_spent, daily_limit, monthly_spent, monthly_limit)) => {
                Ok(format!(
                    "Daily: ${:.2}/${:.2} | Monthly: ${:.2}/${:.2}",
                    daily_spent, daily_limit, monthly_spent, monthly_limit
                ))
            }
            None => Ok("No budget data found".to_string())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::init();

    let server = MiraServer::new().await?;

    // Run as stdio server for Claude Code integration
    Server::builder()
        .name("mira-database")
        .version("1.0.0")
        .capabilities(|caps| {
            caps.tools(true)
                .resources(true)
        })
        .serve_stdio(server)
        .await?;

    Ok(())
}
```

## Configuration

### Claude Code MCP Config

Add to `~/.claude/mcp.json`:

```json
{
  "servers": {
    "mira": {
      "command": "/home/peter/Mira/backend/target/release/mira-mcp-server",
      "env": {
        "DATABASE_URL": "sqlite:///home/peter/Mira/backend/data/mira.db",
        "QDRANT_URL": "http://localhost:6334",
        "OPENAI_API_KEY": "${OPENAI_API_KEY}"
      }
    }
  }
}
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | SQLite connection string | `sqlite://data/mira.db` |
| `QDRANT_URL` | Qdrant gRPC endpoint | `http://localhost:6334` |
| `OPENAI_API_KEY` | For embedding queries | Required for semantic search |
| `RUST_LOG` | Logging level | `info` |

## Usage Examples

Once configured, Claude Code can use these tools directly:

```
User: "What sessions have I had recently?"
Claude: [calls list_sessions with limit=5]
> Shows 5 most recent chat sessions

User: "Search my code for authentication logic"
Claude: [calls semantic_search with query="authentication login user validation", collection="code"]
> Returns semantically similar code snippets

User: "What files usually change together with src/api/auth.rs?"
Claude: [calls find_cochange_patterns with file_path="src/api/auth.rs"]
> Returns files with high co-change correlation

User: "How much of my API budget have I used?"
Claude: [calls get_budget_status]
> Daily: $2.45/$5.00 | Monthly: $47.20/$150.00
```

## Security Considerations

1. **Read-Only by Default**: Most tools only read data. Write operations require explicit opt-in.

2. **SQL Injection Prevention**: All queries use parameterized statements via SQLx.

3. **Path Validation**: File paths are validated against allowed project directories.

4. **Rate Limiting**: Consider adding rate limits for expensive operations (semantic search).

5. **API Key Security**: OpenAI key is passed via environment, not stored in config files.

## Future Enhancements

1. **Write Tools**: Add tools for creating sessions, saving artifacts, etc.

2. **Streaming Results**: Support streaming for large result sets.

3. **Subscriptions**: Implement resource subscriptions for real-time updates.

4. **Authentication**: Add optional auth for multi-user scenarios.

5. **Caching**: Cache frequent queries and embeddings.

## Implementation Priority

### Phase 1: Core Query Tools
- [ ] `list_sessions`
- [ ] `get_session`
- [ ] `search_memories`
- [ ] `get_budget_status`

### Phase 2: Semantic Search
- [ ] `semantic_search` (requires embedding integration)
- [ ] `search_code`
- [ ] `search_conversations`

### Phase 3: Code Intelligence
- [ ] `find_cochange_patterns`
- [ ] `get_call_graph`
- [ ] `get_design_patterns`

### Phase 4: Resources & Advanced
- [ ] Schema resource
- [ ] Project tree resource
- [ ] Write operations (opt-in)
