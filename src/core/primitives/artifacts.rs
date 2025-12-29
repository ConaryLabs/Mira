//! Artifact storage for large outputs
//!
//! Stores large tool outputs in database with:
//! - SHA256 deduplication
//! - TTL for automatic cleanup
//! - Secret detection integration
//! - Smart excerpting for model-friendly previews
//! - Targeted retrieval via fetch/search

use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::limits::{
    ARTIFACT_THRESHOLD_BYTES, DEFAULT_FETCH_LIMIT, EXCERPT_HEAD_CHARS, EXCERPT_TAIL_CHARS,
    INLINE_MAX_BYTES, MAX_ARTIFACT_SIZE, PROJECT_ARTIFACT_CAP_BYTES, TTL_DIFF_SECS,
    TTL_SECRET_SECS, TTL_TOOL_OUTPUT_SECS,
};

use super::secrets::detect_secrets;

use super::excerpts::create_smart_excerpt;

/// Tools that should be artifacted when output exceeds threshold
const ARTIFACT_TOOLS: &[&str] = &[
    "bash",
    "git_diff",
    "git_log",
    "grep",
    "read_file",
    "cargo_build",
    "cargo_test",
];

/// Result of artifact decision
#[derive(Debug)]
pub struct ArtifactDecision {
    pub should_artifact: bool,
    pub preview: String,
    pub artifact_id: Option<String>,
    pub total_bytes: usize,
    pub contains_secrets: bool,
    pub secret_kind: Option<String>,
}

/// Lightweight reference to an artifact (for cross-crate sharing)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub kind: String,
    pub total_bytes: usize,
    pub preview: Option<String>,
    pub contains_secrets: bool,
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

/// Storage statistics
#[derive(Debug)]
pub struct ArtifactStats {
    pub count: u64,
    pub total_bytes: u64,
}

/// Loaded artifact data (internal)
struct ArtifactData {
    text: String,
    total_bytes: usize,
    contains_secrets: bool,
}

/// Artifact store for managing large output storage
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

        // Check for secrets
        let (contains_secrets, secret_kind) = {
            let secret = detect_secrets(output);
            (secret.is_some(), secret.map(|s| s.kind.to_string()))
        };

        // Decide based on tool + size
        let should_artifact = total_bytes > ARTIFACT_THRESHOLD_BYTES
            && ARTIFACT_TOOLS.iter().any(|t| tool_name.contains(t));

        // Create smart preview based on tool type
        let preview = if should_artifact || total_bytes > INLINE_MAX_BYTES {
            create_smart_excerpt(tool_name, output)
        } else {
            output.to_string()
        };

        ArtifactDecision {
            should_artifact,
            preview,
            artifact_id: None, // Set after storage
            total_bytes,
            contains_secrets,
            secret_kind,
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
        // Enforce max size to prevent unbounded storage
        if content.len() > MAX_ARTIFACT_SIZE {
            anyhow::bail!(
                "Artifact too large: {} bytes (max {})",
                content.len(),
                MAX_ARTIFACT_SIZE
            );
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().timestamp();

        // Compute TTL
        let ttl = if contains_secrets {
            TTL_SECRET_SECS
        } else if kind == "diff" {
            TTL_DIFF_SECS
        } else {
            TTL_TOOL_OUTPUT_SECS
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

        // Create preview
        let preview_text = {
            use super::excerpts::create_excerpt;
            create_excerpt(content, EXCERPT_HEAD_CHARS, EXCERPT_TAIL_CHARS)
        };

        // Searchable text (first 16KB)
        let searchable_text = if content.len() > 16384 {
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

    /// Load artifact data (shared by fetch/search)
    async fn load_artifact_data(&self, artifact_id: &str) -> Result<Option<ArtifactData>> {
        let row: Option<(Vec<u8>, i64, i32)> = sqlx::query_as(
            "SELECT data, uncompressed_bytes, contains_secrets FROM artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_optional(&self.db)
        .await?;

        let Some((data, total_bytes, contains_secrets)) = row else {
            return Ok(None);
        };

        Ok(Some(ArtifactData {
            text: String::from_utf8_lossy(&data).into_owned(),
            total_bytes: total_bytes as usize,
            contains_secrets: contains_secrets != 0,
        }))
    }

    /// Fetch a slice of an artifact
    pub async fn fetch(
        &self,
        artifact_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Option<FetchResult>> {
        // Cap limit to default fetch limit
        let limit = limit.min(DEFAULT_FETCH_LIMIT);

        let Some(artifact) = self.load_artifact_data(artifact_id).await? else {
            return Ok(None);
        };

        // Redact secrets instead of blocking entirely
        // This allows Mira to read the artifact content with just the secrets masked
        let text = if artifact.contains_secrets {
            super::secrets::redact_secrets(&artifact.text)
        } else {
            artifact.text.clone()
        };

        // UTF-8 safe slice using byte boundaries
        let (content, actual_start, actual_end) = safe_utf8_slice(&text, offset, limit);
        let truncated = actual_end < text.len();

        Ok(Some(FetchResult {
            artifact_id: artifact_id.to_string(),
            offset: actual_start,
            limit: actual_end - actual_start,
            total_bytes: text.len(),
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

        let Some(artifact) = self.load_artifact_data(artifact_id).await? else {
            return Ok(None);
        };

        // Redact secrets but still allow searching
        let text = if artifact.contains_secrets {
            super::secrets::redact_secrets(&artifact.text)
        } else {
            artifact.text.clone()
        };

        let query_lower = query.to_lowercase();
        let text_lower = text.to_lowercase();

        let mut matches = Vec::new();
        let mut search_start = 0;

        while matches.len() < max_results {
            if let Some(pos) = text_lower[search_start..].find(&query_lower) {
                let absolute_pos = search_start + pos;

                // Get context around match using byte-safe slicing
                let context_start = absolute_pos.saturating_sub(context_bytes / 2);
                let context_end =
                    (absolute_pos + query.len() + context_bytes / 2).min(text.len());

                let (preview, _, _) =
                    safe_utf8_slice(&text, context_start, context_end - context_start);

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
            total_bytes: text.len(),
            matches,
            note: if artifact.contains_secrets { Some("Secrets redacted".to_string()) } else { None },
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
        let result =
            sqlx::query("DELETE FROM artifacts WHERE expires_at IS NOT NULL AND expires_at < $1")
                .bind(now)
                .execute(&self.db)
                .await?;
        Ok(result.rows_affected())
    }

    /// Enforce size cap per project - delete oldest artifacts if over limit
    /// Returns number of artifacts deleted
    pub async fn enforce_size_cap(&self, max_bytes: i64) -> Result<u64> {
        // Get current total size for project
        let total: (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(compressed_bytes), 0) FROM artifacts WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_one(&self.db)
        .await?;

        if total.0 <= max_bytes {
            return Ok(0);
        }

        let excess = total.0 - max_bytes;
        let mut deleted = 0u64;
        let mut freed = 0i64;

        // Delete oldest artifacts until under cap
        let oldest: Vec<(String, i64)> = sqlx::query_as(
            "SELECT id, compressed_bytes FROM artifacts WHERE project_path = $1 ORDER BY created_at ASC LIMIT 100",
        )
        .bind(&self.project_path)
        .fetch_all(&self.db)
        .await?;

        for (id, size) in oldest {
            if freed >= excess {
                break;
            }
            sqlx::query("DELETE FROM artifacts WHERE id = $1")
                .bind(&id)
                .execute(&self.db)
                .await?;
            freed += size;
            deleted += 1;
        }

        Ok(deleted)
    }

    /// Check for existing artifact with same sha256 (dedupe)
    pub async fn find_by_sha256(&self, sha256: &str) -> Result<Option<String>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM artifacts WHERE project_path = $1 AND sha256 = $2 LIMIT 1",
        )
        .bind(&self.project_path)
        .bind(sha256)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|(id,)| id))
    }

    /// Compute SHA256 hash of content
    pub fn compute_sha256(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Store an artifact with dedupe - returns existing ID if content already exists
    /// Returns (id, was_dedupe_hit)
    pub async fn store_deduped(
        &self,
        kind: &str,
        tool_name: Option<&str>,
        tool_call_id: Option<&str>,
        content: &str,
        contains_secrets: bool,
        secret_reason: Option<&str>,
    ) -> Result<(String, bool)> {
        let sha256 = Self::compute_sha256(content);

        // Check for existing
        if let Some(existing_id) = self.find_by_sha256(&sha256).await? {
            return Ok((existing_id, true)); // true = was dedupe hit
        }

        // Store new
        let id = self
            .store(
                kind,
                tool_name,
                tool_call_id,
                content,
                contains_secrets,
                secret_reason,
            )
            .await?;
        Ok((id, false))
    }

    /// Run full maintenance: TTL cleanup + size cap enforcement
    /// Returns (expired_deleted, cap_deleted)
    pub async fn maintenance(&self) -> Result<(u64, u64)> {
        let expired = self.cleanup_expired().await?;
        let capped = self.enforce_size_cap(PROJECT_ARTIFACT_CAP_BYTES).await?;
        Ok((expired, capped))
    }

    /// Get storage stats for project
    pub async fn stats(&self) -> Result<ArtifactStats> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(compressed_bytes), 0) FROM artifacts WHERE project_path = $1",
        )
        .bind(&self.project_path)
        .fetch_one(&self.db)
        .await?;

        Ok(ArtifactStats {
            count: row.0 as u64,
            total_bytes: row.1 as u64,
        })
    }

    /// Create an ArtifactRef from stored artifact
    pub async fn get_ref(&self, artifact_id: &str) -> Result<Option<ArtifactRef>> {
        let row: Option<(String, String, i64, Option<String>, i32)> = sqlx::query_as(
            "SELECT id, kind, uncompressed_bytes, preview_text, contains_secrets FROM artifacts WHERE id = $1",
        )
        .bind(artifact_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(row.map(|(id, kind, bytes, preview, secrets)| ArtifactRef {
            id,
            kind,
            total_bytes: bytes as usize,
            preview,
            contains_secrets: secrets != 0,
        }))
    }
}

/// UTF-8 safe byte slicing - finds valid char boundaries
/// Returns (slice, actual_start, actual_end) where boundaries are adjusted to valid UTF-8
fn safe_utf8_slice(text: &str, start: usize, limit: usize) -> (String, usize, usize) {
    let bytes = text.as_bytes();
    let len = bytes.len();

    if start >= len {
        return (String::new(), len, len);
    }

    // Find valid start boundary (move forward to char boundary)
    let mut actual_start = start.min(len);
    while actual_start < len && !text.is_char_boundary(actual_start) {
        actual_start += 1;
    }

    // Find valid end boundary (move backward to char boundary)
    let mut actual_end = (actual_start + limit).min(len);
    while actual_end > actual_start && !text.is_char_boundary(actual_end) {
        actual_end -= 1;
    }

    let content = text[actual_start..actual_end].to_string();
    (content, actual_start, actual_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_sha256() {
        let hash = ArtifactStore::compute_sha256("hello world");
        assert_eq!(hash.len(), 64); // SHA256 produces 64 hex chars
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_safe_utf8_slice() {
        let text = "hello";
        let (slice, start, end) = safe_utf8_slice(text, 0, 3);
        assert_eq!(slice, "hel");
        assert_eq!(start, 0);
        assert_eq!(end, 3);
    }
}
