// src/memory/features/recall_engine/search/mod.rs

//! Search strategies - focused, single-purpose search implementations.

mod recent_search;
mod semantic_search;
mod hybrid_search;
mod multihead_search;

pub use recent_search::RecentSearch;
pub use semantic_search::SemanticSearch;
pub use hybrid_search::HybridSearch;
pub use multihead_search::MultiHeadSearch;
