// src/watcher/config.rs
// Configuration for the file watcher service

use std::env;

/// Configuration for the file watcher
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Whether file watching is enabled
    pub enabled: bool,
    /// Debounce timeout per file (ms)
    pub debounce_ms: u64,
    /// Batch processing window (ms)
    pub batch_ms: u64,
    /// Cooldown after git operations (ms) - events during this window are suppressed
    pub git_cooldown_ms: u64,
    /// Maximum events to process in a single batch
    pub max_batch_size: usize,
    /// Delay between processing individual files (ms) - prevents CPU spikes
    pub process_delay_ms: u64,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: 300,
            batch_ms: 1000,
            git_cooldown_ms: 3000,
            max_batch_size: 100,
            process_delay_ms: 50,
        }
    }
}

impl WatcherConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            enabled: env::var("FILE_WATCHER_ENABLED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(true),
            debounce_ms: env::var("FILE_WATCHER_DEBOUNCE_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            batch_ms: env::var("FILE_WATCHER_BATCH_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1000),
            git_cooldown_ms: env::var("FILE_WATCHER_GIT_COOLDOWN_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            max_batch_size: env::var("FILE_WATCHER_MAX_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100),
            process_delay_ms: env::var("FILE_WATCHER_PROCESS_DELAY_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(50),
        }
    }
}

/// Directories to ignore when watching
pub const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "vendor",
    ".cargo",
    "__pycache__",
    ".venv",
    "coverage",
    ".cache",
    ".turbo",
];

/// File extensions to watch and process
pub const WATCHED_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "mjs"];

/// Check if a path component should be ignored
pub fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS.contains(&name)
}

/// Check if a file extension should be processed
pub fn should_process_extension(ext: &str) -> bool {
    WATCHED_EXTENSIONS.contains(&ext)
}
