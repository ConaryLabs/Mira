//! Index freshness checking

use sqlx::sqlite::SqlitePool;
use std::process::Command;

use super::types::IndexStatus;

/// Check index freshness for files in the project
/// Returns status showing stale files that may have outdated symbol information
pub async fn check_index_freshness(
    db: &SqlitePool,
    project_path: &str,
) -> Option<IndexStatus> {
    // Get last indexed time for the project
    let last_indexed: Option<(i64,)> = sqlx::query_as(
        r#"
        SELECT MAX(analyzed_at) FROM code_symbols
        WHERE file_path LIKE $1
        "#,
    )
    .bind(format!("{}%", project_path))
    .fetch_optional(db)
    .await
    .ok()?;

    let last_indexed_ts = last_indexed.map(|r| r.0);

    // Get list of files modified since last index
    let stale_files = get_stale_files(db, project_path, last_indexed_ts).await;

    if stale_files.is_empty() && last_indexed_ts.is_none() {
        return None;
    }

    Some(IndexStatus {
        stale_files,
        last_indexed: last_indexed_ts,
    })
}

/// Get files modified since last index
async fn get_stale_files(
    db: &SqlitePool,
    project_path: &str,
    since: Option<i64>,
) -> Vec<String> {
    let Some(since_ts) = since else {
        return Vec::new();
    };

    // Use git to find files modified since the timestamp
    let since_date = chrono::DateTime::from_timestamp(since_ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default();

    if since_date.is_empty() {
        return Vec::new();
    }

    // Get files modified since the index time using git
    let output = Command::new("git")
        .args([
            "diff", "--name-only",
            &format!("--since={}", since_date),
            "HEAD",
        ])
        .current_dir(project_path)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter(|l| l.ends_with(".rs") || l.ends_with(".ts") || l.ends_with(".py"))
                .take(10)
                .map(|s| s.to_string())
                .collect()
        }
        _ => {
            // Fallback: check file mtime vs index time
            check_file_mtimes(db, project_path, since_ts).await
        }
    }
}

/// Fallback: check file modification times vs index time
async fn check_file_mtimes(
    db: &SqlitePool,
    project_path: &str,
    _since_ts: i64,
) -> Vec<String> {
    // Get indexed files with their analyzed_at times
    let indexed_files: Vec<(String, i64)> = sqlx::query_as(
        r#"
        SELECT DISTINCT file_path, MAX(analyzed_at) as last_analyzed
        FROM code_symbols
        WHERE file_path LIKE $1
        GROUP BY file_path
        LIMIT 100
        "#,
    )
    .bind(format!("{}%", project_path))
    .fetch_all(db)
    .await
    .unwrap_or_default();

    let mut stale = Vec::new();

    for (file_path, analyzed_at) in indexed_files {
        if let Ok(metadata) = std::fs::metadata(&file_path) {
            if let Ok(mtime) = metadata.modified() {
                let mtime_ts = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);

                if mtime_ts > analyzed_at {
                    // File was modified after indexing
                    stale.push(file_path);
                }
            }
        }

        if stale.len() >= 10 {
            break;
        }
    }

    stale
}
