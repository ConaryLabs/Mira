// src/config/helpers.rs
// Helper functions for loading environment variables

use std::env;

pub fn require_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("Missing required env var: {}", key))
}

pub fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

pub fn env_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

pub fn require_env_parsed<T: std::str::FromStr>(key: &str) -> T
where
    T::Err: std::fmt::Display,
{
    env::var(key)
        .unwrap_or_else(|_| panic!("Missing required env var: {}", key))
        .parse()
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", key, e))
}
