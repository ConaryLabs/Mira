-- Proposals: Auto-captured goals, tasks, decisions pending confirmation
-- Part of the "Proactive Organization System" (GPT-5.2 design)
--
-- Key concepts:
-- 1. Extractor pipeline captures candidates from conversation
-- 2. High-confidence items auto-commit, low-confidence queue for review
-- 3. Dedupe via embeddings prevents DB trash
-- 4. Lazy confirmation at natural breakpoints (session_start, precompact)

CREATE TABLE proposals (
    id TEXT PRIMARY KEY,
    proposal_type TEXT NOT NULL,            -- 'goal', 'task', 'decision', 'summary'
    content TEXT NOT NULL,                  -- the extracted content
    title TEXT,                             -- optional short title (for goals/tasks)
    confidence REAL NOT NULL DEFAULT 0.5,   -- 0.0-1.0, higher = more certain
    evidence TEXT,                          -- JSON: message quotes/context that triggered this
    status TEXT NOT NULL DEFAULT 'pending', -- 'pending', 'confirmed', 'rejected', 'auto_committed'

    -- Deduplication
    content_hash TEXT,                      -- for exact match dedup
    embedding_id TEXT,                      -- Qdrant point ID for semantic dedup
    similar_to TEXT,                        -- if merged, points to the canonical proposal/item

    -- Provenance
    source_tool TEXT,                       -- which tool call triggered extraction
    source_context TEXT,                    -- additional context (file, task, etc.)
    project_path TEXT,                      -- project this belongs to

    -- Lifecycle
    created_at INTEGER NOT NULL,
    processed_at INTEGER,                   -- when confirmed/rejected/auto-committed
    promoted_to TEXT,                       -- if confirmed, ID of the created goal/task/decision

    -- For batch review
    batch_id TEXT,                          -- group proposals for batch review
    review_priority INTEGER DEFAULT 0       -- higher = review first
);

CREATE INDEX idx_proposals_type ON proposals(proposal_type);
CREATE INDEX idx_proposals_status ON proposals(status);
CREATE INDEX idx_proposals_confidence ON proposals(confidence);
CREATE INDEX idx_proposals_project ON proposals(project_path);
CREATE INDEX idx_proposals_batch ON proposals(batch_id);
CREATE INDEX idx_proposals_hash ON proposals(content_hash);
CREATE INDEX idx_proposals_created ON proposals(created_at);

-- Extraction patterns: heuristic rules for detecting goals/tasks/decisions
CREATE TABLE extraction_patterns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pattern_type TEXT NOT NULL,             -- 'goal', 'task', 'decision'
    pattern TEXT NOT NULL,                  -- regex pattern
    confidence_boost REAL DEFAULT 0.0,      -- add to base confidence when matched
    description TEXT,                       -- human-readable description
    enabled BOOLEAN DEFAULT TRUE,
    times_matched INTEGER DEFAULT 0,
    times_confirmed INTEGER DEFAULT 0,      -- track pattern effectiveness
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX idx_patterns_type ON extraction_patterns(pattern_type);
CREATE INDEX idx_patterns_enabled ON extraction_patterns(enabled);

-- Seed with initial patterns
INSERT INTO extraction_patterns (pattern_type, pattern, confidence_boost, description, created_at, updated_at) VALUES
    -- Goal patterns (larger objectives)
    ('goal', '(?i)\b(goal|objective|aim)\s*(is|:)', 0.3, 'Explicit goal statement', strftime('%s', 'now'), strftime('%s', 'now')),
    ('goal', '(?i)\bwe\s+(want|need)\s+to\s+(?:eventually|ultimately)', 0.2, 'Long-term want/need', strftime('%s', 'now'), strftime('%s', 'now')),
    ('goal', '(?i)\bthe\s+plan\s+is\s+to', 0.2, 'Plan statement', strftime('%s', 'now'), strftime('%s', 'now')),
    ('goal', '(?i)\bworking\s+towards?', 0.1, 'Working towards', strftime('%s', 'now'), strftime('%s', 'now')),

    -- Task patterns (actionable items)
    ('task', '(?i)\b(need|have)\s+to\s+\w+', 0.2, 'Need/have to do', strftime('%s', 'now'), strftime('%s', 'now')),
    ('task', '(?i)\bshould\s+\w+', 0.1, 'Should do', strftime('%s', 'now'), strftime('%s', 'now')),
    ('task', '(?i)\blet''?s\s+\w+', 0.15, 'Let''s do', strftime('%s', 'now'), strftime('%s', 'now')),
    ('task', '(?i)\bTODO:?\s*', 0.4, 'Explicit TODO', strftime('%s', 'now'), strftime('%s', 'now')),
    ('task', '(?i)\bFIXME:?\s*', 0.35, 'Explicit FIXME', strftime('%s', 'now'), strftime('%s', 'now')),
    ('task', '(?i)\bremind\s+me\s+to', 0.3, 'Remind me to', strftime('%s', 'now'), strftime('%s', 'now')),
    ('task', '(?i)\bdon''?t\s+forget\s+to', 0.25, 'Don''t forget to', strftime('%s', 'now'), strftime('%s', 'now')),

    -- Decision patterns
    ('decision', '(?i)\b(decided|going)\s+(to|with)', 0.3, 'Decided to/Going with', strftime('%s', 'now'), strftime('%s', 'now')),
    ('decision', '(?i)\blet''?s\s+(go|use|stick)\s+with', 0.25, 'Let''s go/use/stick with', strftime('%s', 'now'), strftime('%s', 'now')),
    ('decision', '(?i)\bwe''?ll\s+(use|go\s+with)', 0.2, 'We''ll use/go with', strftime('%s', 'now'), strftime('%s', 'now')),
    ('decision', '(?i)\bthe\s+choice\s+is', 0.3, 'The choice is', strftime('%s', 'now'), strftime('%s', 'now')),
    ('decision', '(?i)\binstead\s+of\s+.+,?\s+(we|I)''?(ll|m)', 0.2, 'Instead of X, we''ll Y', strftime('%s', 'now'), strftime('%s', 'now'));
