//! Artifact storage for tool outputs
//!
//! Stores large tool outputs (git diff, grep, file contents) in DB with:
//! - Compression (optional, via flate2/gzip)
//! - TTL for automatic cleanup
//! - Secret detection
//! - Head+tail excerpting for model-friendly previews
//! - Targeted retrieval via fetch_artifact / search_artifact tools

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

/// Thresholds for artifact vs inline decision
pub const INLINE_MAX_BYTES: usize = 2048;
pub const ARTIFACT_THRESHOLD_BYTES: usize = 4096;

/// TTL defaults (seconds)
pub const TTL_TOOL_OUTPUT: i64 = 7 * 24 * 60 * 60;  // 7 days
pub const TTL_DIFF: i64 = 30 * 24 * 60 * 60;        // 30 days
pub const TTL_SECRET: i64 = 24 * 60 * 60;           // 24 hours

/// Excerpt sizes for head+tail preview
const EXCERPT_HEAD: usize = 1200;
const EXCERPT_TAIL: usize = 800;

/// Tools that should be artifacted when output exceeds threshold
const ARTIFACT_TOOLS: &[&str] = &[
    "bash", "git_diff", "git_log", "grep", "read_file", "cargo_build", "cargo_test",
];

/// Patterns that indicate secrets (case-insensitive prefix match)
const SECRET_PATTERNS: &[(&str, &str)] = &[
    ("-----BEGIN RSA PRIVATE KEY-----", "private_key"),
    ("-----BEGIN EC PRIVATE KEY-----", "private_key"),
    ("-----BEGIN OPENSSH PRIVATE KEY-----", "private_key"),
    ("-----BEGIN PGP PRIVATE KEY-----", "private_key"),
    ("sk-proj-", "openai_key"),
    ("sk-ant-", "anthropic_key"),
    ("AIzaSy", "google_api_key"),
    ("ghp_", "github_pat"),
    ("github_pat_", "github_pat"),
    ("gho_", "github_oauth"),
    ("AWS_SECRET_ACCESS_KEY", "aws_secret"),
    ("PRIVATE_KEY=", "env_private_key"),
];

/// Result of artifact decision
#[derive(Debug)]
pub struct ArtifactDecision {
    pub should_artifact: bool,
    pub preview: String,
    pub artifact_id: Option<String>,
    pub total_bytes: usize,
    pub contains_secrets: bool,
    pub secret_reason: Option<String>,
}

/// Stored artifact metadata (without data blob)
#[derive(Debug, Clone)]
pub struct ArtifactMeta {
    pub id: String,
    pub kind: String,
    pub tool_name: Option<String>,
    pub uncompressed_bytes: i64,
    pub compressed_bytes: i64,
    pub contains_secrets: bool,
    pub preview_text: Option<String>,
}

/// Artifact store for managing tool output storage
#[derive(Clone)]
pub struct ArtifactStore {
    db: SqlitePool,
    project_path: String,
}

impl ArtifactStore {
    pub fn new(db: SqlitePool, project_path: String) -> Self {
        Self { db, project_path }
    }

    /// Decide whether to artifact this output and create preview
    pub fn decide(&self, tool_name: &str, output: &str) -> ArtifactDecision {
        let total_bytes = output.len();

        // Check for secrets first
        let (contains_secrets, secret_reason) = detect_secrets(output);

        // Decide based on tool + size
        let should_artifact = total_bytes > ARTIFACT_THRESHOLD_BYTES
            && ARTIFACT_TOOLS.iter().any(|t| tool_name.contains(t));

        // Create preview (head + tail for large outputs)
        let preview = if should_artifact || total_bytes > INLINE_MAX_BYTES {
            create_excerpt(output, EXCERPT_HEAD, EXCERPT_TAIL)
        } else {
            output.to_string()
        };

        ArtifactDecision {
            should_artifact,
            preview,
            artifact_id: None, // Set after storage
            total_bytes,
            contains_secrets,
            secret_reason,
        }
    }

    /// Store an artifact and return its ID
    pub async fn store(
        &self,
        kind: &str,
        tool_name: Option<&str>,
        tool_call_id: Option<&str>,
        content: &str,
        contains_secrets: bool,
        secret_reason: Option<&str>,
    ) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        // Compute TTL
        let ttl = if contains_secrets {
            TTL_SECRET
        } else if kind == "diff" {
            TTL_DIFF
        } else {
            TTL_TOOL_OUTPUT
        };
        let expires_at = now + ttl;

        // Hash for deduplication
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let sha256 = format!("{:x}", hasher.finalize());

        // For now, no compression (can add zstd later)
        let data = content.as_bytes();
        let uncompressed_bytes = data.len() as i64;
        let compressed_bytes = uncompressed_bytes; // No compression yet

        // Create preview and searchable text
        let preview_text = create_excerpt(content, EXCERPT_HEAD, EXCERPT_TAIL);
        let searchable_text = if content.len() > 16384 {
            // First 16KB for search
            content.chars().take(16384).collect::<String>()
        } else {
            content.to_string()
        };

        sqlx::query(
            r#"
            INSERT INTO artifacts (
                id, created_at, expires_at, project_path,
                kind, tool_name, tool_call_id, message_id,
                content_type, encoding, compression,
                uncompressed_bytes, compressed_bytes,
                sha256, contains_secrets, secret_reason,
                preview_text, data, searchable_text
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, NULL,
                'text/plain; charset=utf-8', 'utf-8', 'none',
                $8, $9,
                $10, $11, $12,
                $13, $14, $15
            )
            "#,
        )
        .bind(&id)
        .bind(now)
        .bind(expires_at)
        .bind(&self.project_path)
        .bind(kind)
        .bind(tool_name)
        .bind(tool_call_id)
        .bind(uncompressed_bytes)
        .bind(compressed_bytes)
        .bind(&sha256)
        .bind(contains_secrets as i32)
        .bind(secret_reason)
        .bind(&preview_text)
        .bind(data)
        .bind(&searchable_text)
        .execute(&self.db)
        .await?;

        Ok(id)
    }

    /// Fetch a slice of an artifact
    pub async fn fetch(&self, artifact_id: &str, offset: usize, limit: usize) -> Result<Option<FetchResult>> {
        // Cap limit to 16KB
        let limit = limit.min(16384);

        let row: Option<(Vec<u8>, i64, i32)> = sqlx::query_as(
            "SELECT data, uncompressed_bytes, contains_secrets FROM artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_optional(&self.db)
        .await?;

        let Some((data, total_bytes, contains_secrets)) = row else {
            return Ok(None);
        };

        // Check secrets
        if contains_secrets != 0 {
            // For now, refuse to fetch secret artifacts
            return Ok(Some(FetchResult {
                artifact_id: artifact_id.to_string(),
                offset,
                limit,
                total_bytes: total_bytes as usize,
                content: "[REDACTED: artifact contains potential secrets]".to_string(),
                truncated: false,
            }));
        }

        // Convert to string (assuming UTF-8, no compression)
        let text = String::from_utf8_lossy(&data);

        // Extract slice using chars to handle UTF-8 properly
        let chars: Vec<char> = text.chars().collect();
        let start = offset.min(chars.len());
        let end = (start + limit).min(chars.len());
        let content: String = chars[start..end].iter().collect();
        let truncated = end < chars.len();

        Ok(Some(FetchResult {
            artifact_id: artifact_id.to_string(),
            offset: start,
            limit: end - start,
            total_bytes: total_bytes as usize,
            content,
            truncated,
        }))
    }

    /// Search within an artifact
    pub async fn search(
        &self,
        artifact_id: &str,
        query: &str,
        max_results: usize,
        context_bytes: usize,
    ) -> Result<Option<SearchResult>> {
        // Cap parameters
        let max_results = max_results.min(20);
        let context_bytes = context_bytes.min(500);

        let row: Option<(Vec<u8>, i64, i32)> = sqlx::query_as(
            "SELECT data, uncompressed_bytes, contains_secrets FROM artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_optional(&self.db)
        .await?;

        let Some((data, total_bytes, contains_secrets)) = row else {
            return Ok(None);
        };

        if contains_secrets != 0 {
            return Ok(Some(SearchResult {
                artifact_id: artifact_id.to_string(),
                query: query.to_string(),
                total_bytes: total_bytes as usize,
                matches: vec![],
                note: Some("Cannot search artifact containing secrets".to_string()),
            }));
        }

        let text = String::from_utf8_lossy(&data);
        let query_lower = query.to_lowercase();
        let text_lower = text.to_lowercase();

        let mut matches = Vec::new();
        let mut search_start = 0;

        while matches.len() < max_results {
            if let Some(pos) = text_lower[search_start..].find(&query_lower) {
                let absolute_pos = search_start + pos;

                // Get context around match
                let context_start = absolute_pos.saturating_sub(context_bytes / 2);
                let context_end = (absolute_pos + query.len() + context_bytes / 2).min(text.len());

                // Safe char slicing
                let chars: Vec<char> = text.chars().collect();
                let char_start = text[..context_start].chars().count();
                let char_end = text[..context_end].chars().count().min(chars.len());
                let preview: String = chars[char_start..char_end].iter().collect();

                matches.push(SearchMatch {
                    match_index: matches.len() + 1,
                    offset: absolute_pos,
                    preview,
                    suggest_fetch_offset: context_start.saturating_sub(200),
                    suggest_fetch_limit: 800,
                });

                search_start = absolute_pos + query.len();
            } else {
                break;
            }
        }

        Ok(Some(SearchResult {
            artifact_id: artifact_id.to_string(),
            query: query.to_string(),
            total_bytes: total_bytes as usize,
            matches,
            note: None,
        }))
    }

    /// Link artifact to message (called after message is saved)
    pub async fn link_to_message(&self, artifact_id: &str, message_id: &str) -> Result<()> {
        sqlx::query("UPDATE artifacts SET message_id = $1 WHERE id = $2")
            .bind(message_id)
            .bind(artifact_id)
            .execute(&self.db)
            .await?;
        Ok(())
    }

    /// Cleanup expired artifacts
    pub async fn cleanup_expired(&self) -> Result<u64> {
        let now = Utc::now().timestamp();
        let result = sqlx::query("DELETE FROM artifacts WHERE expires_at IS NOT NULL AND expires_at < $1")
            .bind(now)
            .execute(&self.db)
            .await?;
        Ok(result.rows_affected())
    }
}

/// Result of fetch_artifact
#[derive(Debug, serde::Serialize)]
pub struct FetchResult {
    pub artifact_id: String,
    pub offset: usize,
    pub limit: usize,
    pub total_bytes: usize,
    pub content: String,
    pub truncated: bool,
}

/// Result of search_artifact
#[derive(Debug, serde::Serialize)]
pub struct SearchResult {
    pub artifact_id: String,
    pub query: String,
    pub total_bytes: usize,
    pub matches: Vec<SearchMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct SearchMatch {
    pub match_index: usize,
    pub offset: usize,
    pub preview: String,
    pub suggest_fetch_offset: usize,
    pub suggest_fetch_limit: usize,
}

/// Detect potential secrets in content
fn detect_secrets(content: &str) -> (bool, Option<String>) {
    let content_lower = content.to_lowercase();

    for (pattern, reason) in SECRET_PATTERNS {
        if content.contains(pattern) || content_lower.contains(&pattern.to_lowercase()) {
            return (true, Some(reason.to_string()));
        }
    }

    (false, None)
}

/// Create head+tail excerpt with UTF-8 safe slicing
fn create_excerpt(content: &str, head_chars: usize, tail_chars: usize) -> String {
    let chars: Vec<char> = content.chars().collect();
    let total = chars.len();

    if total <= head_chars + tail_chars + 50 {
        // Small enough to include entirely
        return content.to_string();
    }

    let head: String = chars[..head_chars].iter().collect();
    let tail: String = chars[total - tail_chars..].iter().collect();

    format!(
        "{}\n\n…[truncated {} chars, use fetch_artifact for full content]…\n\n{}",
        head,
        total - head_chars - tail_chars,
        tail
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_secrets() {
        assert!(detect_secrets("sk-proj-abc123").0);
        assert!(detect_secrets("ghp_xxxxxxxxxxxx").0);
        assert!(detect_secrets("-----BEGIN RSA PRIVATE KEY-----").0);
        assert!(!detect_secrets("normal output").0);
    }

    #[test]
    fn test_create_excerpt() {
        let short = "short content";
        assert_eq!(create_excerpt(short, 1200, 800), short);

        let long = "a".repeat(5000);
        let excerpt = create_excerpt(&long, 100, 50);
        assert!(excerpt.contains("truncated"));
        assert!(excerpt.starts_with(&"a".repeat(100)));
        assert!(excerpt.ends_with(&"a".repeat(50)));
    }
}
