// backend/src/cache/mod.rs

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tracing::{debug, info, warn};

/// LLM response cache for cost optimization
pub struct LlmCache {
    db: SqlitePool,
    enabled: bool,
    default_ttl_seconds: i64,
}

/// Cache key components for generating SHA-256 hash
#[derive(Debug, Clone, Serialize)]
struct CacheKeyData {
    messages: Vec<Value>,
    tools: Option<Vec<Value>>,
    system: String,
    model: String,
    reasoning_effort: Option<String>,
}

/// Cached response data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedResponse {
    pub response: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub cost_usd: f64,
    pub created_at: i64,
    pub last_accessed: i64,
    pub access_count: i64,
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: i64,
    pub total_hits: i64,
    pub total_size_bytes: i64,
    pub hit_rate: f64,
    pub avg_access_count: f64,
}

impl LlmCache {
    /// Create a new LLM cache
    pub fn new(db: SqlitePool, enabled: bool, default_ttl_seconds: i64) -> Self {
        Self {
            db,
            enabled,
            default_ttl_seconds,
        }
    }

    /// Generate a cache key from request components
    pub fn generate_key(
        messages: &[Value],
        tools: Option<&[Value]>,
        system: &str,
        model: &str,
        reasoning_effort: Option<&str>,
    ) -> Result<String> {
        let key_data = CacheKeyData {
            messages: messages.to_vec(),
            tools: tools.map(|t| t.to_vec()),
            system: system.to_string(),
            model: model.to_string(),
            reasoning_effort: reasoning_effort.map(|s| s.to_string()),
        };

        let json = serde_json::to_string(&key_data)?;
        let mut hasher = Sha256::new();
        hasher.update(json.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        Ok(hash)
    }

    /// Get a cached response if available and not expired
    pub async fn get(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        system: &str,
        model: &str,
        reasoning_effort: Option<&str>,
    ) -> Result<Option<CachedResponse>> {
        if !self.enabled {
            return Ok(None);
        }

        let key_hash = Self::generate_key(messages, tools, system, model, reasoning_effort)?;
        let now = Utc::now().timestamp();

        let result = sqlx::query!(
            r#"
            SELECT
                response, model, reasoning_effort,
                tokens_input, tokens_output, cost_usd,
                created_at, last_accessed, access_count,
                expires_at
            FROM llm_cache
            WHERE key_hash = ?
            "#,
            key_hash
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(row) = result {
            // Check if expired
            if let Some(expires_at) = row.expires_at {
                if now >= expires_at {
                    debug!("Cache entry expired: key={}", &key_hash[..8]);
                    self.delete(&key_hash).await?;
                    return Ok(None);
                }
            }

            // Update access count and last_accessed
            self.record_access(&key_hash).await?;

            debug!(
                "Cache hit: key={}, access_count={}, age={}s",
                &key_hash[..8],
                row.access_count + 1,
                now - row.created_at
            );

            Ok(Some(CachedResponse {
                response: row.response,
                model: row.model,
                reasoning_effort: row.reasoning_effort,
                tokens_input: row.tokens_input,
                tokens_output: row.tokens_output,
                cost_usd: row.cost_usd,
                created_at: row.created_at,
                last_accessed: now,
                access_count: row.access_count + 1,
            }))
        } else {
            debug!("Cache miss: key={}", &key_hash[..8]);
            Ok(None)
        }
    }

    /// Store a response in the cache
    pub async fn put(
        &self,
        messages: &[Value],
        tools: Option<&[Value]>,
        system: &str,
        model: &str,
        reasoning_effort: Option<&str>,
        response: &str,
        tokens_input: i64,
        tokens_output: i64,
        cost_usd: f64,
        ttl_seconds: Option<i64>,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let key_hash = Self::generate_key(messages, tools, system, model, reasoning_effort)?;
        let now = Utc::now().timestamp();
        let ttl = ttl_seconds.unwrap_or(self.default_ttl_seconds);
        let expires_at = if ttl > 0 { Some(now + ttl) } else { None };

        // Prepare request_data for storage
        let request_data = serde_json::to_string(&CacheKeyData {
            messages: messages.to_vec(),
            tools: tools.map(|t| t.to_vec()),
            system: system.to_string(),
            model: model.to_string(),
            reasoning_effort: reasoning_effort.map(|s| s.to_string()),
        })?;

        sqlx::query!(
            r#"
            INSERT INTO llm_cache (
                key_hash, request_data, response, model, reasoning_effort,
                tokens_input, tokens_output, cost_usd,
                created_at, last_accessed, access_count,
                ttl_seconds, expires_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, ?, ?)
            ON CONFLICT(key_hash) DO UPDATE SET
                response = excluded.response,
                last_accessed = excluded.last_accessed,
                access_count = access_count + 1
            "#,
            key_hash,
            request_data,
            response,
            model,
            reasoning_effort,
            tokens_input,
            tokens_output,
            cost_usd,
            now,
            now,
            ttl,
            expires_at
        )
        .execute(&self.db)
        .await?;

        debug!(
            "Cached response: key={}, model={}, ttl={}s",
            &key_hash[..8],
            model,
            ttl
        );

        Ok(())
    }

    /// Record an access to a cached entry (update access_count and last_accessed)
    async fn record_access(&self, key_hash: &str) -> Result<()> {
        let now = Utc::now().timestamp();

        sqlx::query!(
            r#"
            UPDATE llm_cache
            SET
                access_count = access_count + 1,
                last_accessed = ?
            WHERE key_hash = ?
            "#,
            now,
            key_hash
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Delete a cache entry
    async fn delete(&self, key_hash: &str) -> Result<()> {
        sqlx::query!(
            r#"
            DELETE FROM llm_cache
            WHERE key_hash = ?
            "#,
            key_hash
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Clean up expired cache entries
    pub async fn cleanup_expired(&self) -> Result<i64> {
        let now = Utc::now().timestamp();

        let result = sqlx::query!(
            r#"
            DELETE FROM llm_cache
            WHERE expires_at IS NOT NULL AND expires_at < ?
            "#,
            now
        )
        .execute(&self.db)
        .await?;

        let deleted = result.rows_affected() as i64;

        if deleted > 0 {
            info!("Cleaned up {} expired cache entries", deleted);
        }

        Ok(deleted)
    }

    /// Clean up least recently used entries if cache size exceeds limit
    pub async fn cleanup_lru(&self, max_entries: i64) -> Result<i64> {
        let count_result = sqlx::query!(
            r#"
            SELECT COUNT(*) as "count!: i64"
            FROM llm_cache
            "#
        )
        .fetch_one(&self.db)
        .await?;

        if count_result.count <= max_entries {
            return Ok(0);
        }

        let to_delete = count_result.count - max_entries;

        let result = sqlx::query!(
            r#"
            DELETE FROM llm_cache
            WHERE key_hash IN (
                SELECT key_hash
                FROM llm_cache
                ORDER BY last_accessed ASC
                LIMIT ?
            )
            "#,
            to_delete
        )
        .execute(&self.db)
        .await?;

        let deleted = result.rows_affected() as i64;

        if deleted > 0 {
            info!("Cleaned up {} LRU cache entries (limit: {})", deleted, max_entries);
        }

        Ok(deleted)
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats> {
        let result = sqlx::query!(
            r#"
            SELECT
                COUNT(*) as "total_entries!: i64",
                COALESCE(SUM(access_count), 0) as "total_hits!: i64",
                COALESCE(SUM(LENGTH(request_data) + LENGTH(response)), 0) as "total_size!: i64",
                COALESCE(AVG(access_count), 0.0) as "avg_access!: f64"
            FROM llm_cache
            "#
        )
        .fetch_one(&self.db)
        .await?;

        let hit_rate = if result.total_entries > 0 {
            result.total_hits as f64 / result.total_entries as f64
        } else {
            0.0
        };

        Ok(CacheStats {
            total_entries: result.total_entries,
            total_hits: result.total_hits,
            total_size_bytes: result.total_size,
            hit_rate,
            avg_access_count: result.avg_access,
        })
    }

    /// Get cache statistics for a specific model
    pub async fn get_stats_by_model(&self, model: &str) -> Result<CacheStats> {
        let result = sqlx::query!(
            r#"
            SELECT
                COUNT(*) as "total_entries!: i64",
                COALESCE(SUM(access_count), 0) as "total_hits!: i64",
                COALESCE(SUM(LENGTH(request_data) + LENGTH(response)), 0) as "total_size!: i64",
                COALESCE(AVG(access_count), 0.0) as "avg_access!: f64"
            FROM llm_cache
            WHERE model = ?
            "#,
            model
        )
        .fetch_one(&self.db)
        .await?;

        let hit_rate = if result.total_entries > 0 {
            result.total_hits as f64 / result.total_entries as f64
        } else {
            0.0
        };

        Ok(CacheStats {
            total_entries: result.total_entries,
            total_hits: result.total_hits,
            total_size_bytes: result.total_size,
            hit_rate,
            avg_access_count: result.avg_access,
        })
    }

    /// Clear all cache entries
    pub async fn clear_all(&self) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM llm_cache
            "#
        )
        .execute(&self.db)
        .await?;

        let deleted = result.rows_affected() as i64;
        warn!("Cleared all cache entries: {} deleted", deleted);

        Ok(deleted)
    }

    /// Clear cache entries for a specific model
    pub async fn clear_by_model(&self, model: &str) -> Result<i64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM llm_cache
            WHERE model = ?
            "#,
            model
        )
        .execute(&self.db)
        .await?;

        let deleted = result.rows_affected() as i64;
        info!("Cleared cache entries for model {}: {} deleted", model, deleted);

        Ok(deleted)
    }

    /// Check if cache is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get default TTL
    pub fn default_ttl(&self) -> i64 {
        self.default_ttl_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cache_key_generation() {
        let messages = vec![json!({"role": "user", "content": "Hello"})];
        let tools = None;
        let system = "You are a helpful assistant";
        let model = "gpt-5.1";
        let reasoning_effort = Some("medium");

        let key1 = LlmCache::generate_key(&messages, tools, system, model, reasoning_effort)
            .unwrap();
        let key2 = LlmCache::generate_key(&messages, tools, system, model, reasoning_effort)
            .unwrap();

        assert_eq!(key1, key2, "Same inputs should generate same key");
        assert_eq!(key1.len(), 64, "SHA-256 hash should be 64 hex chars");
    }

    #[test]
    fn test_cache_key_differs_on_reasoning_effort() {
        let messages = vec![json!({"role": "user", "content": "Hello"})];
        let tools = None;
        let system = "You are a helpful assistant";
        let model = "gpt-5.1";

        let key_medium = LlmCache::generate_key(&messages, tools, system, model, Some("medium"))
            .unwrap();
        let key_high = LlmCache::generate_key(&messages, tools, system, model, Some("high"))
            .unwrap();

        assert_ne!(key_medium, key_high, "Different reasoning efforts should generate different keys");
    }

    #[test]
    fn test_cache_key_differs_on_messages() {
        let messages1 = vec![json!({"role": "user", "content": "Hello"})];
        let messages2 = vec![json!({"role": "user", "content": "Hi"})];
        let tools = None;
        let system = "You are a helpful assistant";
        let model = "gpt-5.1";
        let reasoning_effort = Some("medium");

        let key1 = LlmCache::generate_key(&messages1, tools, system, model, reasoning_effort)
            .unwrap();
        let key2 = LlmCache::generate_key(&messages2, tools, system, model, reasoning_effort)
            .unwrap();

        assert_ne!(key1, key2, "Different messages should generate different keys");
    }
}
