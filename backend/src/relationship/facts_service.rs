// src/relationship/facts_service.rs

use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use tracing::{debug, info};
use uuid::Uuid;

use super::MemoryFact;

pub struct FactsService {
    pool: SqlitePool,
}

impl FactsService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
    
    /// Store or update a fact
    pub async fn upsert_fact(
        &self,
        user_id: &str,
        fact_key: &str,
        fact_value: &str,
        fact_category: &str,
        context: Option<&str>,
        confidence: f64,
    ) -> Result<String> {
        let now = Utc::now().timestamp();
        
        // Check if fact already exists
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM memory_facts WHERE user_id = ? AND fact_key = ?"
        )
        .bind(user_id)
        .bind(fact_key)
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some((fact_id,)) = existing {
            // Update existing fact
            sqlx::query(
                r#"
                UPDATE memory_facts 
                SET fact_value = ?, fact_category = ?, context = ?, 
                    confidence = ?, updated_at = ?
                WHERE id = ?
                "#
            )
            .bind(fact_value)
            .bind(fact_category)
            .bind(context)
            .bind(confidence)
            .bind(now)
            .bind(&fact_id)
            .execute(&self.pool)
            .await?;
            
            info!("Updated fact '{}' for user {}", fact_key, user_id);
            Ok(fact_id)
        } else {
            // Insert new fact
            let fact_id = Uuid::new_v4().to_string();
            
            sqlx::query(
                r#"
                INSERT INTO memory_facts (
                    id, user_id, fact_key, fact_value, fact_category,
                    context, confidence, reference_count, still_relevant,
                    created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, 0, 1, ?, ?)
                "#
            )
            .bind(&fact_id)
            .bind(user_id)
            .bind(fact_key)
            .bind(fact_value)
            .bind(fact_category)
            .bind(context)
            .bind(confidence)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
            
            info!("Created new fact '{}' for user {}", fact_key, user_id);
            Ok(fact_id)
        }
    }
    
    /// Get a specific fact by key
    pub async fn get_fact(&self, user_id: &str, fact_key: &str) -> Result<Option<MemoryFact>> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, fact_key, fact_value, fact_category,
                   context, confidence, last_referenced, reference_count,
                   still_relevant, created_at, updated_at
            FROM memory_facts
            WHERE user_id = ? AND fact_key = ? AND still_relevant = 1
            "#
        )
        .bind(user_id)
        .bind(fact_key)
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(row) = row {
            Ok(Some(self.row_to_fact(row)?))
        } else {
            Ok(None)
        }
    }
    
    /// Get all facts for a user, optionally filtered by category
    pub async fn get_user_facts(
        &self,
        user_id: &str,
        category: Option<&str>,
    ) -> Result<Vec<MemoryFact>> {
        let query = if let Some(cat) = category {
            sqlx::query(
                r#"
                SELECT id, user_id, fact_key, fact_value, fact_category,
                       context, confidence, last_referenced, reference_count,
                       still_relevant, created_at, updated_at
                FROM memory_facts
                WHERE user_id = ? AND fact_category = ? AND still_relevant = 1
                ORDER BY confidence DESC, reference_count DESC
                "#
            )
            .bind(user_id)
            .bind(cat)
        } else {
            sqlx::query(
                r#"
                SELECT id, user_id, fact_key, fact_value, fact_category,
                       context, confidence, last_referenced, reference_count,
                       still_relevant, created_at, updated_at
                FROM memory_facts
                WHERE user_id = ? AND still_relevant = 1
                ORDER BY confidence DESC, reference_count DESC
                "#
            )
            .bind(user_id)
        };
        
        let rows = query.fetch_all(&self.pool).await?;
        
        let mut facts = Vec::new();
        for row in rows {
            facts.push(self.row_to_fact(row)?);
        }
        
        Ok(facts)
    }
    
    /// Mark a fact as referenced (updates last_referenced and increments count)
    pub async fn reference_fact(&self, fact_id: &str) -> Result<()> {
        let now = Utc::now().timestamp();
        
        sqlx::query(
            r#"
            UPDATE memory_facts 
            SET last_referenced = ?, reference_count = reference_count + 1
            WHERE id = ?
            "#
        )
        .bind(now)
        .bind(fact_id)
        .execute(&self.pool)
        .await?;
        
        debug!("Referenced fact {}", fact_id);
        Ok(())
    }
    
    /// Mark a fact as no longer relevant (soft delete)
    pub async fn deprecate_fact(&self, fact_id: &str) -> Result<()> {
        sqlx::query("UPDATE memory_facts SET still_relevant = 0 WHERE id = ?")
            .bind(fact_id)
            .execute(&self.pool)
            .await?;
        
        info!("Deprecated fact {}", fact_id);
        Ok(())
    }
    
    /// Delete a fact permanently
    pub async fn delete_fact(&self, fact_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM memory_facts WHERE id = ?")
            .bind(fact_id)
            .execute(&self.pool)
            .await?;
        
        info!("Deleted fact {}", fact_id);
        Ok(())
    }
    
    /// Convert database row to MemoryFact
    fn row_to_fact(&self, row: sqlx::sqlite::SqliteRow) -> Result<MemoryFact> {
        use sqlx::Row;
        
        Ok(MemoryFact {
            id: row.try_get("id")?,
            user_id: row.try_get("user_id")?,
            fact_key: row.try_get("fact_key")?,
            fact_value: row.try_get("fact_value")?,
            fact_category: row.try_get("fact_category")?,
            confidence: row.try_get("confidence")?,
            source: row.try_get("context")?,  // Note: context maps to source field
            learned_at: row.try_get("created_at")?,
            last_confirmed: row.try_get("last_referenced")?,
            times_referenced: row.try_get("reference_count")?,
        })
    }
}
