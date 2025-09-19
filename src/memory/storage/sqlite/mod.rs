pub mod store;
pub mod core;
// pub mod code_intelligence;  // Future: AST and code analysis storage
// pub mod projects;           // Future: Git and project-specific operations

pub use store::SqliteMemoryStore;
