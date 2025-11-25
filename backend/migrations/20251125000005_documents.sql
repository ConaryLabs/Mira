-- backend/migrations/20251125_005_documents.sql
-- Documents & Embeddings: Document Processing, Chunking, RAG

-- ============================================================================
-- DOCUMENT MANAGEMENT
-- ============================================================================

CREATE TABLE IF NOT EXISTS documents (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    user_id TEXT,
    original_name TEXT,
    file_name TEXT,
    file_path TEXT NOT NULL,
    file_type TEXT NOT NULL,
    file_size INTEGER,
    size_bytes INTEGER,
    file_hash TEXT,
    content_hash TEXT,
    metadata TEXT,
    page_count INTEGER,
    chunk_count INTEGER DEFAULT 0,
    status TEXT DEFAULT 'pending',
    processing_started_at INTEGER,
    processing_completed_at INTEGER,
    uploaded_at INTEGER DEFAULT (strftime('%s', 'now')),
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    updated_at INTEGER,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
);

CREATE INDEX IF NOT EXISTS idx_documents_project ON documents(project_id);
CREATE INDEX IF NOT EXISTS idx_documents_user ON documents(user_id);
CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(file_hash);
CREATE INDEX IF NOT EXISTS idx_documents_type ON documents(file_type);

-- ============================================================================
-- DOCUMENT CHUNKS
-- ============================================================================

CREATE TABLE IF NOT EXISTS document_chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    page_number INTEGER,
    char_start INTEGER,
    char_end INTEGER,
    qdrant_point_id TEXT,
    embedding_point_id TEXT,
    collection_name TEXT DEFAULT 'conversation',
    created_at INTEGER DEFAULT (strftime('%s', 'now')),
    UNIQUE(document_id, chunk_index),
    FOREIGN KEY (document_id) REFERENCES documents(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_document_chunks_document ON document_chunks(document_id);
CREATE INDEX IF NOT EXISTS idx_document_chunks_index ON document_chunks(chunk_index);
CREATE INDEX IF NOT EXISTS idx_document_chunks_point ON document_chunks(embedding_point_id);
