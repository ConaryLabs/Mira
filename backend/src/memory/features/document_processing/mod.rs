// src/memory/features/document_processing/mod.rs
//! Document processing module for PDF, DOCX, and text files
//! 
//! This module handles:
//! - Document parsing (PDF, DOCX, TXT, MD)
//! - Intelligent text chunking with semantic boundaries
//! - Storage in SQLite and Qdrant
//! - Duplicate detection via SHA-256 hashing
//! - WebSocket-based upload with progress tracking

use anyhow::Result;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use sha2::{Sha256, Digest};
use tokio::io::AsyncReadExt;

mod parser;
mod chunker;
mod storage;

pub use parser::{DocumentParser, RawDocument};
pub use chunker::{DocumentChunker, ChunkingStrategy};
pub use storage::{DocumentStorage, DocumentRecord, DocumentSearchResult};

/// Processed document with all metadata and chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedDocument {
    pub id: String,
    pub project_id: String,
    pub file_path: String,  // Path to original stored file
    pub file_name: String,
    pub file_type: String,
    pub file_hash: String,  // SHA-256 for duplicate detection
    pub size_bytes: i64,
    pub content: String,
    pub chunks: Vec<DocumentChunk>,
    pub metadata: DocumentMetadata,
    pub word_count: usize,
    pub processing_status: ProcessingStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Document processing status for progress tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum ProcessingStatus {
    Pending,
    Processing { progress: f32 },
    Completed,
    Failed { error: String },
}

/// Individual document chunk for storage and retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id: String,
    pub document_id: String,
    pub content: String,
    pub chunk_index: usize,
    pub page_number: Option<usize>,
    pub section_title: Option<String>,
    pub char_start: usize,
    pub char_end: usize,
}

/// Document metadata extracted during parsing
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub creation_date: Option<String>,
    pub page_count: Option<usize>,
    pub language: Option<String>,
}

/// Main document processor that coordinates parsing, chunking, and storage
pub struct DocumentProcessor {
    parser: DocumentParser,
    chunker: DocumentChunker,
    storage: DocumentStorage,
    max_file_size: usize,  // Default 100MB
}

impl DocumentProcessor {
    /// Create a new document processor with database connections
    pub fn new(
        sqlite_pool: sqlx::SqlitePool, 
        qdrant_client: qdrant_client::Qdrant
    ) -> Self {
        Self {
            parser: DocumentParser::new(),
            chunker: DocumentChunker::new(),
            storage: DocumentStorage::new(sqlite_pool, qdrant_client),
            max_file_size: 100 * 1024 * 1024,  // 100MB limit
        }
    }
    
    /// Process a document file through the full pipeline
    pub async fn process_document(
        &self, 
        file_path: &Path, 
        project_id: &str,
        progress_callback: Option<Box<dyn Fn(f32) + Send + Sync>>
    ) -> Result<ProcessedDocument> {
        // 1. Check file size
        let file_metadata = tokio::fs::metadata(file_path).await?;
        if file_metadata.len() as usize > self.max_file_size {
            return Err(anyhow::anyhow!("File size exceeds maximum limit of 100MB"));
        }
        
        // Report progress: Starting
        if let Some(ref callback) = progress_callback {
            callback(0.1);
        }
        
        // 2. Calculate file hash for duplicate detection
        let file_hash = self.calculate_file_hash(file_path).await?;
        
        // 3. Check for existing document with same hash
        if let Some(existing) = self.storage.find_by_hash(&file_hash, project_id).await? {
            return Err(anyhow::anyhow!(
                "Document already exists with ID: {}", 
                existing.id
            ));
        }
        
        // Report progress: Parsing
        if let Some(ref callback) = progress_callback {
            callback(0.2);
        }
        
        // 4. Store original file in storage directory
        let stored_path = self.store_original_file(file_path, project_id).await?;
        
        // 5. Parse document to extract text and metadata
        let raw_document = self.parser.parse(file_path).await?;
        
        // Report progress: Chunking
        if let Some(ref callback) = progress_callback {
            callback(0.5);
        }
        
        // 6. Chunk the document intelligently
        let chunks = self.chunker.chunk_document(
            &raw_document.content,
            Some(&raw_document.metadata)
        )?;
        
        // 7. Create processed document structure
        let processed = ProcessedDocument {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_string(),
            file_path: stored_path.clone(),
            file_name: file_path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string(),
            file_type: self.detect_file_type(file_path),
            file_hash,
            size_bytes: file_metadata.len() as i64,
            content: raw_document.content.clone(),
            chunks: chunks.into_iter().enumerate().map(|(idx, chunk_content)| {
                DocumentChunk {
                    id: uuid::Uuid::new_v4().to_string(),
                    document_id: String::new(), // Will be set during storage
                    content: chunk_content.clone(),
                    chunk_index: idx,
                    page_number: None, // Could be enhanced with page tracking
                    section_title: None, // Could extract from headers
                    char_start: 0, // Would need position tracking
                    char_end: chunk_content.len(),
                }
            }).collect(),
            metadata: raw_document.metadata,
            word_count: raw_document.content.split_whitespace().count(),
            processing_status: ProcessingStatus::Processing { progress: 0.8 },
            created_at: chrono::Utc::now(),
        };
        
        // Report progress: Storing
        if let Some(ref callback) = progress_callback {
            callback(0.8);
        }
        
        // 8. Store in database and vector store
        self.storage.store_document(&processed).await?;
        
        // Report progress: Complete
        if let Some(ref callback) = progress_callback {
            callback(1.0);
        }
        
        Ok(ProcessedDocument {
            processing_status: ProcessingStatus::Completed,
            ..processed
        })
    }
    
    /// Calculate SHA-256 hash of file for duplicate detection
    async fn calculate_file_hash(&self, file_path: &Path) -> Result<String> {
        let mut file = tokio::fs::File::open(file_path).await?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 8192];
        
        loop {
            let bytes_read = file.read(&mut buffer).await?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }
    
    /// Store original file in project storage directory
    async fn store_original_file(&self, file_path: &Path, project_id: &str) -> Result<String> {
        // Create documents storage directory structure
        let storage_dir = PathBuf::from("storage/documents").join(project_id);
        tokio::fs::create_dir_all(&storage_dir).await?;
        
        // Generate unique filename with timestamp
        let timestamp = chrono::Utc::now().timestamp_millis();
        let file_name = file_path.file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?;
        let stored_name = format!("{}_{}", timestamp, file_name.to_string_lossy());
        let stored_path = storage_dir.join(&stored_name);
        
        // Copy file to storage
        tokio::fs::copy(file_path, &stored_path).await?;
        
        Ok(stored_path.to_string_lossy().to_string())
    }
    
    /// Detect file type from extension
    fn detect_file_type(&self, path: &Path) -> String {
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string()
    }
    
    /// Search documents by query
    pub async fn search_documents(
        &self,
        project_id: &str,
        query: &str,
        limit: usize
    ) -> Result<Vec<DocumentSearchResult>> {
        self.storage.search_documents(project_id, query, limit).await
    }
    
    /// Retrieve a specific document by ID
    pub async fn retrieve_document(&self, document_id: &str) -> Result<Option<DocumentRecord>> {
        self.storage.retrieve_document(document_id).await
    }
    
    /// Delete a document and its chunks
    pub async fn delete_document(&self, document_id: &str) -> Result<()> {
        self.storage.delete_document(document_id).await
    }
}
