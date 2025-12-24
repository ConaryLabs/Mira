// src/indexer/code.rs
// Code symbol extraction using tree-sitter

#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::type_complexity)]

use std::path::{Path, PathBuf};
use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use chrono::Utc;
use walkdir::WalkDir;
use ignore::gitignore::Gitignore;
use tree_sitter::Parser;
use futures::stream::{FuturesUnordered, StreamExt};

use super::IndexStats;
use super::parsers;
use crate::tools::SemanticSearch;
use std::sync::Arc;

// Re-export types for backwards compatibility
pub use super::parsers::{Symbol, Import, FunctionCall, ParsedFile};

pub struct CodeIndexer {
    db: SqlitePool,
    semantic: Option<Arc<SemanticSearch>>,
    rust_parser: Parser,
    python_parser: Parser,
    typescript_parser: Parser,
    javascript_parser: Parser,
    go_parser: Parser,
}

impl CodeIndexer {
    #[allow(dead_code)] // Convenience constructor
    pub fn new(db: SqlitePool) -> Result<Self> {
        Self::with_semantic(db, None)
    }

    pub fn with_semantic(db: SqlitePool, semantic: Option<Arc<SemanticSearch>>) -> Result<Self> {
        let mut rust_parser = Parser::new();
        rust_parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;

        let mut python_parser = Parser::new();
        python_parser.set_language(&tree_sitter_python::LANGUAGE.into())?;

        let mut typescript_parser = Parser::new();
        typescript_parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;

        let mut javascript_parser = Parser::new();
        javascript_parser.set_language(&tree_sitter_javascript::LANGUAGE.into())?;

        let mut go_parser = Parser::new();
        go_parser.set_language(&tree_sitter_go::LANGUAGE.into())?;

        Ok(Self {
            db,
            semantic,
            rust_parser,
            python_parser,
            typescript_parser,
            javascript_parser,
            go_parser,
        })
    }

    /// Generate embeddable text representation of a symbol
    fn symbol_to_text(symbol: &Symbol, file_path: &str) -> String {
        let mut text = format!("{} ({})", symbol.name, symbol.symbol_type);

        if let Some(ref sig) = symbol.signature {
            text.push_str(&format!("\nSignature: {}", sig));
        }

        if let Some(ref doc) = symbol.documentation {
            text.push_str(&format!("\nDoc: {}", doc));
        }

        // Use relative path for cleaner embedding
        let display_path = file_path
            .split("/Mira/")
            .last()
            .unwrap_or(file_path);
        text.push_str(&format!("\nFile: {}", display_path));

        text
    }

    /// Generate a unique ID for a symbol (for Qdrant deduplication)
    fn symbol_id(file_path: &str, symbol: &Symbol) -> String {
        format!("{}:{}:{}", file_path, symbol.name, symbol.start_line)
    }

    /// Index all code files in a directory (sequential)
    pub async fn index_directory(&mut self, path: &Path) -> Result<IndexStats> {
        let mut stats = IndexStats::default();

        // Load .gitignore if present
        let gitignore_path = path.join(".gitignore");
        let gitignore = if gitignore_path.exists() {
            Gitignore::new(&gitignore_path).0
        } else {
            Gitignore::empty()
        };

        // Walk directory
        for entry in WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let file_path = entry.path();

            // Skip directories and non-code files
            if !file_path.is_file() {
                continue;
            }

            // Skip gitignored files
            if gitignore.matched(file_path, false).is_ignore() {
                continue;
            }

            // Skip hidden directories and build output
            if file_path.components().any(|c| {
                let name = c.as_os_str().to_string_lossy();
                name.starts_with('.') || name == "target" || name == "node_modules" || name == "__pycache__"
            }) {
                continue;
            }

            // Check extension
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go") {
                continue;
            }

            match self.index_file(file_path).await {
                Ok(file_stats) => stats.merge(file_stats),
                Err(e) => stats.errors.push(format!("{}: {}", file_path.display(), e)),
            }
        }

        Ok(stats)
    }

    /// Index all code files in a directory (parallel parsing, sequential writes)
    /// Parses files in parallel for CPU efficiency, then writes to SQLite sequentially
    /// to avoid lock contention
    pub async fn index_directory_parallel(
        db: SqlitePool,
        semantic: Option<Arc<SemanticSearch>>,
        path: &Path,
        max_concurrent: usize,
    ) -> Result<IndexStats> {
        let mut stats = IndexStats::default();

        // Load .gitignore if present
        let gitignore_path = path.join(".gitignore");
        let gitignore = if gitignore_path.exists() {
            Gitignore::new(&gitignore_path).0
        } else {
            Gitignore::empty()
        };

        // Collect all files to process
        let mut files_to_process: Vec<PathBuf> = Vec::new();

        for entry in WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let file_path = entry.path();

            // Skip directories and non-code files
            if !file_path.is_file() {
                continue;
            }

            // Skip gitignored files
            if gitignore.matched(file_path, false).is_ignore() {
                continue;
            }

            // Skip hidden directories and build output
            if file_path.components().any(|c| {
                let name = c.as_os_str().to_string_lossy();
                name.starts_with('.') || name == "target" || name == "node_modules" || name == "__pycache__"
            }) {
                continue;
            }

            // Check extension
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go") {
                continue;
            }

            files_to_process.push(file_path.to_path_buf());
        }

        let file_count = files_to_process.len();
        tracing::info!("Parallel parsing {} files with {} workers, then sequential writes", file_count, max_concurrent);

        // Phase 1: Parse files in parallel (CPU-bound, no DB writes)
        let mut futures = FuturesUnordered::new();
        let mut file_iter = files_to_process.into_iter();
        let mut parsed_files: Vec<ParsedFile> = Vec::with_capacity(file_count);

        // Seed the initial batch of parse tasks
        for _ in 0..max_concurrent {
            if let Some(file_path) = file_iter.next() {
                futures.push(tokio::spawn(async move {
                    parse_file_only(&file_path).await
                }));
            }
        }

        // Collect parse results and add new tasks
        while let Some(result) = futures.next().await {
            match result {
                Ok(Ok(parsed)) => parsed_files.push(parsed),
                Ok(Err(e)) => stats.errors.push(format!("Parse error: {}", e)),
                Err(e) => stats.errors.push(format!("Task error: {}", e)),
            }

            // Add next file if available
            if let Some(file_path) = file_iter.next() {
                futures.push(tokio::spawn(async move {
                    parse_file_only(&file_path).await
                }));
            }
        }

        tracing::info!("Parsed {} files, now writing to database sequentially", parsed_files.len());

        // Phase 2: Write to database sequentially (avoids SQLite lock contention)
        let mut indexer = CodeIndexer::with_semantic(db, semantic)?;
        for parsed in parsed_files {
            match indexer.store_parsed_file(parsed).await {
                Ok(file_stats) => stats.merge(file_stats),
                Err(e) => stats.errors.push(format!("Store error: {}", e)),
            }
        }

        Ok(stats)
    }

    /// Delete all data for a file (used when file is deleted)
    pub async fn delete_file(&self, path: &Path) -> Result<()> {
        let file_path_str = path.to_string_lossy().to_string();

        // Delete from SQLite
        sqlx::query("DELETE FROM code_symbols WHERE file_path = $1")
            .bind(&file_path_str)
            .execute(&self.db)
            .await?;

        sqlx::query("DELETE FROM imports WHERE file_path = $1")
            .bind(&file_path_str)
            .execute(&self.db)
            .await?;

        // Delete embeddings from Qdrant
        if let Some(ref semantic) = self.semantic {
            if semantic.is_available() {
                semantic.delete_by_field(
                    crate::tools::COLLECTION_CODE,
                    "file_path",
                    &file_path_str
                ).await?;
            }
        }

        tracing::info!("Deleted index data for {}", file_path_str);
        Ok(())
    }

    /// Index a single file
    pub async fn index_file(&mut self, path: &Path) -> Result<IndexStats> {
        tracing::info!("[INDEX] ENTER index_file: {}", path.display());
        let mut stats = IndexStats::default();
        stats.files_processed = 1;

        tracing::debug!("[INDEX] Reading file content...");
        let content = std::fs::read_to_string(path)?;
        tracing::debug!("[INDEX] File read complete: {} bytes", content.len());
        let content_hash = format!("{:x}", md5_hash(&content));
        let file_path_str = path.to_string_lossy().to_string();

        // Check if file has changed
        tracing::debug!("[INDEX] Checking content hash in DB...");
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT content_hash FROM code_symbols WHERE file_path = $1 LIMIT 1"
        )
        .bind(&file_path_str)
        .fetch_optional(&self.db)
        .await?;
        tracing::debug!("[INDEX] Hash check complete");

        if let Some((existing_hash,)) = existing {
            if existing_hash == content_hash {
                // File unchanged, skip
                tracing::info!("[INDEX] File unchanged, skipping: {}", file_path_str);
                return Ok(stats);
            }
        }

        // File has changed - invalidate any memories scoped to this file
        let _ = invalidate_file_memories(&self.db, &file_path_str).await;

        // Delete old symbols for this file
        tracing::debug!("[INDEX] Deleting old symbols...");
        sqlx::query("DELETE FROM code_symbols WHERE file_path = $1")
            .bind(&file_path_str)
            .execute(&self.db)
            .await?;

        sqlx::query("DELETE FROM imports WHERE file_path = $1")
            .bind(&file_path_str)
            .execute(&self.db)
            .await?;
        tracing::debug!("[INDEX] Old symbols deleted");

        // Delete old embeddings from Qdrant (if available)
        if let Some(ref semantic) = self.semantic {
            if semantic.is_available() {
                tracing::debug!("[INDEX] Deleting old embeddings from Qdrant...");
                if let Err(e) = semantic.delete_by_field(
                    crate::tools::COLLECTION_CODE,
                    "file_path",
                    &file_path_str
                ).await {
                    tracing::warn!("Failed to delete old embeddings for {}: {}", file_path_str, e);
                }
                tracing::debug!("[INDEX] Qdrant delete complete");
            }
        }

        // Parse based on extension
        tracing::debug!("[INDEX] Parsing file with tree-sitter...");
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let (symbols, imports, calls) = match ext {
            "rs" => self.parse_rust(&content)?,
            "py" => self.parse_python(&content)?,
            "ts" | "tsx" => self.parse_typescript(&content)?,
            "js" | "jsx" => self.parse_javascript(&content)?,
            "go" => self.parse_go(&content)?,
            _ => return Ok(stats),
        };
        tracing::debug!("[INDEX] Parse complete: {} symbols, {} imports, {} calls", symbols.len(), imports.len(), calls.len());

        let now = Utc::now().timestamp();

        // Insert symbols
        for symbol in &symbols {
            sqlx::query(r#"
                INSERT INTO code_symbols
                (file_path, name, qualified_name, symbol_type, language, start_line, end_line,
                 signature, visibility, documentation, content_hash, is_test, is_async, analyzed_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#)
            .bind(&file_path_str)
            .bind(&symbol.name)
            .bind(&symbol.qualified_name)
            .bind(&symbol.symbol_type)
            .bind(&symbol.language)
            .bind(symbol.start_line as i32)
            .bind(symbol.end_line as i32)
            .bind(&symbol.signature)
            .bind(&symbol.visibility)
            .bind(&symbol.documentation)
            .bind(&content_hash)
            .bind(symbol.is_test)
            .bind(symbol.is_async)
            .bind(now)
            .execute(&self.db)
            .await?;
        }

        // Insert imports
        for import in &imports {
            let symbols_json = import.imported_symbols.as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or_default());

            sqlx::query(r#"
                INSERT OR IGNORE INTO imports (file_path, import_path, imported_symbols, is_external, analyzed_at)
                VALUES ($1, $2, $3, $4, $5)
            "#)
            .bind(&file_path_str)
            .bind(&import.import_path)
            .bind(&symbols_json)
            .bind(import.is_external)
            .bind(now)
            .execute(&self.db)
            .await?;
        }

        stats.symbols_found = symbols.len();
        stats.imports_found = imports.len();

        // Insert call graph relationships
        // First, delete existing call graph entries for symbols in this file
        sqlx::query(r#"
            DELETE FROM call_graph WHERE caller_id IN (
                SELECT id FROM code_symbols WHERE file_path = $1
            )
        "#)
        .bind(&file_path_str)
        .execute(&self.db)
        .await?;

        // Build a map of symbol names to their IDs for this file
        let symbol_ids: Vec<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT id, name, qualified_name FROM code_symbols WHERE file_path = $1"
        )
        .bind(&file_path_str)
        .fetch_all(&self.db)
        .await?;

        let symbol_map: std::collections::HashMap<String, i64> = symbol_ids.iter()
            .flat_map(|(id, name, qname)| {
                let mut entries = vec![(name.clone(), *id)];
                if let Some(q) = qname {
                    entries.push((q.clone(), *id));
                }
                entries
            })
            .collect();

        // Delete existing unresolved calls for this file's symbols
        sqlx::query(r#"
            DELETE FROM unresolved_calls WHERE caller_id IN (
                SELECT id FROM code_symbols WHERE file_path = $1
            )
        "#)
        .bind(&file_path_str)
        .execute(&self.db)
        .await?;

        // Insert calls where we can resolve the caller
        let mut calls_inserted = 0;
        let mut unresolved_inserted = 0;
        for call in &calls {
            // Find caller ID
            let caller_id = symbol_map.get(&call.caller_name);

            if let Some(&caller_id) = caller_id {
                // Try to find callee ID (might not exist if external)
                // First try exact match, then try just the function name part
                let callee_name = call.callee_name.split("::").last().unwrap_or(&call.callee_name);

                let callee_id: Option<(i64,)> = sqlx::query_as(
                    "SELECT id FROM code_symbols WHERE name = $1 OR qualified_name LIKE $2 LIMIT 1"
                )
                .bind(callee_name)
                .bind(format!("%{}", call.callee_name))
                .fetch_optional(&self.db)
                .await?;

                if let Some((callee_id,)) = callee_id {
                    // Insert the resolved call relationship (with callee_name for searching)
                    let result = sqlx::query(r#"
                        INSERT OR IGNORE INTO call_graph (caller_id, callee_id, call_type, call_line, callee_name)
                        VALUES ($1, $2, $3, $4, $5)
                    "#)
                    .bind(caller_id)
                    .bind(callee_id)
                    .bind(&call.call_type)
                    .bind(call.call_line as i32)
                    .bind(&call.callee_name)
                    .execute(&self.db)
                    .await;

                    if result.is_ok() {
                        calls_inserted += 1;
                    }
                } else {
                    // Skip common stdlib/builtin method calls that will never resolve
                    // These add noise without value
                    let skip_methods = [
                        // Rust common methods
                        "unwrap", "unwrap_or", "unwrap_or_default", "unwrap_or_else",
                        "expect", "ok", "err", "is_ok", "is_err", "is_some", "is_none",
                        "map", "map_err", "and_then", "or_else", "filter", "flatten",
                        "collect", "iter", "into_iter", "enumerate", "zip", "chain",
                        "take", "skip", "first", "last", "get", "get_mut",
                        "push", "pop", "insert", "remove", "clear", "len", "is_empty",
                        "clone", "to_string", "to_owned", "as_ref", "as_mut",
                        "into", "from", "try_into", "try_from",
                        "bind", "fetch_all", "fetch_one", "fetch_optional", "execute",
                        "send", "recv", "await", "spawn", "block_on",
                        "min", "max", "min_by", "max_by", "sum", "product",
                        "join", "split", "trim", "contains", "starts_with", "ends_with",
                        "format", "write", "read", "flush",
                        // Common trait methods
                        "default", "new", "build", "with",
                    ];

                    let callee_short = call.callee_name.split("::").last().unwrap_or(&call.callee_name);
                    if skip_methods.contains(&callee_short) {
                        continue;
                    }

                    // Store as unresolved for later resolution
                    let result = sqlx::query(r#"
                        INSERT OR IGNORE INTO unresolved_calls (caller_id, callee_name, call_type, call_line)
                        VALUES ($1, $2, $3, $4)
                    "#)
                    .bind(caller_id)
                    .bind(&call.callee_name)
                    .bind(&call.call_type)
                    .bind(call.call_line as i32)
                    .execute(&self.db)
                    .await;

                    if result.is_ok() {
                        unresolved_inserted += 1;
                    }
                }
            }
        }

        // Try to resolve any pending unresolved calls that might now be resolvable
        tracing::debug!("[INDEX] Resolving pending calls...");
        let resolved = self.resolve_pending_calls().await.unwrap_or(0);
        if resolved > 0 {
            tracing::debug!("Resolved {} previously unresolved calls", resolved);
        }
        tracing::debug!("[INDEX] Pending calls resolved");

        stats.calls_found = calls_inserted;
        stats.unresolved_calls = unresolved_inserted;

        // Generate embeddings for semantic search (if available)
        // Use batch embedding for better performance
        if let Some(ref semantic) = self.semantic {
            if semantic.is_available() {
                tracing::debug!("[INDEX] Ensuring Qdrant collection exists...");
                // Ensure collection exists
                if let Err(e) = semantic.ensure_collection(crate::tools::COLLECTION_CODE).await {
                    tracing::warn!("Failed to ensure code collection: {}", e);
                } else {
                    tracing::debug!("[INDEX] Collection ensured");
                    // Collect embeddable symbols
                    let embeddable: Vec<_> = symbols.iter()
                        .filter(|s| matches!(s.symbol_type.as_str(),
                            "function" | "struct" | "class" | "trait" | "enum" | "interface" | "type"))
                        .collect();

                    if !embeddable.is_empty() {
                        tracing::debug!("[INDEX] Building {} embeddable items...", embeddable.len());
                        // Build batch items: (id, content, metadata)
                        let batch_items: Vec<_> = embeddable.iter().map(|symbol| {
                            let text = Self::symbol_to_text(symbol, &file_path_str);
                            let id = Self::symbol_id(&file_path_str, symbol);

                            let mut metadata = std::collections::HashMap::new();
                            metadata.insert("file_path".to_string(), serde_json::json!(file_path_str.clone()));
                            metadata.insert("symbol_name".to_string(), serde_json::json!(symbol.name.clone()));
                            metadata.insert("symbol_type".to_string(), serde_json::json!(symbol.symbol_type.clone()));
                            metadata.insert("language".to_string(), serde_json::json!(symbol.language.clone()));
                            metadata.insert("start_line".to_string(), serde_json::json!(symbol.start_line));
                            metadata.insert("end_line".to_string(), serde_json::json!(symbol.end_line));

                            if let Some(ref sig) = symbol.signature {
                                metadata.insert("signature".to_string(), serde_json::json!(sig.clone()));
                            }

                            (id, text, metadata)
                        }).collect();

                        // Store all symbols in one batch operation
                        tracing::info!("[INDEX] Calling store_batch with {} items (Gemini API + Qdrant)...", batch_items.len());
                        match semantic.store_batch(crate::tools::COLLECTION_CODE, batch_items).await {
                            Ok(count) => {
                                tracing::info!("[INDEX] store_batch complete: {} embeddings generated", count);
                                stats.embeddings_generated = count;
                            }
                            Err(e) => {
                                tracing::warn!("Batch embedding failed: {}", e);
                            }
                        }
                    }
                }
            }
        }

        tracing::info!("[INDEX] EXIT index_file: {} - {} symbols, {} embeddings",
            path.display(), stats.symbols_found, stats.embeddings_generated);
        Ok(stats)
    }

    /// Store pre-parsed file data to database (used by parallel indexer)
    pub async fn store_parsed_file(&mut self, parsed: ParsedFile) -> Result<IndexStats> {
        let mut stats = IndexStats::default();
        stats.files_processed = 1;

        let file_path_str = parsed.path.to_string_lossy().to_string();

        // Check if file has changed
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT content_hash FROM code_symbols WHERE file_path = $1 LIMIT 1"
        )
        .bind(&file_path_str)
        .fetch_optional(&self.db)
        .await?;

        if let Some((existing_hash,)) = existing {
            if existing_hash == parsed.content_hash {
                // File unchanged, skip
                return Ok(stats);
            }
        }

        // File has changed - invalidate any memories scoped to this file
        let _ = invalidate_file_memories(&self.db, &file_path_str).await;

        // Delete old symbols for this file
        sqlx::query("DELETE FROM code_symbols WHERE file_path = $1")
            .bind(&file_path_str)
            .execute(&self.db)
            .await?;

        sqlx::query("DELETE FROM imports WHERE file_path = $1")
            .bind(&file_path_str)
            .execute(&self.db)
            .await?;

        // Delete old embeddings from Qdrant (if available)
        if let Some(ref semantic) = self.semantic {
            if semantic.is_available() {
                if let Err(e) = semantic.delete_by_field(
                    crate::tools::COLLECTION_CODE,
                    "file_path",
                    &file_path_str
                ).await {
                    tracing::warn!("Failed to delete old embeddings for {}: {}", file_path_str, e);
                }
            }
        }

        let now = chrono::Utc::now().timestamp();

        // Insert symbols
        for symbol in &parsed.symbols {
            sqlx::query(r#"
                INSERT INTO code_symbols
                (file_path, name, qualified_name, symbol_type, language, start_line, end_line,
                 signature, visibility, documentation, content_hash, is_test, is_async, analyzed_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#)
            .bind(&file_path_str)
            .bind(&symbol.name)
            .bind(&symbol.qualified_name)
            .bind(&symbol.symbol_type)
            .bind(&symbol.language)
            .bind(symbol.start_line as i32)
            .bind(symbol.end_line as i32)
            .bind(&symbol.signature)
            .bind(&symbol.visibility)
            .bind(&symbol.documentation)
            .bind(&parsed.content_hash)
            .bind(symbol.is_test)
            .bind(symbol.is_async)
            .bind(now)
            .execute(&self.db)
            .await?;
        }

        // Insert imports
        for import in &parsed.imports {
            let symbols_json = import.imported_symbols.as_ref()
                .map(|s| serde_json::to_string(s).unwrap_or_default());

            sqlx::query(r#"
                INSERT OR IGNORE INTO imports (file_path, import_path, imported_symbols, is_external, analyzed_at)
                VALUES ($1, $2, $3, $4, $5)
            "#)
            .bind(&file_path_str)
            .bind(&import.import_path)
            .bind(&symbols_json)
            .bind(import.is_external)
            .bind(now)
            .execute(&self.db)
            .await?;
        }

        stats.symbols_found = parsed.symbols.len();
        stats.imports_found = parsed.imports.len();

        // Insert call graph (simplified - just count, skip complex resolution for parallel)
        stats.calls_found = parsed.calls.len();

        // Generate embeddings for semantic search (if available)
        if let Some(ref semantic) = self.semantic {
            if semantic.is_available() {
                if let Err(e) = semantic.ensure_collection(crate::tools::COLLECTION_CODE).await {
                    tracing::warn!("Failed to ensure code collection: {}", e);
                } else {
                    let embeddable: Vec<_> = parsed.symbols.iter()
                        .filter(|s| matches!(s.symbol_type.as_str(),
                            "function" | "struct" | "class" | "trait" | "enum" | "interface" | "type"))
                        .collect();

                    if !embeddable.is_empty() {
                        let batch_items: Vec<_> = embeddable.iter().map(|symbol| {
                            let text = Self::symbol_to_text(symbol, &file_path_str);
                            let id = Self::symbol_id(&file_path_str, symbol);

                            let mut metadata = std::collections::HashMap::new();
                            metadata.insert("file_path".to_string(), serde_json::json!(file_path_str.clone()));
                            metadata.insert("symbol_name".to_string(), serde_json::json!(symbol.name.clone()));
                            metadata.insert("symbol_type".to_string(), serde_json::json!(symbol.symbol_type.clone()));
                            metadata.insert("language".to_string(), serde_json::json!(symbol.language.clone()));
                            metadata.insert("start_line".to_string(), serde_json::json!(symbol.start_line));
                            metadata.insert("end_line".to_string(), serde_json::json!(symbol.end_line));

                            if let Some(ref sig) = symbol.signature {
                                metadata.insert("signature".to_string(), serde_json::json!(sig.clone()));
                            }

                            (id, text, metadata)
                        }).collect();

                        match semantic.store_batch(crate::tools::COLLECTION_CODE, batch_items).await {
                            Ok(count) => {
                                stats.embeddings_generated = count;
                            }
                            Err(e) => {
                                tracing::warn!("Batch embedding failed: {}", e);
                            }
                        }
                    }
                }
            }
        }

        Ok(stats)
    }

    fn parse_rust(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        parsers::rust::parse(&mut self.rust_parser, content)
    }

    fn parse_python(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        parsers::python::parse(&mut self.python_parser, content)
    }

    fn parse_typescript(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        parsers::typescript::parse(&mut self.typescript_parser, content)
    }

    fn parse_javascript(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        parsers::typescript::parse_javascript(&mut self.javascript_parser, content)
    }

    fn parse_go(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        parsers::go::parse(&mut self.go_parser, content)
    }

    /// Try to resolve pending unresolved calls against newly indexed symbols
    async fn resolve_pending_calls(&self) -> Result<usize> {
        let mut resolved_count = 0;

        // Get all unresolved calls
        let unresolved: Vec<(i64, i64, String, Option<String>, Option<i32>)> = sqlx::query_as(
            r#"
            SELECT uc.id, uc.caller_id, uc.callee_name, uc.call_type, uc.call_line
            FROM unresolved_calls uc
            "#
        )
        .fetch_all(&self.db)
        .await?;

        for (unresolved_id, caller_id, callee_name, call_type, call_line) in unresolved {
            // Try to find the callee now
            let callee_short = callee_name.split("::").last().unwrap_or(&callee_name);
            let callee_pattern = format!("%{}", callee_name);

            let callee_id: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM code_symbols WHERE name = $1 OR qualified_name LIKE $2 LIMIT 1"
            )
            .bind(callee_short)
            .bind(&callee_pattern)
            .fetch_optional(&self.db)
            .await?;

            if let Some((callee_id,)) = callee_id {
                // Insert the resolved call
                let insert_result = sqlx::query(r#"
                    INSERT OR IGNORE INTO call_graph (caller_id, callee_id, call_type, call_line, callee_name)
                    VALUES ($1, $2, $3, $4, $5)
                "#)
                .bind(caller_id)
                .bind(callee_id)
                .bind(&call_type)
                .bind(call_line)
                .bind(&callee_name)
                .execute(&self.db)
                .await;

                if insert_result.is_ok() {
                    // Delete from unresolved
                    sqlx::query("DELETE FROM unresolved_calls WHERE id = $1")
                        .bind(unresolved_id)
                        .execute(&self.db)
                        .await?;
                    resolved_count += 1;
                }
            }
        }

        Ok(resolved_count)
    }
}


fn md5_hash(content: &str) -> u128 {
    // Simple hash - not cryptographic, just for change detection
    let mut hash: u128 = 0;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u128);
    }
    hash
}

/// Invalidate file-scoped memories when a file is re-indexed
///
/// Marks memories referencing this file as 'stale' (but NOT decisions/preferences).
/// This ensures code-related context gets downranked after file changes.
pub async fn invalidate_file_memories(db: &SqlitePool, file_path: &str) -> Result<usize> {
    let result = sqlx::query(
        r#"
        UPDATE memory_facts
        SET validity = 'stale', updated_at = $2
        WHERE file_path = $1
          AND validity = 'active'
          AND fact_type NOT IN ('decision', 'preference')
        "#,
    )
    .bind(file_path)
    .bind(Utc::now().timestamp())
    .execute(db)
    .await?;

    let count = result.rows_affected() as usize;
    if count > 0 {
        tracing::debug!("Invalidated {} file-scoped memories for {}", count, file_path);
    }
    Ok(count)
}

/// Standalone file indexer for parallel processing
/// Creates its own parser instance since tree-sitter parsers require &mut self
#[allow(dead_code)] // Used by parallel indexer internally
pub async fn index_file_standalone(
    db: SqlitePool,
    semantic: Option<Arc<SemanticSearch>>,
    path: &Path,
) -> Result<IndexStats> {
    let mut indexer = CodeIndexer::with_semantic(db, semantic)?;
    indexer.index_file(path).await
}

/// Parse a file without storing (for parallel parse phase)
/// Creates its own parser, reads file, returns parsed data
async fn parse_file_only(path: &Path) -> Result<ParsedFile> {
    let path = path.to_path_buf();

    // Use spawn_blocking since parsing is CPU-bound
    tokio::task::spawn_blocking(move || {
        parse_file_sync(&path)
    }).await?
}

/// Synchronous file parsing (runs in spawn_blocking)
fn parse_file_sync(path: &Path) -> Result<ParsedFile> {
    let content = std::fs::read_to_string(path)?;
    let content_hash = format!("{:x}", md5_hash(&content));

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    // Create parser for this extension and parse using the parsers module
    let mut parser = Parser::new();
    let (symbols, imports, calls) = match ext {
        "rs" => {
            parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
            parsers::rust::parse(&mut parser, &content)?
        }
        "py" => {
            parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
            parsers::python::parse(&mut parser, &content)?
        }
        "ts" | "tsx" => {
            parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
            parsers::typescript::parse(&mut parser, &content)?
        }
        "js" | "jsx" => {
            parser.set_language(&tree_sitter_javascript::LANGUAGE.into())?;
            parsers::typescript::parse_javascript(&mut parser, &content)?
        }
        "go" => {
            parser.set_language(&tree_sitter_go::LANGUAGE.into())?;
            parsers::go::parse(&mut parser, &content)?
        }
        _ => (Vec::new(), Vec::new(), Vec::new()),
    };

    Ok(ParsedFile {
        path: path.to_path_buf(),
        content_hash,
        symbols,
        imports,
        calls,
    })
}
