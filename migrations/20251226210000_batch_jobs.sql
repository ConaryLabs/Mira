-- Batch jobs for async processing via Gemini Batch API
--
-- Provides 50% cost savings for high-volume, latency-tolerant tasks like:
-- - Memory compaction
-- - Document summarization
-- - Codebase analysis

CREATE TABLE IF NOT EXISTS batch_jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Optional project association
    project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    -- Job type: compaction, summarize, analyze, etc.
    job_type TEXT NOT NULL,
    -- Status: pending, submitted, running, succeeded, failed, cancelled, expired
    status TEXT NOT NULL DEFAULT 'pending',
    -- Gemini batch name (e.g., "batches/123456")
    gemini_batch_name TEXT,
    -- Display name for the batch
    display_name TEXT,
    -- Input data as JSON (for inline requests) or file reference
    input_data TEXT,
    -- Number of requests in the batch
    request_count INTEGER DEFAULT 0,
    -- Output data as JSON (results)
    output_data TEXT,
    -- Timestamps
    created_at INTEGER NOT NULL,
    submitted_at INTEGER,
    completed_at INTEGER,
    -- Error message if failed
    error_message TEXT,
    -- Metadata for tracking
    metadata TEXT
);

CREATE INDEX IF NOT EXISTS idx_batch_jobs_status ON batch_jobs(status);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_project ON batch_jobs(project_id);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_type ON batch_jobs(job_type);
CREATE INDEX IF NOT EXISTS idx_batch_jobs_gemini ON batch_jobs(gemini_batch_name);

-- Individual requests within a batch (for tracking per-request results)
CREATE TABLE IF NOT EXISTS batch_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id INTEGER NOT NULL REFERENCES batch_jobs(id) ON DELETE CASCADE,
    -- Request key for correlation
    request_key TEXT NOT NULL,
    -- Request content as JSON
    request_data TEXT NOT NULL,
    -- Response content as JSON
    response_data TEXT,
    -- Status: pending, succeeded, failed
    status TEXT NOT NULL DEFAULT 'pending',
    -- Error message if failed
    error_message TEXT,
    -- Timestamps
    created_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE(job_id, request_key)
);

CREATE INDEX IF NOT EXISTS idx_batch_requests_job ON batch_requests(job_id);
CREATE INDEX IF NOT EXISTS idx_batch_requests_status ON batch_requests(status);
