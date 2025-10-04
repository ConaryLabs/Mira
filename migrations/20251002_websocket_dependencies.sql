-- WebSocket Dependency Tracking Migration
-- Tracks frontend→backend message calls and backend handlers

-- Frontend WebSocket sends (TypeScript/JavaScript)
CREATE TABLE IF NOT EXISTS websocket_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Frontend source
    frontend_file_id INTEGER NOT NULL,
    frontend_element TEXT NOT NULL,    -- "ChatInterface.sendMessage"
    call_line INTEGER NOT NULL,
    
    -- Message details
    message_type TEXT NOT NULL,        -- "git_command", "project_command"
    method TEXT,                       -- "git.import", "project.create"
    
    -- Linking
    handler_id INTEGER,                -- Foreign key to websocket_handlers
    
    -- Metadata
    project_id TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (frontend_file_id) REFERENCES repository_files(id) ON DELETE CASCADE,
    FOREIGN KEY (handler_id) REFERENCES websocket_handlers(id) ON DELETE SET NULL
);

CREATE INDEX idx_ws_calls_frontend ON websocket_calls(frontend_file_id);
CREATE INDEX idx_ws_calls_type ON websocket_calls(message_type, method);
CREATE INDEX idx_ws_calls_project ON websocket_calls(project_id);

-- Backend WebSocket handlers (Rust)
CREATE TABLE IF NOT EXISTS websocket_handlers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Backend source
    backend_file_id INTEGER NOT NULL,
    handler_function TEXT NOT NULL,    -- "handle_git_command"
    handler_line INTEGER NOT NULL,
    
    -- Message details
    message_type TEXT NOT NULL,        -- "GitCommand", "ProjectCommand"
    method TEXT,                       -- "git.import", "project.create"
    
    -- Metadata
    project_id TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (backend_file_id) REFERENCES repository_files(id) ON DELETE CASCADE
);

CREATE INDEX idx_ws_handlers_backend ON websocket_handlers(backend_file_id);
CREATE INDEX idx_ws_handlers_type ON websocket_handlers(message_type, method);
CREATE INDEX idx_ws_handlers_project ON websocket_handlers(project_id);

-- Backend WebSocket responses (Rust)
CREATE TABLE IF NOT EXISTS websocket_responses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    
    -- Backend source
    backend_file_id INTEGER NOT NULL,
    sending_function TEXT NOT NULL,    -- "handle_git_import"
    response_line INTEGER NOT NULL,
    
    -- Response details
    response_type TEXT NOT NULL,       -- "Response", "Data", "Status", "Error"
    data_type TEXT,                    -- "git_status", "file_tree", etc.
    
    -- Metadata
    project_id TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    
    FOREIGN KEY (backend_file_id) REFERENCES repository_files(id) ON DELETE CASCADE
);

CREATE INDEX idx_ws_responses_backend ON websocket_responses(backend_file_id);
CREATE INDEX idx_ws_responses_type ON websocket_responses(response_type, data_type);
CREATE INDEX idx_ws_responses_project ON websocket_responses(project_id);
