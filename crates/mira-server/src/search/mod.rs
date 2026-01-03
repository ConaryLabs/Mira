// crates/mira-server/src/search/mod.rs
// Unified search functionality shared between MCP and chat tools

mod keyword;
mod semantic;
mod utils;

pub use keyword::keyword_search;
pub use semantic::{expand_context, expand_context_with_db, format_results, hybrid_search, semantic_search, HybridSearchResult, SearchResult, SearchType};
pub use utils::{distance_to_score, embedding_to_bytes, format_project_header};
