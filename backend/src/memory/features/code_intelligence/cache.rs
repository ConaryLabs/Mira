// backend/src/memory/features/code_intelligence/cache.rs
// Semantic analysis and pattern validation caching

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::sync::Arc;

/// Cached semantic analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticCacheEntry {
    pub id: i64,
    pub symbol_id: i64,
    pub code_hash: String,
    pub analysis_result: SemanticAnalysisResult,
    pub confidence: f64,
    pub created_at: i64,
    pub last_used: i64,
    pub hit_count: i64,
}

/// The actual analysis result stored in cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnalysisResult {
    pub purpose: String,
    pub description: Option<String>,
    pub concepts: Vec<String>,
    pub domain_labels: Vec<String>,
}

/// Cached pattern validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCacheEntry {
    pub id: i64,
    pub pattern_type: String,
    pub code_hash: String,
    pub validation_result: PatternValidationResult,
    pub confidence: f64,
    pub created_at: i64,
    pub last_used: i64,
    pub hit_count: i64,
}

/// The actual validation result stored in cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternValidationResult {
    pub is_pattern: bool,
    pub pattern_name: Option<String>,
    pub involved_symbols: Vec<i64>,
    pub description: Option<String>,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub total_entries: i64,
    pub total_hits: i64,
    pub avg_confidence: f64,
    pub oldest_entry_age_days: i64,
}

/// Service for managing semantic analysis cache
pub struct SemanticCacheService {
    db: Arc<SqlitePool>,
}

impl SemanticCacheService {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Compute hash for code content
    pub fn compute_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get cached analysis for a symbol if hash matches
    pub async fn get_cached(
        &self,
        symbol_id: i64,
        code_hash: &str,
    ) -> Result<Option<SemanticCacheEntry>> {
        let row = sqlx::query!(
            r#"
            SELECT id, symbol_id, code_hash, analysis_result, confidence,
                   created_at, last_used, hit_count
            FROM semantic_analysis_cache
            WHERE symbol_id = ? AND code_hash = ?
            "#,
            symbol_id,
            code_hash
        )
        .fetch_optional(self.db.as_ref())
        .await?;

        if let Some(r) = row {
            let entry_id = r.id.unwrap_or(0);
            // Update hit count and last_used
            let now = Utc::now().timestamp();
            let new_hit_count = r.hit_count + 1;
            sqlx::query!(
                "UPDATE semantic_analysis_cache SET hit_count = ?, last_used = ? WHERE id = ?",
                new_hit_count,
                now,
                entry_id
            )
            .execute(self.db.as_ref())
            .await?;

            let analysis_result: SemanticAnalysisResult =
                serde_json::from_str(&r.analysis_result)?;

            Ok(Some(SemanticCacheEntry {
                id: entry_id,
                symbol_id: r.symbol_id,
                code_hash: r.code_hash,
                analysis_result,
                confidence: r.confidence,
                created_at: r.created_at,
                last_used: now,
                hit_count: new_hit_count,
            }))
        } else {
            Ok(None)
        }
    }

    /// Store analysis result in cache
    pub async fn store(
        &self,
        symbol_id: i64,
        code_hash: &str,
        result: &SemanticAnalysisResult,
        confidence: f64,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let result_json = serde_json::to_string(result)?;

        // Use INSERT OR REPLACE to handle existing entries
        let result = sqlx::query!(
            r#"
            INSERT OR REPLACE INTO semantic_analysis_cache
            (symbol_id, code_hash, analysis_result, confidence, created_at, last_used, hit_count)
            VALUES (?, ?, ?, ?, ?, ?, 0)
            "#,
            symbol_id,
            code_hash,
            result_json,
            confidence,
            now,
            now
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Invalidate cache entry for a symbol
    pub async fn invalidate(&self, symbol_id: i64) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM semantic_analysis_cache WHERE symbol_id = ?",
            symbol_id
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Evict old cache entries based on last_used timestamp
    pub async fn evict_old_entries(&self, max_age_days: i64) -> Result<i64> {
        let cutoff = Utc::now().timestamp() - (max_age_days * 24 * 60 * 60);

        let result = sqlx::query!(
            "DELETE FROM semantic_analysis_cache WHERE last_used < ?",
            cutoff
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Evict least frequently used entries to maintain cache size
    pub async fn evict_lfu(&self, max_entries: i64) -> Result<i64> {
        // Count current entries
        let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) as count FROM semantic_analysis_cache")
            .fetch_one(self.db.as_ref())
            .await? as i64;

        if count <= max_entries {
            return Ok(0);
        }

        let to_delete = count - max_entries;

        // Delete entries with lowest hit counts
        let result = sqlx::query!(
            r#"
            DELETE FROM semantic_analysis_cache
            WHERE id IN (
                SELECT id FROM semantic_analysis_cache
                ORDER BY hit_count ASC, last_used ASC
                LIMIT ?
            )
            "#,
            to_delete
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats> {
        let total_entries: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) as count FROM semantic_analysis_cache")
                .fetch_one(self.db.as_ref())
                .await? as i64;

        let total_hits: i64 =
            sqlx::query_scalar!("SELECT COALESCE(SUM(hit_count), 0) as total FROM semantic_analysis_cache")
                .fetch_one(self.db.as_ref())
                .await? as i64;

        let avg_confidence: f64 =
            sqlx::query_scalar!("SELECT COALESCE(AVG(confidence), 0.0) as avg FROM semantic_analysis_cache")
                .fetch_one(self.db.as_ref())
                .await?;

        let now = Utc::now().timestamp();
        let oldest_created: Option<i64> =
            sqlx::query_scalar!("SELECT MIN(created_at) FROM semantic_analysis_cache")
                .fetch_one(self.db.as_ref())
                .await?;

        let oldest_entry_age_days = oldest_created
            .map(|created| (now - created) / (24 * 60 * 60))
            .unwrap_or(0);

        Ok(CacheStats {
            total_entries,
            total_hits,
            avg_confidence,
            oldest_entry_age_days,
        })
    }
}

/// Service for managing pattern validation cache
pub struct PatternCacheService {
    db: Arc<SqlitePool>,
}

impl PatternCacheService {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self { db }
    }

    /// Compute hash for code content
    pub fn compute_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Get cached validation for pattern type and code hash
    pub async fn get_cached(
        &self,
        pattern_type: &str,
        code_hash: &str,
    ) -> Result<Option<PatternCacheEntry>> {
        let row = sqlx::query!(
            r#"
            SELECT id, pattern_type, code_hash, validation_result, confidence,
                   created_at, last_used, hit_count
            FROM pattern_validation_cache
            WHERE pattern_type = ? AND code_hash = ?
            "#,
            pattern_type,
            code_hash
        )
        .fetch_optional(self.db.as_ref())
        .await?;

        if let Some(r) = row {
            let entry_id = r.id.unwrap_or(0);
            // Update hit count and last_used
            let now = Utc::now().timestamp();
            let new_hit_count = r.hit_count.unwrap_or(0) + 1;
            sqlx::query!(
                "UPDATE pattern_validation_cache SET hit_count = ?, last_used = ? WHERE id = ?",
                new_hit_count,
                now,
                entry_id
            )
            .execute(self.db.as_ref())
            .await?;

            let validation_result: PatternValidationResult =
                serde_json::from_str(&r.validation_result)?;

            Ok(Some(PatternCacheEntry {
                id: entry_id,
                pattern_type: r.pattern_type,
                code_hash: r.code_hash,
                validation_result,
                confidence: r.confidence,
                created_at: r.created_at,
                last_used: now,
                hit_count: new_hit_count,
            }))
        } else {
            Ok(None)
        }
    }

    /// Store validation result in cache
    pub async fn store(
        &self,
        pattern_type: &str,
        code_hash: &str,
        result: &PatternValidationResult,
        confidence: f64,
    ) -> Result<i64> {
        let now = Utc::now().timestamp();
        let result_json = serde_json::to_string(result)?;

        // Use INSERT OR REPLACE to handle existing entries
        let result = sqlx::query!(
            r#"
            INSERT OR REPLACE INTO pattern_validation_cache
            (pattern_type, code_hash, validation_result, confidence, created_at, last_used, hit_count)
            VALUES (?, ?, ?, ?, ?, ?, 0)
            "#,
            pattern_type,
            code_hash,
            result_json,
            confidence,
            now,
            now
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Invalidate all cache entries for a pattern type
    pub async fn invalidate_pattern_type(&self, pattern_type: &str) -> Result<i64> {
        let result = sqlx::query!(
            "DELETE FROM pattern_validation_cache WHERE pattern_type = ?",
            pattern_type
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Invalidate cache entry by code hash
    pub async fn invalidate_by_hash(&self, code_hash: &str) -> Result<i64> {
        let result = sqlx::query!(
            "DELETE FROM pattern_validation_cache WHERE code_hash = ?",
            code_hash
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Evict old cache entries based on last_used timestamp
    pub async fn evict_old_entries(&self, max_age_days: i64) -> Result<i64> {
        let cutoff = Utc::now().timestamp() - (max_age_days * 24 * 60 * 60);

        let result = sqlx::query!(
            "DELETE FROM pattern_validation_cache WHERE last_used < ?",
            cutoff
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Evict least frequently used entries to maintain cache size
    pub async fn evict_lfu(&self, max_entries: i64) -> Result<i64> {
        let count: i64 = sqlx::query_scalar!("SELECT COUNT(*) as count FROM pattern_validation_cache")
            .fetch_one(self.db.as_ref())
            .await? as i64;

        if count <= max_entries {
            return Ok(0);
        }

        let to_delete = count - max_entries;

        let result = sqlx::query!(
            r#"
            DELETE FROM pattern_validation_cache
            WHERE id IN (
                SELECT id FROM pattern_validation_cache
                ORDER BY hit_count ASC, last_used ASC
                LIMIT ?
            )
            "#,
            to_delete
        )
        .execute(self.db.as_ref())
        .await?;

        Ok(result.rows_affected() as i64)
    }

    /// Get cache statistics
    pub async fn get_stats(&self) -> Result<CacheStats> {
        let total_entries: i64 =
            sqlx::query_scalar!("SELECT COUNT(*) as count FROM pattern_validation_cache")
                .fetch_one(self.db.as_ref())
                .await? as i64;

        let total_hits: i64 =
            sqlx::query_scalar!("SELECT COALESCE(SUM(hit_count), 0) as total FROM pattern_validation_cache")
                .fetch_one(self.db.as_ref())
                .await? as i64;

        let avg_confidence: f64 =
            sqlx::query_scalar!("SELECT COALESCE(AVG(confidence), 0.0) as avg FROM pattern_validation_cache")
                .fetch_one(self.db.as_ref())
                .await?;

        let now = Utc::now().timestamp();
        let oldest_created: Option<i64> =
            sqlx::query_scalar!("SELECT MIN(created_at) FROM pattern_validation_cache")
                .fetch_one(self.db.as_ref())
                .await?;

        let oldest_entry_age_days = oldest_created
            .map(|created| (now - created) / (24 * 60 * 60))
            .unwrap_or(0);

        Ok(CacheStats {
            total_entries,
            total_hits,
            avg_confidence,
            oldest_entry_age_days,
        })
    }
}

/// Unified cache manager for code intelligence
pub struct CodeIntelligenceCache {
    pub semantic: SemanticCacheService,
    pub pattern: PatternCacheService,
}

impl CodeIntelligenceCache {
    pub fn new(db: Arc<SqlitePool>) -> Self {
        Self {
            semantic: SemanticCacheService::new(db.clone()),
            pattern: PatternCacheService::new(db),
        }
    }

    /// Run maintenance on all caches
    pub async fn run_maintenance(&self, max_age_days: i64, max_entries: i64) -> Result<(i64, i64)> {
        // Evict old entries first
        let semantic_evicted = self.semantic.evict_old_entries(max_age_days).await?
            + self.semantic.evict_lfu(max_entries).await?;

        let pattern_evicted = self.pattern.evict_old_entries(max_age_days).await?
            + self.pattern.evict_lfu(max_entries).await?;

        Ok((semantic_evicted, pattern_evicted))
    }

    /// Get combined cache statistics
    pub async fn get_combined_stats(&self) -> Result<(CacheStats, CacheStats)> {
        let semantic_stats = self.semantic.get_stats().await?;
        let pattern_stats = self.pattern.get_stats().await?;
        Ok((semantic_stats, pattern_stats))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_hash() {
        let hash1 = SemanticCacheService::compute_hash("fn foo() {}");
        let hash2 = SemanticCacheService::compute_hash("fn foo() {}");
        let hash3 = SemanticCacheService::compute_hash("fn bar() {}");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA-256 produces 64 hex characters
    }

    #[test]
    fn test_analysis_result_serialization() {
        let result = SemanticAnalysisResult {
            purpose: "Test function".to_string(),
            description: Some("A test".to_string()),
            concepts: vec!["testing".to_string()],
            domain_labels: vec!["test".to_string()],
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: SemanticAnalysisResult = serde_json::from_str(&json).unwrap();

        assert_eq!(result.purpose, deserialized.purpose);
        assert_eq!(result.concepts, deserialized.concepts);
    }

    #[test]
    fn test_validation_result_serialization() {
        let result = PatternValidationResult {
            is_pattern: true,
            pattern_name: Some("Factory".to_string()),
            involved_symbols: vec![1, 2, 3],
            description: Some("Creates objects".to_string()),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: PatternValidationResult = serde_json::from_str(&json).unwrap();

        assert_eq!(result.is_pattern, deserialized.is_pattern);
        assert_eq!(result.pattern_name, deserialized.pattern_name);
    }
}
