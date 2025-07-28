use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

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
