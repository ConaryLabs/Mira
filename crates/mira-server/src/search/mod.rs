// crates/mira-server/src/search/mod.rs
// Unified search functionality shared between MCP and chat tools

mod keyword;
mod semantic;
mod utils;

pub use keyword::keyword_search;
pub use semantic::{
    crossref_search, expand_context, expand_context_with_db, find_callers, find_callees,
    format_crossref_results, format_results, hybrid_search, semantic_search,
    CrossRefResult, CrossRefType, HybridSearchResult, SearchResult, SearchType,
};
pub use utils::{distance_to_score, embedding_to_bytes, format_project_header};
