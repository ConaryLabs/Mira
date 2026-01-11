// crates/mira-server/src/search/mod.rs
// Unified search functionality shared between MCP and chat tools

mod context;
mod crossref;
mod keyword;
mod semantic;
mod utils;

pub use context::{expand_context, expand_context_with_db};
pub use crossref::{
    crossref_search, find_callers, find_callees, format_crossref_results, CrossRefResult,
    CrossRefType,
};
pub use keyword::keyword_search;
pub use semantic::{
    format_results, hybrid_search, semantic_search, HybridSearchResult, SearchResult, SearchType,
};
pub use utils::{distance_to_score, embedding_to_bytes, format_project_header};
