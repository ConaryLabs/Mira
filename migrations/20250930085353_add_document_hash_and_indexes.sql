-- Add only the missing file_hash column and indexes for document processing

-- Add file_hash for duplicate detection
ALTER TABLE documents ADD COLUMN file_hash TEXT;

-- Create unique index for duplicate detection
CREATE UNIQUE INDEX IF NOT EXISTS idx_documents_hash_project 
ON documents(file_hash, project_id) 
WHERE file_hash IS NOT NULL;

-- Add index for faster lookups by project
CREATE INDEX IF NOT EXISTS idx_documents_project_created 
ON documents(project_id, uploaded_at DESC);

-- Add index for document chunks to speed up retrieval
CREATE INDEX IF NOT EXISTS idx_document_chunks_doc_id 
ON document_chunks(document_id, chunk_index);
