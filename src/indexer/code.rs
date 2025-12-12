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

pub struct CodeIndexer {
    db: SqlitePool,
    semantic: Option<Arc<SemanticSearch>>,
    rust_parser: Parser,
    python_parser: Parser,
    typescript_parser: Parser,
    javascript_parser: Parser,
}

impl CodeIndexer {
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

        Ok(Self {
            db,
            semantic,
            rust_parser,
            python_parser,
            typescript_parser,
            javascript_parser,
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

            // Skip hidden directories
            if file_path.components().any(|c| {
                c.as_os_str().to_string_lossy().starts_with('.')
            }) {
                continue;
            }

            // Check extension
            let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "rs" | "py" | "ts" | "tsx" | "js" | "jsx") {
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
        let (symbols, imports) = match ext {
            "rs" => self.parse_rust(&content)?,
            "py" => self.parse_python(&content)?,
            "ts" | "tsx" => self.parse_typescript(&content)?,
            "js" | "jsx" => self.parse_javascript(&content)?,
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

    fn parse_rust(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>)> {
        let tree = self.rust_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse Rust code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let bytes = content.as_bytes();

        self.walk_rust_node(tree.root_node(), bytes, &mut symbols, &mut imports, None);

        Ok((symbols, imports))
    }

    fn walk_rust_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        parent_name: Option<&str>,
    ) {
        match node.kind() {
            "function_item" | "function_signature_item" => {
                if let Some(sym) = self.extract_rust_function(node, source, parent_name) {
                    symbols.push(sym);
                }
            }
            "struct_item" => {
                if let Some(sym) = self.extract_rust_struct(node, source) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    // Walk children for impl methods
                    for child in node.children(&mut node.walk()) {
                        self.walk_rust_node(child, source, symbols, imports, Some(&name));
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
                        self.walk_rust_node(child, source, symbols, imports, Some(&name));
                    }
                    return;
                }
            }
            "impl_item" => {
                // Get the type being implemented
                let type_name = node.child_by_field_name("type")
                    .map(|n| node_text(n, source));
                for child in node.children(&mut node.walk()) {
                    self.walk_rust_node(child, source, symbols, imports, type_name.as_deref());
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
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_rust_node(child, source, symbols, imports, parent_name);
        }
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

    fn parse_python(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>)> {
        let tree = self.python_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse Python code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let bytes = content.as_bytes();

        self.walk_python_node(tree.root_node(), bytes, &mut symbols, &mut imports, None);

        Ok((symbols, imports))
    }

    fn walk_python_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        parent_name: Option<&str>,
    ) {
        match node.kind() {
            "function_definition" => {
                if let Some(sym) = self.extract_python_function(node, source, parent_name) {
                    symbols.push(sym);
                }
            }
            "class_definition" => {
                if let Some(sym) = self.extract_python_class(node, source) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    // Walk children for methods
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_python_node(child, source, symbols, imports, Some(&name));
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
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_python_node(child, source, symbols, imports, parent_name);
        }
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

    fn parse_typescript(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>)> {
        let tree = self.typescript_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse TypeScript code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let bytes = content.as_bytes();

        self.walk_ts_node(tree.root_node(), bytes, &mut symbols, &mut imports, None, "typescript");

        Ok((symbols, imports))
    }

    fn parse_javascript(&mut self, content: &str) -> Result<(Vec<Symbol>, Vec<Import>)> {
        let tree = self.javascript_parser.parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse JavaScript code"))?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let bytes = content.as_bytes();

        self.walk_ts_node(tree.root_node(), bytes, &mut symbols, &mut imports, None, "javascript");

        Ok((symbols, imports))
    }

    fn walk_ts_node(
        &self,
        node: Node,
        source: &[u8],
        symbols: &mut Vec<Symbol>,
        imports: &mut Vec<Import>,
        parent_name: Option<&str>,
        language: &str,
    ) {
        match node.kind() {
            "function_declaration" | "method_definition" | "arrow_function" => {
                if let Some(sym) = self.extract_ts_function(node, source, parent_name, language) {
                    symbols.push(sym);
                }
            }
            "class_declaration" => {
                if let Some(sym) = self.extract_ts_class(node, source, language) {
                    let name = sym.name.clone();
                    symbols.push(sym);
                    // Walk children for methods
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in body.children(&mut body.walk()) {
                            self.walk_ts_node(child, source, symbols, imports, Some(&name), language);
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
            _ => {}
        }

        // Recurse into children
        for child in node.children(&mut node.walk()) {
            self.walk_ts_node(child, source, symbols, imports, parent_name, language);
        }
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
