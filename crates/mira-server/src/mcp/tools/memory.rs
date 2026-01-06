// src/mcp/tools/memory.rs
// Memory tools: remember, recall, forget - unified core delegation

use crate::mcp::MiraServer;
use crate::tools::core::memory;

/// Store a memory fact
pub async fn remember(
    server: &MiraServer,
    content: String,
    key: Option<String>,
    fact_type: Option<String>,
    category: Option<String>,
    confidence: Option<f64>,
) -> Result<String, String> {
    memory::remember(server, content, key, fact_type, category, confidence).await
}

/// Search memories
pub async fn recall(
    server: &MiraServer,
    query: String,
    limit: Option<i64>,
    category: Option<String>,
    fact_type: Option<String>,
) -> Result<String, String> {
    memory::recall(server, query, limit, category, fact_type).await
}

/// Delete a memory
pub async fn forget(server: &MiraServer, id: String) -> Result<String, String> {
    memory::forget(server, id).await
}
