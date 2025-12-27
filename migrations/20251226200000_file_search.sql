-- File Search stores for per-project RAG via Gemini API
--
-- Each project can have one FileSearch store that contains indexed documents.
-- The store is created on-demand when the first file is indexed.

CREATE TABLE file_search_stores (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- Gemini API store name (e.g., "fileSearchStores/abc123xyz")
    store_name TEXT NOT NULL,
    -- Human-readable display name
    display_name TEXT,
    -- Timestamps
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    -- Stats from Gemini API
    active_documents INTEGER DEFAULT 0,
    pending_documents INTEGER DEFAULT 0,
    failed_documents INTEGER DEFAULT 0,
    size_bytes INTEGER DEFAULT 0,
    -- One store per project
    UNIQUE(project_id)
);

CREATE INDEX idx_file_search_stores_project ON file_search_stores(project_id);

-- Individual documents indexed in a store
CREATE TABLE file_search_documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    store_id INTEGER NOT NULL REFERENCES file_search_stores(id) ON DELETE CASCADE,
    -- Gemini Files API resource name (e.g., "files/abc123")
    file_name TEXT NOT NULL,
    -- Human-readable display name
    display_name TEXT,
    -- Original local file path
    file_path TEXT NOT NULL,
    -- File info
    mime_type TEXT,
    size_bytes INTEGER,
    -- Processing status: pending, active, failed
    status TEXT NOT NULL DEFAULT 'pending',
    -- Timestamps
    indexed_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    -- Custom metadata as JSON (for filtering)
    metadata TEXT,
    -- Unique by store + local path
    UNIQUE(store_id, file_path)
);

CREATE INDEX idx_file_search_documents_store ON file_search_documents(store_id);
CREATE INDEX idx_file_search_documents_path ON file_search_documents(file_path);
CREATE INDEX idx_file_search_documents_status ON file_search_documents(status);
