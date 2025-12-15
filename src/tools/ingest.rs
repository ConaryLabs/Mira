// src/tools/ingest.rs
// Document ingestion: PDF, markdown, and text file processing

use anyhow::{Context, Result};
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

use super::semantic::{SemanticSearch, COLLECTION_DOCS};

/// Supported document types
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DocType {
    Pdf,
    Markdown,
    Text,
}

impl DocType {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "pdf" => Some(Self::Pdf),
            "md" | "markdown" => Some(Self::Markdown),
            "txt" | "text" => Some(Self::Text),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Markdown => "markdown",
            Self::Text => "text",
        }
    }
}

/// Chunking configuration
const TARGET_CHUNK_TOKENS: usize = 500;
const CHUNK_OVERLAP_TOKENS: usize = 50;
const CHARS_PER_TOKEN: usize = 4; // Rough approximation

/// Result of document ingestion
pub struct IngestResult {
    pub document_id: String,
    pub name: String,
    pub doc_type: String,
    pub chunk_count: usize,
    pub total_tokens: usize,
}

/// Ingest a document from a file path
pub async fn ingest_document(
    db: &SqlitePool,
    semantic: Option<&SemanticSearch>,
    file_path: &str,
    name: Option<&str>,
) -> Result<IngestResult> {
    let path = Path::new(file_path);

    // Validate file exists
    if !path.exists() {
        anyhow::bail!("File not found: {}", file_path);
    }

    // Detect document type from extension
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let doc_type = DocType::from_extension(extension)
        .ok_or_else(|| anyhow::anyhow!(
            "Unsupported file type: .{} (supported: pdf, md, txt)",
            extension
        ))?;

    // Extract text content
    let content = extract_text(path, doc_type)?;

    if content.trim().is_empty() {
        anyhow::bail!("No text content could be extracted from the file");
    }

    // Generate document name
    let doc_name = name.map(|s| s.to_string()).unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string()
    });

    // Generate document ID
    let doc_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Chunk the content
    let chunks = chunk_text(&content, TARGET_CHUNK_TOKENS, CHUNK_OVERLAP_TOKENS);
    let chunk_count = chunks.len();
    let total_tokens: usize = chunks.iter().map(|c| estimate_tokens(c)).sum();

    info!(
        "Ingesting '{}' ({}) - {} chunks, ~{} tokens",
        doc_name, doc_type.as_str(), chunk_count, total_tokens
    );

    // Store document metadata in SQLite
    sqlx::query(
        r#"
        INSERT INTO documents (id, name, file_path, doc_type, content, chunk_count, total_tokens, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
        "#,
    )
    .bind(&doc_id)
    .bind(&doc_name)
    .bind(file_path)
    .bind(doc_type.as_str())
    .bind(if content.len() < 50000 { Some(&content) } else { None }) // Store full content if small
    .bind(chunk_count as i64)
    .bind(total_tokens as i64)
    .bind(now)
    .execute(db)
    .await
    .context("Failed to insert document")?;

    // Store chunks in SQLite and prepare for Qdrant
    let mut qdrant_items: Vec<(String, String, HashMap<String, serde_json::Value>)> = Vec::new();

    for (idx, chunk_content) in chunks.iter().enumerate() {
        let chunk_id = format!("{}_{}", doc_id, idx);
        let token_count = estimate_tokens(chunk_content);

        // Store chunk in SQLite
        sqlx::query(
            r#"
            INSERT INTO document_chunks (id, document_id, chunk_index, content, token_count, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(&chunk_id)
        .bind(&doc_id)
        .bind(idx as i64)
        .bind(chunk_content)
        .bind(token_count as i64)
        .bind(now)
        .execute(db)
        .await
        .context("Failed to insert chunk")?;

        // Prepare for Qdrant batch upload
        let mut metadata = HashMap::new();
        metadata.insert("document_id".to_string(), serde_json::json!(doc_id));
        metadata.insert("document_name".to_string(), serde_json::json!(doc_name));
        metadata.insert("doc_type".to_string(), serde_json::json!(doc_type.as_str()));
        metadata.insert("chunk_index".to_string(), serde_json::json!(idx));
        metadata.insert("file_path".to_string(), serde_json::json!(file_path));

        qdrant_items.push((chunk_id, chunk_content.clone(), metadata));
    }

    // Store embeddings in Qdrant (batch for efficiency)
    if let Some(semantic) = semantic {
        if semantic.is_available() && !qdrant_items.is_empty() {
            semantic.ensure_collection(COLLECTION_DOCS).await?;

            let stored = semantic.store_batch(COLLECTION_DOCS, qdrant_items).await
                .context("Failed to store embeddings in Qdrant")?;

            debug!("Stored {} chunk embeddings in Qdrant", stored);
        }
    }

    Ok(IngestResult {
        document_id: doc_id,
        name: doc_name,
        doc_type: doc_type.as_str().to_string(),
        chunk_count,
        total_tokens,
    })
}

/// Extract text content from a file based on its type
fn extract_text(path: &Path, doc_type: DocType) -> Result<String> {
    match doc_type {
        DocType::Pdf => extract_pdf_text(path),
        DocType::Markdown => extract_markdown_text(path),
        DocType::Text => std::fs::read_to_string(path)
            .context("Failed to read text file"),
    }
}

/// Extract text from a PDF file
fn extract_pdf_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path).context("Failed to read PDF file")?;

    pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| anyhow::anyhow!("PDF extraction failed: {}", e))
}

/// Extract plain text from a markdown file
fn extract_markdown_text(path: &Path) -> Result<String> {
    let md_content = std::fs::read_to_string(path)
        .context("Failed to read markdown file")?;

    // Parse markdown and extract text
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};

    let parser = Parser::new(&md_content);
    let mut text = String::new();
    let mut in_code_block = false;

    for event in parser {
        match event {
            Event::Text(t) => {
                text.push_str(&t);
                if !in_code_block {
                    text.push(' ');
                }
            }
            Event::Code(c) => {
                text.push_str(&c);
                text.push(' ');
            }
            Event::SoftBreak | Event::HardBreak => {
                text.push('\n');
            }
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                text.push('\n');
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                text.push('\n');
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                text.push_str("\n\n");
            }
            Event::Start(Tag::Heading { .. }) => {}
            Event::End(TagEnd::Heading(_)) => {
                text.push_str("\n\n");
            }
            Event::Start(Tag::Item) => {
                text.push_str("• ");
            }
            Event::End(TagEnd::Item) => {
                text.push('\n');
            }
            _ => {}
        }
    }

    Ok(text.trim().to_string())
}

/// Chunk text into segments of approximately target_tokens size with overlap
fn chunk_text(text: &str, target_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    let target_chars = target_tokens * CHARS_PER_TOKEN;
    let overlap_chars = overlap_tokens * CHARS_PER_TOKEN;

    // Split into paragraphs first for more natural breaks
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    let mut chunks = Vec::new();
    let mut current_chunk = String::new();

    for para in paragraphs {
        // If adding this paragraph exceeds target, start a new chunk
        if !current_chunk.is_empty()
            && current_chunk.len() + para.len() > target_chars
        {
            chunks.push(current_chunk.trim().to_string());

            // Start new chunk with overlap from previous
            if overlap_chars > 0 && chunks.last().map(|c| c.len()).unwrap_or(0) > overlap_chars {
                let last = chunks.last().unwrap();
                let overlap_start = last.len().saturating_sub(overlap_chars);
                // Find word boundary for overlap
                let overlap = if let Some(space_pos) = last[overlap_start..].find(' ') {
                    &last[overlap_start + space_pos + 1..]
                } else {
                    &last[overlap_start..]
                };
                current_chunk = format!("{}\n\n", overlap);
            } else {
                current_chunk = String::new();
            }
        }

        // Handle very long paragraphs by splitting on sentences
        if para.len() > target_chars {
            let sentences: Vec<&str> = para
                .split(|c| c == '.' || c == '!' || c == '?')
                .filter(|s| !s.trim().is_empty())
                .collect();

            for sentence in sentences {
                let sentence_with_punct = format!("{}. ", sentence.trim());
                if current_chunk.len() + sentence_with_punct.len() > target_chars && !current_chunk.is_empty() {
                    chunks.push(current_chunk.trim().to_string());
                    current_chunk = String::new();
                }
                current_chunk.push_str(&sentence_with_punct);
            }
        } else {
            if !current_chunk.is_empty() {
                current_chunk.push_str("\n\n");
            }
            current_chunk.push_str(para);
        }
    }

    // Don't forget the last chunk
    if !current_chunk.trim().is_empty() {
        chunks.push(current_chunk.trim().to_string());
    }

    // Filter out any empty chunks
    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

/// Estimate token count for a string (rough approximation)
fn estimate_tokens(text: &str) -> usize {
    // Rough heuristic: ~4 characters per token on average
    (text.len() + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN
}

/// Delete a document and all its chunks
pub async fn delete_document(
    db: &SqlitePool,
    semantic: Option<&SemanticSearch>,
    document_id: &str,
) -> Result<bool> {
    // Check if document exists
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM documents WHERE id = $1"
    )
    .bind(document_id)
    .fetch_optional(db)
    .await?;

    if exists.is_none() {
        return Ok(false);
    }

    // Delete from Qdrant first (by document_id field)
    if let Some(semantic) = semantic {
        if semantic.is_available() {
            semantic.delete_by_field(COLLECTION_DOCS, "document_id", document_id).await?;
        }
    }

    // Delete chunks from SQLite (CASCADE should handle this, but be explicit)
    sqlx::query("DELETE FROM document_chunks WHERE document_id = $1")
        .bind(document_id)
        .execute(db)
        .await?;

    // Delete document from SQLite
    sqlx::query("DELETE FROM documents WHERE id = $1")
        .bind(document_id)
        .execute(db)
        .await?;

    info!("Deleted document: {}", document_id);
    Ok(true)
}

/// Check if a file path is a supported document type
pub fn is_document_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(DocType::from_extension)
        .is_some()
}

/// Update a document if it has changed, or ingest if new
/// Returns Some(result) if document was updated/created, None if unchanged
pub async fn update_document(
    db: &SqlitePool,
    semantic: Option<&SemanticSearch>,
    file_path: &str,
) -> Result<Option<IngestResult>> {
    let path = Path::new(file_path);

    // Check if file exists
    if !path.exists() {
        // File was deleted - remove from database if it exists
        if let Some((doc_id,)) = sqlx::query_as::<_, (String,)>(
            "SELECT id FROM documents WHERE file_path = $1"
        )
        .bind(file_path)
        .fetch_optional(db)
        .await?
        {
            delete_document(db, semantic, &doc_id).await?;
            info!("Removed deleted document: {}", file_path);
        }
        return Ok(None);
    }

    // Get file modification time
    let metadata = std::fs::metadata(path)?;
    let file_mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Check if document exists and get its updated_at
    let existing: Option<(String, i64)> = sqlx::query_as(
        "SELECT id, updated_at FROM documents WHERE file_path = $1"
    )
    .bind(file_path)
    .fetch_optional(db)
    .await?;

    if let Some((doc_id, updated_at)) = existing {
        // Document exists - check if file has changed
        if file_mtime <= updated_at {
            debug!("Document unchanged: {}", file_path);
            return Ok(None);
        }

        // File changed - delete old and re-ingest
        debug!("Document changed, re-ingesting: {}", file_path);
        delete_document(db, semantic, &doc_id).await?;
    }

    // Ingest (new or updated)
    let result = ingest_document(db, semantic, file_path, None).await?;
    Ok(Some(result))
}

/// Delete a document by file path (convenience wrapper)
pub async fn delete_document_by_path(
    db: &SqlitePool,
    semantic: Option<&SemanticSearch>,
    file_path: &str,
) -> Result<bool> {
    if let Some((doc_id,)) = sqlx::query_as::<_, (String,)>(
        "SELECT id FROM documents WHERE file_path = $1"
    )
    .bind(file_path)
    .fetch_optional(db)
    .await?
    {
        delete_document(db, semantic, &doc_id).await
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_basic() {
        let text = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph.";
        let chunks = chunk_text(text, 20, 5); // Very small chunks for testing
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 2); // 5 chars / 4 = 1.25, rounded up = 2
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars / 4 = 2.75, rounded up = 3
    }

    #[test]
    fn test_doc_type_from_extension() {
        assert_eq!(DocType::from_extension("pdf"), Some(DocType::Pdf));
        assert_eq!(DocType::from_extension("PDF"), Some(DocType::Pdf));
        assert_eq!(DocType::from_extension("md"), Some(DocType::Markdown));
        assert_eq!(DocType::from_extension("txt"), Some(DocType::Text));
        assert_eq!(DocType::from_extension("docx"), None);
    }
}
