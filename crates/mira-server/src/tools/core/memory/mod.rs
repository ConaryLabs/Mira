// crates/mira-server/src/tools/core/memory/mod.rs
//! Unified memory tool with CRUD, export, entity, and archive operations
//! (recall, remember, forget, list, archive, export, purge, entities, export_claude_local)

mod security;

pub mod crud;
pub mod recall;
pub mod remember;

// Re-export public API
pub use crud::{archive, forget};
pub use recall::recall;
pub use remember::remember;

use crate::error::MiraError;
use crate::mcp::responses::Json;
use crate::mcp::responses::MemoryOutput;
use crate::tools::core::ToolContext;

/// Unified memory tool dispatcher
pub async fn handle_memory<C: ToolContext>(
    ctx: &C,
    req: crate::mcp::requests::MemoryRequest,
) -> Result<Json<MemoryOutput>, MiraError> {
    use crate::mcp::requests::MemoryAction;
    match req.action {
        MemoryAction::Remember => {
            let content = req.content.ok_or_else(|| {
                MiraError::InvalidInput(
                    "content is required for memory(action=remember)".to_string(),
                )
            })?;
            remember::remember(
                ctx,
                content,
                req.key,
                req.fact_type,
                req.category,
                req.confidence,
                req.scope,
            )
            .await
        }
        MemoryAction::Recall => {
            let query = req.query.ok_or_else(|| {
                MiraError::InvalidInput("query is required for memory(action=recall)".to_string())
            })?;
            recall::recall(ctx, query, req.limit, req.category, req.fact_type).await
        }
        MemoryAction::List => {
            crud::list_memories(ctx, req.limit, req.offset, req.category, req.fact_type).await
        }
        MemoryAction::Forget => {
            let id = req.id.ok_or_else(|| {
                MiraError::InvalidInput("id is required for memory(action=forget)".to_string())
            })?;
            crud::forget(ctx, id).await
        }
        MemoryAction::Archive => {
            let id = req.id.ok_or_else(|| {
                MiraError::InvalidInput("id is required for memory(action=archive)".to_string())
            })?;
            crud::archive(ctx, id).await
        }
        MemoryAction::ExportClaudeLocal => {
            let message = crate::tools::core::claude_local::export_claude_local(ctx).await?;
            Ok(Json(MemoryOutput {
                action: "export_claude_local".into(),
                message,
                data: None,
            }))
        }
        MemoryAction::Export => crud::export_memories(ctx).await,
        MemoryAction::Purge => crud::purge_memories(ctx, req.confirm).await,
        MemoryAction::Entities => crud::list_entities(ctx, req.query, req.limit).await,
    }
}
