// src/git/types.rs

use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use std::str::FromStr;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i64)]
pub enum GitImportStatus {
    Pending = 0,
    Cloned = 1,
    Imported = 2,
    Synced = 3,
}

impl From<i64> for GitImportStatus {
    fn from(val: i64) -> Self {
        match val {
            1 => GitImportStatus::Cloned,
            2 => GitImportStatus::Imported,
            3 => GitImportStatus::Synced,
            _ => GitImportStatus::Pending,
        }
    }
}

// Implement FromStr for parsing from database TEXT field
impl FromStr for GitImportStatus {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try to parse as integer first (for backward compatibility)
        if let Ok(num) = s.parse::<i64>() {
            return Ok(GitImportStatus::from(num));
        }
        
        // Parse string representation
        match s.to_lowercase().as_str() {
            "pending" => Ok(GitImportStatus::Pending),
            "cloned" => Ok(GitImportStatus::Cloned),
            "imported" => Ok(GitImportStatus::Imported),
            "synced" => Ok(GitImportStatus::Synced),
            _ => Ok(GitImportStatus::Pending), // Default fallback
        }
    }
}

// Implement Display for storing in database as TEXT
impl fmt::Display for GitImportStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            GitImportStatus::Pending => "pending",
            GitImportStatus::Cloned => "cloned",
            GitImportStatus::Imported => "imported",
            GitImportStatus::Synced => "synced",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitRepoAttachment {
    pub id: String,
    pub project_id: String,
    pub repo_url: String,
    pub local_path: String,
    pub import_status: GitImportStatus,
    pub last_imported_at: Option<DateTime<Utc>>,
    pub last_sync_at: Option<DateTime<Utc>>,
}

// Re-export the Phase 3 types from client.rs
