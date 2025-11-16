-- backend/migrations/20251117_users_auth.sql
-- Add users table for authentication

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,  -- UUID format
    username TEXT NOT NULL UNIQUE,
    email TEXT UNIQUE,
    password_hash TEXT NOT NULL,
    display_name TEXT,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    last_login_at INTEGER,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE INDEX IF NOT EXISTS idx_users_username ON users(username);
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
CREATE INDEX IF NOT EXISTS idx_users_created_at ON users(created_at);

-- Add a default user for migration/testing (password: 'password123')
-- Hash generated with Rust bcrypt crate cost 12
INSERT OR IGNORE INTO users (id, username, email, password_hash, display_name, is_active)
VALUES (
    'peter-eternal',
    'peter',
    'peter@mira.local',
    '$2b$12$9zk9R/Lybz2Fg9qsDdEpi.ahiV0.Zq6y22sn4EwjacoDQXyO61A7S',
    'Peter',
    TRUE
);
