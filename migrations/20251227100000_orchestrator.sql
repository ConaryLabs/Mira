-- Orchestrator tables for Gemini-powered context management
-- Supports routing cache, debouncing, category summaries, and extracted decisions

-- Routing cache: query -> category mapping with similarity search
CREATE TABLE IF NOT EXISTS routing_cache (
    query_hash TEXT PRIMARY KEY,
    category TEXT NOT NULL,
    confidence REAL NOT NULL,
    reasoning TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    hits INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_routing_cache_created ON routing_cache(created_at);
CREATE INDEX IF NOT EXISTS idx_routing_cache_category ON routing_cache(category);

-- Category summaries: pre-computed summaries for carousel
CREATE TABLE IF NOT EXISTS category_summaries (
    id INTEGER PRIMARY KEY,
    category TEXT NOT NULL,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    generated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(category, project_id)
);
CREATE INDEX IF NOT EXISTS idx_category_summaries_cat ON category_summaries(category);

-- Centralized debounce state (replaces /tmp files)
CREATE TABLE IF NOT EXISTS debounce_state (
    key TEXT PRIMARY KEY,
    last_triggered INTEGER NOT NULL,
    trigger_count INTEGER DEFAULT 1,
    context TEXT  -- Optional JSON context
);
CREATE INDEX IF NOT EXISTS idx_debounce_last ON debounce_state(last_triggered);

-- Extracted decisions from transcripts
CREATE TABLE IF NOT EXISTS extracted_decisions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    content TEXT NOT NULL,
    confidence REAL NOT NULL,
    decision_type TEXT NOT NULL,  -- technical, architectural, approach, rejection
    context TEXT,
    extracted_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS idx_decisions_session ON extracted_decisions(session_id);
CREATE INDEX IF NOT EXISTS idx_decisions_type ON extracted_decisions(decision_type);
CREATE INDEX IF NOT EXISTS idx_decisions_project ON extracted_decisions(project_id);
CREATE INDEX IF NOT EXISTS idx_decisions_extracted ON extracted_decisions(extracted_at);
