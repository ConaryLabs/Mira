// src/memory/features/decay.rs
// Complete decay system - algorithm and scheduling in one module
// Handles both the decay calculations and database updates

use std::sync::Arc;
use std::time::Duration as StdDuration;
use anyhow::Result;
use chrono::{Duration, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, Transaction, Sqlite};
use tracing::{debug, info, warn};
use crate::state::AppState;

// ============================================================================
// CONFIGURATION
// ============================================================================

/// Configuration for memory decay
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DecayConfig {
    /// Minimum salience floor (memories never decay below this)
    pub floor: f32,
    /// Boost factor when a memory is recalled
    pub recall_boost: f32,
    /// Maximum salience cap
    pub ceiling: f32,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            floor: 2.0,         // 20% floor - memories never completely disappear
            recall_boost: 1.3,  // 30% boost on recall
            ceiling: 10.0,      // Maximum salience
        }
    }
}

// ============================================================================
// DECAY CALCULATIONS (Pure Functions)
// ============================================================================

/// Calculate decayed salience based on age
/// This is the core decay algorithm
pub fn calculate_decay(
    original_salience: f32,
    age: Duration,  // This is chrono::Duration
    config: &DecayConfig,
) -> f32 {
    // Superhuman stepped decay - memories fade very slowly
    let retention = if age.num_hours() < 24 {
        1.0   // First 24 hours: Perfect recall
    } else if age.num_days() < 7 {
        0.95  // First week: 95% retention
    } else if age.num_days() < 30 {
        0.90  // First month: 90% retention  
    } else if age.num_days() < 90 {
        0.80  // First 3 months: 80% retention
    } else if age.num_days() < 365 {
        0.70  // First year: 70% retention
    } else if age.num_days() < 730 {
        0.50  // First 2 years: 50% retention
    } else {
        0.30  // Ancient history: 30% retention
    };
    
    // Apply retention and respect floor
    (original_salience * retention).max(config.floor)
}

/// Reinforce a memory when it's recalled
pub fn reinforce_memory(
    current_salience: f32,
    config: &DecayConfig,
) -> f32 {
    (current_salience * config.recall_boost).min(config.ceiling)
}

// ============================================================================
// DATABASE OPERATIONS
// ============================================================================

/// Spawn the background decay task
pub fn spawn_decay_scheduler(
    app_state: Arc<AppState>,
    interval: StdDuration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut cycles = 0u64;
        loop {
            cycles += 1;
            if let Err(err) = run_decay_cycle(app_state.clone()).await {
                warn!("Decay cycle {} failed: {:#}", cycles, err);
            }
            tokio::time::sleep(interval).await;
        }
    })
}

/// Run one decay cycle - updates all memories based on age
pub async fn run_decay_cycle(app: Arc<AppState>) -> Result<()> {
    let pool: &SqlitePool = &app.sqlite_store.pool;
    let config = DecayConfig::default();
    let now = Utc::now();
    
    // Get memories that need decay, excluding summaries
    let rows = sqlx::query(
        r#"
        SELECT 
            m.id,
            m.timestamp,
            a.salience,
            a.original_salience
        FROM memory_entries m
        INNER JOIN message_analysis a ON m.id = a.message_id
        WHERE a.salience IS NOT NULL
            AND a.salience > ?
            AND (m.tags IS NULL OR m.tags NOT LIKE '%"summary"%')
        ORDER BY m.timestamp DESC
        LIMIT 1000
        "#,
    )
    .bind(config.floor)
    .fetch_all(pool)
    .await?;
    
    if rows.is_empty() {
        debug!("No memories to decay");
        return Ok(());
    }
    
    let mut tx: Transaction<'_, Sqlite> = pool.begin().await?;
    let mut updated = 0;
    let mut skipped = 0;
    
    for row in &rows {
        let id: i64 = row.get("id");
        let current_salience: f32 = row.get("salience");
        
        // Get timestamp and calculate age
        let created_dt = row.get::<NaiveDateTime, _>("timestamp");
        let created = Utc.from_utc_datetime(&created_dt);
        let age = now.signed_duration_since(created);
        
        // Use original salience if available, otherwise use current
        let original = row
            .get::<Option<f32>, _>("original_salience")
            .unwrap_or(current_salience);
        
        // Apply our decay algorithm
        let decayed = calculate_decay(original, age, &config);
        
        // Skip if change is negligible
        if (current_salience - decayed).abs() < 0.01 {
            skipped += 1;
            continue;
        }
        
        // Update salience
        sqlx::query(
            "UPDATE message_analysis SET salience = ? WHERE message_id = ?"
        )
        .bind(decayed)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        
        updated += 1;
    }
    
    tx.commit().await?;
    
    if updated > 0 {
        info!("Decay cycle complete: {} updated, {} skipped", updated, skipped);
    } else {
        debug!("Decay cycle: no updates needed");
    }
    
    Ok(())
}

/// Apply reinforcement when memories are recalled
pub async fn reinforce_memories(
    memory_ids: &[i64],
    pool: &SqlitePool,
) -> Result<()> {
    if memory_ids.is_empty() {
        return Ok(());
    }
    
    let config = DecayConfig::default();
    let now = Utc::now();
    let mut tx = pool.begin().await?;
    
    for &id in memory_ids {
        // Get current salience
        let row = sqlx::query(
            "SELECT salience FROM message_analysis WHERE message_id = ?"
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        
        if let Some(row) = row {
            let current: f32 = row.get("salience");
            let reinforced = reinforce_memory(current, &config);
            
            // Update with reinforced value and recall tracking
            sqlx::query(
                r#"
                UPDATE message_analysis 
                SET salience = ?,
                    last_recalled = ?,
                    recall_count = recall_count + 1
                WHERE message_id = ?
                "#
            )
            .bind(reinforced)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
    }
    
    tx.commit().await?;
    
    debug!("Reinforced {} recalled memories", memory_ids.len());
    Ok(())
}

// ============================================================================
// MONITORING AND STATISTICS
// ============================================================================

/// Get decay statistics for monitoring
pub async fn get_decay_stats(pool: &SqlitePool) -> Result<DecayStats> {
    let config = DecayConfig::default();
    
    let stats = sqlx::query_as::<_, DecayStats>(
        r#"
        SELECT
            COUNT(DISTINCT m.id) as total_memories,
            AVG(a.salience) as avg_salience,
            MIN(a.salience) as min_salience,
            MAX(a.salience) as max_salience,
            COUNT(CASE WHEN a.salience <= ? THEN 1 END) as at_floor_count,
            COUNT(CASE WHEN a.recall_count > 0 THEN 1 END) as recalled_count
        FROM memory_entries m
        INNER JOIN message_analysis a ON m.id = a.message_id
        WHERE a.salience IS NOT NULL
        "#
    )
    .bind(config.floor)
    .fetch_one(pool)
    .await?;
    
    Ok(stats)
}

#[derive(Debug, sqlx::FromRow)]
pub struct DecayStats {
    pub total_memories: i64,
    pub avg_salience: Option<f32>,
    pub min_salience: Option<f32>,
    pub max_salience: Option<f32>,
    #[sqlx(rename = "at_floor_count")]
    pub at_floor_count: i64,
    pub recalled_count: i64,
}

impl DecayStats {
    pub fn summary(&self) -> String {
        format!(
            "Memory stats: {} total, avg: {:.2}, range: {:.2}-{:.2}, {} at floor, {} recalled",
            self.total_memories,
            self.avg_salience.unwrap_or(0.0),
            self.min_salience.unwrap_or(0.0),
            self.max_salience.unwrap_or(0.0),
            self.at_floor_count,
            self.recalled_count
        )
    }
}
