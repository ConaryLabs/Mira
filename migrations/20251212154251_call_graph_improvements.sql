-- Call graph improvements: track unresolved calls and add callee names

-- Table for calls that couldn't be resolved at indexing time
-- These will be resolved later when the target file is indexed
CREATE TABLE IF NOT EXISTS unresolved_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    caller_id INTEGER NOT NULL,
    callee_name TEXT NOT NULL,              -- The unresolved function/method name
    call_type TEXT DEFAULT 'direct',        -- 'direct', 'method', 'static', 'macro'
    call_line INTEGER,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    FOREIGN KEY (caller_id) REFERENCES code_symbols(id) ON DELETE CASCADE,
    UNIQUE(caller_id, callee_name, call_line)
);

CREATE INDEX IF NOT EXISTS idx_unresolved_callee_name ON unresolved_calls(callee_name);
CREATE INDEX IF NOT EXISTS idx_unresolved_caller ON unresolved_calls(caller_id);

-- Add callee_name to call_graph for searching by name even when resolved
ALTER TABLE call_graph ADD COLUMN callee_name TEXT;

-- Create index for name-based searches
CREATE INDEX IF NOT EXISTS idx_callgraph_callee_name ON call_graph(callee_name);
