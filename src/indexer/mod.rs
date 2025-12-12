// src/indexer/mod.rs
// Code and Git indexing for populating intelligence tables

pub mod code;
pub mod git;
pub mod parsers;
pub mod watcher;

pub use code::CodeIndexer;
pub use git::GitIndexer;
pub use watcher::Watcher;

/// Statistics returned after indexing operations
#[derive(Debug, Default, Clone)]
pub struct IndexStats {
    pub files_processed: usize,
    pub symbols_found: usize,
    pub imports_found: usize,
    pub calls_found: usize,
    pub unresolved_calls: usize,
    pub resolved_calls: usize,
    pub commits_indexed: usize,
    pub cochange_patterns: usize,
    pub embeddings_generated: usize,
    pub errors: Vec<String>,
}

impl IndexStats {
    pub fn merge(&mut self, other: IndexStats) {
        self.files_processed += other.files_processed;
        self.symbols_found += other.symbols_found;
        self.imports_found += other.imports_found;
        self.calls_found += other.calls_found;
        self.unresolved_calls += other.unresolved_calls;
        self.resolved_calls += other.resolved_calls;
        self.commits_indexed += other.commits_indexed;
        self.cochange_patterns += other.cochange_patterns;
        self.embeddings_generated += other.embeddings_generated;
        self.errors.extend(other.errors);
    }
}
