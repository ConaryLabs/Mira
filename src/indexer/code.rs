// src/indexer/code.rs
// Code symbol extraction using tree-sitter

use std::path::Path;
use anyhow::{anyhow, Result};
use sqlx::sqlite::SqlitePool;
use chrono::Utc;
use walkdir::WalkDir;
use ignore::gitignore::Gitignore;
use tree_sitter::{Parser, Node};

use super::IndexStats;
use crate::tools::SemanticSearch;
use std::sync::Arc;

/// Extracted symbol from source code
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub qualified_name: Option<String>,
    pub symbol_type: String,
    pub language: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: Option<String>,
    pub visibility: Option<String>,
    pub documentation: Option<String>,
    pub is_test: bool,
    pub is_async: bool,
}

/// Extracted import statement
#[derive(Debug, Clone)]
pub struct Import {
    pub import_path: String,
    pub imported_symbols: Option<Vec<String>>,
    pub is_external: bool,
}

/// Extracted function call (for call graph)
#[derive(Debug, Clone)]
pub struct FunctionCall {
    pub caller_name: String,
    pub callee_name: String,
    pub call_line: u32,
    pub call_type: String, // "direct", "method", "async"
}

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

    /// Index all code files in a directory
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
        let mut stats = IndexStats::default();
        stats.files_processed = 1;

        let content = std::fs::read_to_string(path)?;
        let content_hash = format!("{:x}", md5_hash(&content));
        let file_path_str = path.to_string_lossy().to_string();

        // Check if file has changed
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT content_hash FROM code_symbols WHERE file_path = $1 LIMIT 1"
        )
        .bind(&file_path_str)
        .fetch_optional(&self.db)
        .await?;

        if let Some((existing_hash,)) = existing {
            if existing_hash == content_hash {
                // File unchanged, skip
                return Ok(stats);
            }
        }

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

        // Parse based on extension
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let (symbols, imports, calls) = match ext {
            "rs" => self.parse_rust(&content)?,
            "py" => self.parse_python(&content)?,
            "ts" | "tsx" => self.parse_typescript(&content)?,
            "js" | "jsx" => self.parse_javascript(&content)?,
            "go" => self.parse_go(&content)?,
            _ => return Ok(stats),
        };

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
        let resolved = self.resolve_pending_calls().await.unwrap_or(0);
        if resolved > 0 {
            tracing::debug!("Resolved {} previously unresolved calls", resolved);
        }

        stats.calls_found = calls_inserted;
        stats.unresolved_calls = unresolved_inserted;

        // Generate embeddings for semantic search (if available)
        if let Some(ref semantic) = self.semantic {
            if semantic.is_available() {
                // Ensure collection exists
                if let Err(e) = semantic.ensure_collection(crate::tools::COLLECTION_CODE).await {
                    tracing::warn!("Failed to ensure code collection: {}", e);
                } else {
                    // Embed each symbol (functions, structs, classes - skip modules/imports)
                    for symbol in &symbols {
                        // Only embed meaningful symbols
                        if matches!(symbol.symbol_type.as_str(),
                            "function" | "struct" | "class" | "trait" | "enum" | "interface" | "type")
                        {
                            let text = Self::symbol_to_text(symbol, &file_path_str);
                            let id = Self::symbol_id(&file_path_str, symbol);

                            let mut metadata = std::collections::HashMap::new();
                            metadata.insert("file_path".to_string(), serde_json::json!(file_path_str.clone()));
                            metadata.insert("name".to_string(), serde_json::json!(symbol.name.clone()));
                            metadata.insert("symbol_type".to_string(), serde_json::json!(symbol.symbol_type.clone()));
                            metadata.insert("language".to_string(), serde_json::json!(symbol.language.clone()));
                            metadata.insert("start_line".to_string(), serde_json::json!(symbol.start_line));
                            metadata.insert("end_line".to_string(), serde_json::json!(symbol.end_line));

                            if let Some(ref sig) = symbol.signature {
                                metadata.insert("signature".to_string(), serde_json::json!(sig.clone()));
                            }

                            if let Err(e) = semantic.store(
                                crate::tools::COLLECTION_CODE,
                                &id,
                                &text,
                                metadata,
                            ).await {
                                tracing::warn!("Failed to embed symbol {}: {}", symbol.name, e);
                            } else {
                                stats.embeddings_generated += 1;
                            }
                        }
                    }
                }
            }
        }

        Ok(stats)
    }

    fn parse_rust(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        let tree = self.rust_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse Rust code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut calls = Vec::new();
        let bytes = content.as_bytes();

        self.walk_rust_node(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None);

        Ok((symbols, imports, calls))
    }

    fn walk_rust_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        calls: &mut Vec<FunctionCall>,
        parent_name: Option<&str>,
        current_function: Option<&str>,
    ) {
        match node.kind() {
            "function_item" | "function_signature_item" => {
                if let Some(sym) = self.extract_rust_function(node, source, parent_name) {
                    let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                    symbols.push(sym);
                    // Walk function body with this function as context
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_rust_node(child, source, symbols, imports, calls, parent_name, Some(&func_name));
                        }
                    }
                    return;
                }
            }
            "struct_item" => {
                if let Some(sym) = self.extract_rust_struct(node, source) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    // Walk children for impl methods
                    for child in node.children(&mut node.walk()) {
                        self.walk_rust_node(child, source, symbols, imports, calls, Some(&name), current_function);
                    }
                    return;
                }
            }
            "enum_item" => {
                if let Some(sym) = self.extract_rust_enum(node, source) {
                    symbols.push(sym);
                }
            }
            "trait_item" => {
                if let Some(sym) = self.extract_rust_trait(node, source) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    for child in node.children(&mut node.walk()) {
                        self.walk_rust_node(child, source, symbols, imports, calls, Some(&name), current_function);
                    }
                    return;
                }
            }
            "impl_item" => {
                // Get the type being implemented
                let type_name = node.child_by_field_name("type")
                    .map(|n| node_text(n, source));
                for child in node.children(&mut node.walk()) {
                    self.walk_rust_node(child, source, symbols, imports, calls, type_name.as_deref(), current_function);
                }
                return;
            }
            "const_item" | "static_item" => {
                if let Some(sym) = self.extract_rust_const(node, source) {
                    symbols.push(sym);
                }
            }
            "use_declaration" => {
                if let Some(import) = self.extract_rust_use(node, source) {
                    imports.push(import);
                }
            }
            "mod_item" => {
                if let Some(sym) = self.extract_rust_mod(node, source) {
                    symbols.push(sym);
                }
            }
            "call_expression" => {
                // Extract function call if we're inside a function
                if let Some(caller) = current_function {
                    if let Some(call) = self.extract_rust_call(node, source, caller) {
                        calls.push(call);
                    }
                }
            }
            "macro_invocation" => {
                // Extract macro calls (like println!, vec!, etc.)
                if let Some(caller) = current_function {
                    if let Some(call) = self.extract_rust_macro_call(node, source, caller) {
                        calls.push(call);
                    }
                }
            }
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_rust_node(child, source, symbols, imports, calls, parent_name, current_function);
        }
    }

    fn extract_rust_call(&self, node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
        // Get the function being called
        let function_node = node.child_by_field_name("function")?;
        let callee_name = match function_node.kind() {
            "identifier" => node_text(function_node, source),
            "field_expression" => {
                // method call: obj.method() - extract method name
                function_node.child_by_field_name("field")
                    .map(|n| node_text(n, source))?
            }
            "scoped_identifier" => {
                // Type::method() - extract full path
                node_text(function_node, source)
            }
            _ => return None,
        };

        // Determine call type
        let call_type = if function_node.kind() == "field_expression" {
            "method"
        } else if callee_name.contains("::") {
            "static"
        } else {
            "direct"
        };

        Some(FunctionCall {
            caller_name: caller.to_string(),
            callee_name,
            call_line: node.start_position().row as u32 + 1,
            call_type: call_type.to_string(),
        })
    }

    fn extract_rust_macro_call(&self, node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
        // Get macro name
        let macro_node = node.child_by_field_name("macro")
            .or_else(|| node.child(0))?;
        let macro_name = node_text(macro_node, source);

        // Skip common low-value macros
        if matches!(macro_name.as_str(), "println" | "print" | "eprintln" | "eprint" |
                    "format" | "write" | "writeln" | "panic" | "todo" | "unimplemented" |
                    "assert" | "assert_eq" | "assert_ne" | "debug_assert" | "debug_assert_eq") {
            return None;
        }

        Some(FunctionCall {
            caller_name: caller.to_string(),
            callee_name: format!("{}!", macro_name),
            call_line: node.start_position().row as u32 + 1,
            call_type: "macro".to_string(),
        })
    }

    fn extract_rust_function(&self, node: Node, source: &[u8], parent: Option<&str>) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let qualified_name = parent.map(|p| format!("{}::{}", p, name));

        // Get visibility
        let visibility = node.children(&mut node.walk())
            .find(|n| n.kind() == "visibility_modifier")
            .map(|n| node_text(n, source));

        // Check for async
        let is_async = node.children(&mut node.walk())
            .any(|n| n.kind() == "async");

        // Check for test attribute
        let is_test = self.has_test_attribute(node, source);

        // Get signature (parameters + return type)
        let params = node.child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or_default();
        let return_type = node.child_by_field_name("return_type")
            .map(|n| node_text(n, source));
        let signature = if let Some(ret) = return_type {
            format!("{} {}", params, ret)
        } else {
            params
        };

        // Get doc comments (preceding line_comment or block_comment with ///)
        let documentation = self.get_rust_doc_comment(node, source);

        Some(Symbol {
            name,
            qualified_name,
            symbol_type: "function".to_string(),
            language: "rust".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: Some(signature),
            visibility,
            documentation,
            is_test,
            is_async,
        })
    }

    fn extract_rust_struct(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let visibility = node.children(&mut node.walk())
            .find(|n| n.kind() == "visibility_modifier")
            .map(|n| node_text(n, source));

        let documentation = self.get_rust_doc_comment(node, source);

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "struct".to_string(),
            language: "rust".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility,
            documentation,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_rust_enum(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let visibility = node.children(&mut node.walk())
            .find(|n| n.kind() == "visibility_modifier")
            .map(|n| node_text(n, source));

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "enum".to_string(),
            language: "rust".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility,
            documentation: self.get_rust_doc_comment(node, source),
            is_test: false,
            is_async: false,
        })
    }

    fn extract_rust_trait(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let visibility = node.children(&mut node.walk())
            .find(|n| n.kind() == "visibility_modifier")
            .map(|n| node_text(n, source));

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "trait".to_string(),
            language: "rust".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility,
            documentation: self.get_rust_doc_comment(node, source),
            is_test: false,
            is_async: false,
        })
    }

    fn extract_rust_const(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let visibility = node.children(&mut node.walk())
            .find(|n| n.kind() == "visibility_modifier")
            .map(|n| node_text(n, source));

        let type_node = node.child_by_field_name("type")
            .map(|n| node_text(n, source));

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: if node.kind() == "const_item" { "const" } else { "static" }.to_string(),
            language: "rust".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: type_node,
            visibility,
            documentation: None,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_rust_mod(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let visibility = node.children(&mut node.walk())
            .find(|n| n.kind() == "visibility_modifier")
            .map(|n| node_text(n, source));

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "module".to_string(),
            language: "rust".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility,
            documentation: None,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_rust_use(&self, node: Node, source: &[u8]) -> Option<Import> {
        // Get the use path
        let path = node.child_by_field_name("argument")
            .map(|n| node_text(n, source))?;

        // Determine if external (doesn't start with crate::, self::, super::)
        let is_external = !path.starts_with("crate::")
            && !path.starts_with("self::")
            && !path.starts_with("super::");

        Some(Import {
            import_path: path,
            imported_symbols: None,
            is_external,
        })
    }

    fn has_test_attribute(&self, node: Node, source: &[u8]) -> bool {
        // Look for #[test] or #[cfg(test)] in preceding siblings
        if let Some(parent) = node.parent() {
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "attribute_item" {
                    let text = node_text(child, source);
                    if text.contains("test") {
                        return true;
                    }
                }
                if child.id() == node.id() {
                    break;
                }
            }
        }
        false
    }

    fn get_rust_doc_comment(&self, node: Node, source: &[u8]) -> Option<String> {
        let mut docs = Vec::new();

        // Look at preceding siblings for doc comments
        if let Some(parent) = node.parent() {
            let mut found_node = false;
            for child in parent.children(&mut parent.walk()).collect::<Vec<_>>().into_iter().rev() {
                if child.id() == node.id() {
                    found_node = true;
                    continue;
                }
                if !found_node {
                    continue;
                }

                if child.kind() == "line_comment" {
                    let text = node_text(child, source);
                    if text.starts_with("///") || text.starts_with("//!") {
                        docs.push(text.trim_start_matches('/').trim().to_string());
                    } else {
                        break;
                    }
                } else if child.kind() == "attribute_item" {
                    // Skip attributes
                    continue;
                } else {
                    break;
                }
            }
        }

        if docs.is_empty() {
            None
        } else {
            docs.reverse();
            Some(docs.join("\n"))
        }
    }

    fn parse_python(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        let tree = self.python_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse Python code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut calls = Vec::new();
        let bytes = content.as_bytes();

        self.walk_python_node(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None);

        Ok((symbols, imports, calls))
    }

    fn walk_python_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        calls: &mut Vec<FunctionCall>,
        parent_name: Option<&str>,
        current_function: Option<&str>,
    ) {
        match node.kind() {
            "function_definition" => {
                if let Some(sym) = self.extract_python_function(node, source, parent_name) {
                    let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                    symbols.push(sym);
                    // Walk function body with this function as context
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_python_node(child, source, symbols, imports, calls, parent_name, Some(&func_name));
                        }
                    }
                    return;
                }
            }
            "class_definition" => {
                if let Some(sym) = self.extract_python_class(node, source) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    // Walk children for methods
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_python_node(child, source, symbols, imports, calls, Some(&name), current_function);
                        }
                    }
                    return;
                }
            }
            "import_statement" | "import_from_statement" => {
                if let Some(import) = self.extract_python_import(node, source) {
                    imports.push(import);
                }
            }
            "call" => {
                // Extract function call if we're inside a function
                if let Some(caller) = current_function {
                    if let Some(call) = self.extract_python_call(node, source, caller) {
                        calls.push(call);
                    }
                }
            }
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_python_node(child, source, symbols, imports, calls, parent_name, current_function);
        }
    }

    fn extract_python_call(&self, node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
        // Get the function being called
        let function_node = node.child_by_field_name("function")?;
        let callee_name = match function_node.kind() {
            "identifier" => node_text(function_node, source),
            "attribute" => {
                // method call: obj.method() - extract method name
                function_node.child_by_field_name("attribute")
                    .map(|n| node_text(n, source))?
            }
            _ => return None,
        };

        // Skip common builtins
        if matches!(callee_name.as_str(), "print" | "len" | "str" | "int" | "float" |
                    "list" | "dict" | "set" | "tuple" | "range" | "enumerate" | "zip" |
                    "open" | "type" | "isinstance" | "hasattr" | "getattr" | "setattr") {
            return None;
        }

        // Determine call type
        let call_type = if function_node.kind() == "attribute" {
            "method"
        } else {
            "direct"
        };

        Some(FunctionCall {
            caller_name: caller.to_string(),
            callee_name,
            call_line: node.start_position().row as u32 + 1,
            call_type: call_type.to_string(),
        })
    }

    fn extract_python_function(&self, node: Node, source: &[u8], parent: Option<&str>) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let qualified_name = parent.map(|p| format!("{}.{}", p, name));

        let params = node.child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or_default();

        let return_type = node.child_by_field_name("return_type")
            .map(|n| node_text(n, source));

        let signature = if let Some(ret) = return_type {
            format!("{} -> {}", params, ret)
        } else {
            params
        };

        let is_async = node.kind() == "function_definition"
            && node.children(&mut node.walk()).any(|n| n.kind() == "async");

        // Check for test (name starts with test_)
        let is_test = name.starts_with("test_");

        Some(Symbol {
            name,
            qualified_name,
            symbol_type: "function".to_string(),
            language: "python".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: Some(signature),
            visibility: None,
            documentation: self.get_python_docstring(node, source),
            is_test,
            is_async,
        })
    }

    fn extract_python_class(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "class".to_string(),
            language: "python".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility: None,
            documentation: self.get_python_docstring(node, source),
            is_test: false,
            is_async: false,
        })
    }

    fn extract_python_import(&self, node: Node, source: &[u8]) -> Option<Import> {
        let text = node_text(node, source);

        // Parse "import x" or "from x import y"
        let import_path = if node.kind() == "import_from_statement" {
            node.child_by_field_name("module_name")
                .map(|n| node_text(n, source))
                .unwrap_or_else(|| text.clone())
        } else {
            node.children(&mut node.walk())
                .find(|n| n.kind() == "dotted_name")
                .map(|n| node_text(n, source))
                .unwrap_or_else(|| text.clone())
        };

        Some(Import {
            import_path,
            imported_symbols: None,
            is_external: true, // Assume external for Python
        })
    }

    fn get_python_docstring(&self, node: Node, source: &[u8]) -> Option<String> {
        // Look for string as first child of body
        if let Some(body) = node.child_by_field_name("body") {
            if let Some(first_child) = body.child(0) {
                if first_child.kind() == "expression_statement" {
                    if let Some(string_node) = first_child.child(0) {
                        if string_node.kind() == "string" {
                            let text = node_text(string_node, source);
                            // Strip quotes
                            return Some(text.trim_matches('"').trim_matches('\'').to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn parse_typescript(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        let tree = self.typescript_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse TypeScript code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut calls = Vec::new();
        let bytes = content.as_bytes();

        self.walk_ts_node(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None, "typescript");

        Ok((symbols, imports, calls))
    }

    fn parse_javascript(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        let tree = self.javascript_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse JavaScript code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut calls = Vec::new();
        let bytes = content.as_bytes();

        self.walk_ts_node(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None, "javascript");

        Ok((symbols, imports, calls))
    }

    fn walk_ts_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        calls: &mut Vec<FunctionCall>,
        parent_name: Option<&str>,
        current_function: Option<&str>,
        language: &str,
    ) {
        match node.kind() {
            "function_declaration" | "method_definition" | "arrow_function" => {
                if let Some(sym) = self.extract_ts_function(node, source, parent_name, language) {
                    let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                    symbols.push(sym);
                    // Walk function body with this function as context
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_ts_node(child, source, symbols, imports, calls, parent_name, Some(&func_name), language);
                        }
                    }
                    return;
                }
            }
            "class_declaration" => {
                if let Some(sym) = self.extract_ts_class(node, source, language) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    // Walk children for methods
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_ts_node(child, source, symbols, imports, calls, Some(&name), current_function, language);
                        }
                    }
                    return;
                }
            }
            "interface_declaration" => {
                if let Some(sym) = self.extract_ts_interface(node, source, language) {
                    symbols.push(sym);
                }
            }
            "type_alias_declaration" => {
                if let Some(sym) = self.extract_ts_type_alias(node, source, language) {
                    symbols.push(sym);
                }
            }
            "import_statement" => {
                if let Some(import) = self.extract_ts_import(node, source) {
                    imports.push(import);
                }
            }
            "call_expression" => {
                // Extract function call if we're inside a function
                if let Some(caller) = current_function {
                    if let Some(call) = self.extract_ts_call(node, source, caller) {
                        calls.push(call);
                    }
                }
            }
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_ts_node(child, source, symbols, imports, calls, parent_name, current_function, language);
        }
    }

    fn extract_ts_call(&self, node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
        // Get the function being called
        let function_node = node.child_by_field_name("function")?;
        let callee_name = match function_node.kind() {
            "identifier" => node_text(function_node, source),
            "member_expression" => {
                // method call: obj.method() - extract method name
                function_node.child_by_field_name("property")
                    .map(|n| node_text(n, source))?
            }
            _ => return None,
        };

        // Skip common builtins/console methods
        if matches!(callee_name.as_str(), "log" | "error" | "warn" | "info" | "debug" |
                    "toString" | "valueOf" | "push" | "pop" | "shift" | "unshift" |
                    "map" | "filter" | "reduce" | "forEach" | "find" | "some" | "every" |
                    "slice" | "splice" | "concat" | "join" | "split" | "trim" |
                    "parseInt" | "parseFloat" | "setTimeout" | "setInterval" | "clearTimeout" |
                    "require" | "import") {
            return None;
        }

        // Determine call type
        let call_type = if function_node.kind() == "member_expression" {
            "method"
        } else {
            "direct"
        };

        Some(FunctionCall {
            caller_name: caller.to_string(),
            callee_name,
            call_line: node.start_position().row as u32 + 1,
            call_type: call_type.to_string(),
        })
    }

    fn extract_ts_function(&self, node: Node, source: &[u8], parent: Option<&str>, language: &str) -> Option<Symbol> {
        let name = node.child_by_field_name("name")
            .map(|n| node_text(n, source))
            .or_else(|| {
                // For arrow functions assigned to variables
                if let Some(parent) = node.parent() {
                    if parent.kind() == "variable_declarator" {
                        return parent.child_by_field_name("name")
                            .map(|n| node_text(n, source));
                    }
                }
                None
            })?;

        let qualified_name = parent.map(|p| format!("{}.{}", p, name));

        let params = node.child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or_default();

        let is_async = node.children(&mut node.walk())
            .any(|n| node_text(n, source) == "async");

        Some(Symbol {
            name,
            qualified_name,
            symbol_type: "function".to_string(),
            language: language.to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: Some(params),
            visibility: None,
            documentation: None,
            is_test: false,
            is_async,
        })
    }

    fn extract_ts_class(&self, node: Node, source: &[u8], language: &str) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "class".to_string(),
            language: language.to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility: None,
            documentation: None,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_ts_interface(&self, node: Node, source: &[u8], language: &str) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "interface".to_string(),
            language: language.to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility: None,
            documentation: None,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_ts_type_alias(&self, node: Node, source: &[u8], language: &str) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: "type".to_string(),
            language: language.to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility: None,
            documentation: None,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_ts_import(&self, node: Node, source: &[u8]) -> Option<Import> {
        // Find the source string
        let source_node = node.child_by_field_name("source")
            .or_else(|| {
                node.children(&mut node.walk())
                    .find(|n| n.kind() == "string")
            })?;

        let path = node_text(source_node, source)
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();

        let is_external = !path.starts_with('.') && !path.starts_with('/');

        Some(Import {
            import_path: path,
            imported_symbols: None,
            is_external,
        })
    }

    // ========== Go parsing ==========

    fn parse_go(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>, Vec<FunctionCall>)> {
        let tree = self.go_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse Go code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let mut calls = Vec::new();
        let bytes = content.as_bytes();

        self.walk_go_node(tree.root_node(), bytes, &mut symbols, &mut imports, &mut calls, None, None);

        Ok((symbols, imports, calls))
    }

    fn walk_go_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        calls: &mut Vec<FunctionCall>,
        parent_name: Option<&str>,
        current_function: Option<&str>,
    ) {
        match node.kind() {
            "function_declaration" => {
                if let Some(sym) = self.extract_go_function(node, source, None) {
                    let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                    symbols.push(sym);
                    // Walk function body with this function as context
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_go_node(child, source, symbols, imports, calls, parent_name, Some(&func_name));
                        }
                    }
                    return;
                }
            }
            "method_declaration" => {
                // Extract receiver type for qualified name
                let receiver_type = node.child_by_field_name("receiver")
                    .and_then(|r| {
                        // receiver is a parameter_list, get the type from it
                        r.children(&mut r.walk())
                            .find(|n| n.kind() == "parameter_declaration")
                            .and_then(|p| p.child_by_field_name("type"))
                            .map(|t| {
                                // Handle pointer receivers (*Type)
                                let text = node_text(t, source);
                                text.trim_start_matches('*').to_string()
                            })
                    });

                if let Some(sym) = self.extract_go_function(node, source, receiver_type.as_deref()) {
                    let func_name = sym.qualified_name.clone().unwrap_or_else(|| sym.name.clone());
                    symbols.push(sym);
                    // Walk method body
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_go_node(child, source, symbols, imports, calls, parent_name, Some(&func_name));
                        }
                    }
                    return;
                }
            }
            "type_declaration" => {
                // type_declaration contains type_spec children
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "type_spec" {
                        if let Some(sym) = self.extract_go_type(child, source) {
                            symbols.push(sym);
                        }
                    }
                }
            }
            "import_declaration" => {
                // import_declaration contains import_spec children
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "import_spec" || child.kind() == "import_spec_list" {
                        self.extract_go_imports(child, source, imports);
                    }
                }
            }
            "call_expression" => {
                // Extract function call if we're inside a function
                if let Some(caller) = current_function {
                    if let Some(call) = self.extract_go_call(node, source, caller) {
                        calls.push(call);
                    }
                }
            }
            "const_declaration" | "var_declaration" => {
                // Extract package-level constants and variables
                if parent_name.is_none() {
                    if let Some(sym) = self.extract_go_var(node, source) {
                        symbols.push(sym);
                    }
                }
            }
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_go_node(child, source, symbols, imports, calls, parent_name, current_function);
        }
    }

    fn extract_go_function(&self, node: Node, source: &[u8], receiver: Option<&str>) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        let qualified_name = receiver.map(|r| format!("{}.{}", r, name));

        // Get parameters
        let params = node.child_by_field_name("parameters")
            .map(|n| node_text(n, source))
            .unwrap_or_default();

        // Get return type
        let return_type = node.child_by_field_name("result")
            .map(|n| node_text(n, source));

        let signature = if let Some(ret) = return_type {
            format!("{} {}", params, ret)
        } else {
            params
        };

        // Check visibility (Go uses capitalization)
        let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Some("public".to_string())
        } else {
            Some("private".to_string())
        };

        // Check for test function (name starts with Test)
        let is_test = name.starts_with("Test") || name.starts_with("Benchmark") || name.starts_with("Example");

        // Get doc comment
        let documentation = self.get_go_doc_comment(node, source);

        Some(Symbol {
            name,
            qualified_name,
            symbol_type: "function".to_string(),
            language: "go".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: Some(signature),
            visibility,
            documentation,
            is_test,
            is_async: false, // Go doesn't have async keyword
        })
    }

    fn extract_go_type(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        let name_node = node.child_by_field_name("name")?;
        let name = node_text(name_node, source);

        // Determine type kind (struct, interface, or alias)
        let type_node = node.child_by_field_name("type")?;
        let symbol_type = match type_node.kind() {
            "struct_type" => "struct",
            "interface_type" => "interface",
            _ => "type",
        };

        // Check visibility
        let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Some("public".to_string())
        } else {
            Some("private".to_string())
        };

        let documentation = self.get_go_doc_comment(node, source);

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: symbol_type.to_string(),
            language: "go".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: None,
            visibility,
            documentation,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_go_imports(&self, node: Node, source: &[u8], imports: &mut Vec<Import>) {
        match node.kind() {
            "import_spec" => {
                // Get the path
                if let Some(path_node) = node.child_by_field_name("path") {
                    let path = node_text(path_node, source)
                        .trim_matches('"')
                        .to_string();

                    // Check if it's an external (third-party) import
                    // Standard library doesn't have dots, third-party usually has domain
                    let is_external = path.contains('.');

                    imports.push(Import {
                        import_path: path,
                        imported_symbols: None,
                        is_external,
                    });
                }
            }
            "import_spec_list" => {
                // Recurse into the list
                for child in node.children(&mut node.walk()) {
                    if child.kind() == "import_spec" {
                        self.extract_go_imports(child, source, imports);
                    }
                }
            }
            _ => {}
        }
    }

    fn extract_go_var(&self, node: Node, source: &[u8]) -> Option<Symbol> {
        // Get the first var_spec or const_spec
        let spec = node.children(&mut node.walk())
            .find(|n| n.kind() == "var_spec" || n.kind() == "const_spec")?;

        let name_node = spec.child_by_field_name("name")
            .or_else(|| spec.children(&mut spec.walk()).find(|n| n.kind() == "identifier"))?;
        let name = node_text(name_node, source);

        let symbol_type = if node.kind() == "const_declaration" { "const" } else { "var" };

        let visibility = if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false) {
            Some("public".to_string())
        } else {
            Some("private".to_string())
        };

        // Get type if specified
        let type_sig = spec.child_by_field_name("type")
            .map(|n| node_text(n, source));

        Some(Symbol {
            name: name.clone(),
            qualified_name: Some(name),
            symbol_type: symbol_type.to_string(),
            language: "go".to_string(),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            signature: type_sig,
            visibility,
            documentation: None,
            is_test: false,
            is_async: false,
        })
    }

    fn extract_go_call(&self, node: Node, source: &[u8], caller: &str) -> Option<FunctionCall> {
        // Get the function being called
        let function_node = node.child_by_field_name("function")?;
        let callee_name = match function_node.kind() {
            "identifier" => node_text(function_node, source),
            "selector_expression" => {
                // method call: obj.Method() or pkg.Func()
                function_node.child_by_field_name("field")
                    .map(|n| node_text(n, source))?
            }
            _ => return None,
        };

        // Skip common low-value calls
        if matches!(callee_name.as_str(),
            "Print" | "Println" | "Printf" | "Sprint" | "Sprintf" | "Sprintln" |
            "Error" | "Errorf" | "Fatal" | "Fatalf" | "Panic" | "Panicf" |
            "Log" | "Logf" | "Debug" | "Debugf" | "Info" | "Infof" | "Warn" | "Warnf" |
            "New" | "Make" | "Append" | "Copy" | "Delete" | "Close" | "Len" | "Cap"
        ) {
            return None;
        }

        let call_type = if function_node.kind() == "selector_expression" {
            "method"
        } else {
            "direct"
        };

        Some(FunctionCall {
            caller_name: caller.to_string(),
            callee_name,
            call_line: node.start_position().row as u32 + 1,
            call_type: call_type.to_string(),
        })
    }

    fn get_go_doc_comment(&self, node: Node, source: &[u8]) -> Option<String> {
        // Go doc comments are // comments immediately preceding the declaration
        let mut docs = Vec::new();

        if let Some(parent) = node.parent() {
            let mut found_node = false;
            let children: Vec<_> = parent.children(&mut parent.walk()).collect();

            for child in children.into_iter().rev() {
                if child.id() == node.id() {
                    found_node = true;
                    continue;
                }
                if !found_node {
                    continue;
                }

                if child.kind() == "comment" {
                    let text = node_text(child, source);
                    if text.starts_with("//") {
                        docs.push(text.trim_start_matches('/').trim().to_string());
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        if docs.is_empty() {
            None
        } else {
            docs.reverse();
            Some(docs.join("\n"))
        }
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

fn node_text(node: Node, source: &[u8]) -> String {
    std::str::from_utf8(&source[node.byte_range()])
        .unwrap_or("")
        .to_string()
}

fn md5_hash(content: &str) -> u128 {
    // Simple hash - not cryptographic, just for change detection
    let mut hash: u128 = 0;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u128);
    }
    hash
}
