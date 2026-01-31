// crates/mira-server/src/search/mod.rs
// Unified search functionality shared between MCP and chat tools

mod context;
mod crossref;
mod keyword;
mod semantic;
mod tree;
mod utils;

pub use context::{expand_context, expand_context_with_conn};
pub use crossref::{
    CrossRefResult, CrossRefType, crossref_search, find_callees, find_callers,
    format_crossref_results,
};
pub use keyword::keyword_search;
pub use semantic::{
    HybridSearchResult, SearchResult, SearchType, format_results, hybrid_search, semantic_search,
};
pub use utils::{distance_to_score, embedding_to_bytes, format_project_header};
