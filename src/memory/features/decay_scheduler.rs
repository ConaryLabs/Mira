// src/memory/features/decay_scheduler.rs

//! Salience decay scheduler for the new memory system.
//! 
//! Runs on an interval and gently decays salience for memories,
//! with stronger decay for long-unaccessed entries.
//! 
//! Works with the new memory_entries and message_analysis tables.

use std::{sync::Arc, time::Duration};
use anyhow::Result;
use chrono::{NaiveDateTime, TimeZone, Utc};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tracing::{debug, info, warn};
use crate::state::AppState;

/// Spawn the background decay task.
///
/// `interval` is the time between decay passes (e.g., 1h).
pub fn spawn_decay_scheduler(
    app_state: Arc<AppState>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(err) = run_decay_cycle(app_state.clone()).await {
                warn!("Decay cycle failed: {err:#}");
            }
            tokio::time::sleep(interval).await;
        }
    })
}

/// One decay pass. Safe/idempotent.
pub async fn run_decay_cycle(app: Arc<AppState>) -> Result<()> {
    let pool: &SqlitePool = &app.sqlite_store.pool;
    
    // Query the new schema - join memory_entries with message_analysis
    // Skip entries that are tagged as summaries (they should persist)
    let rows = sqlx::query(
        r#"
        SELECT 
            m.id,
            m.timestamp,
            m.tags,
            a.salience,
            a.last_recalled,
            a.recall_count
        FROM memory_entries m
        INNER JOIN message_analysis a ON m.id = a.message_id
        WHERE a.salience IS NOT NULL
            AND a.salience > 0.01
            AND (m.tags IS NULL OR m.tags NOT LIKE '%"summary"%')
        ORDER BY a.last_recalled NULLS FIRST, m.timestamp ASC
        LIMIT 500
        "#,
    )
    .fetch_all(pool)
    .await?;
    
    if rows.is_empty() {
        debug!("No memories to decay");
        return Ok(());
    }
    
    let now = Utc::now();
    
    // Decay configuration
    let recent_threshold_days: i64 = 7;     // Recent memory boundary
    let gentle_decay: f32 = 0.98;           // For recent memories
    let moderate_decay: f32 = 0.95;         // For older memories
    let stronger_decay: f32 = 0.90;         // For very old, unaccessed memories
    let floor: f32 = 0.01;                  // Minimum salience
    
    let mut tx: Transaction<'_, Sqlite> = pool.begin().await?;
    let mut updated_count = 0;
    
    for row in &rows {
        let id: i64 = row.get("id");
        let current_salience: f32 = row.get("salience");
        let recall_count: Option<i32> = row.get("recall_count");
        
        // Determine last access time
        let last_recalled = row
            .get::<Option<NaiveDateTime>, _>("last_recalled")
            .map(|naive| Utc.from_utc_datetime(&naive));
        
        let created_dt = row.get::<NaiveDateTime, _>("timestamp");
        let created = Utc.from_utc_datetime(&created_dt);
        
        let last_access = last_recalled.unwrap_or(created);
        let age_days = (now - last_access).num_days();
        
        // Calculate decay factor based on age and access patterns
        let decay_factor = if age_days <= recent_threshold_days {
            gentle_decay  // Recent memories decay slowly
        } else if age_days <= 30 {
            moderate_decay  // Month-old memories decay moderately
        } else if recall_count.unwrap_or(0) > 5 {
            moderate_decay  // Frequently recalled memories resist decay
        } else {
            stronger_decay  // Old, rarely accessed memories decay faster
        };
        
        // Apply decay
        let new_salience = (current_salience * decay_factor).max(floor);
        
        // Skip if change is negligible
        if (current_salience - new_salience).abs() < 0.001 {
            continue;
        }
        
        // Update the analysis table
        sqlx::query(
            r#"
            UPDATE message_analysis
            SET salience = ?
            WHERE message_id = ?
            "#,
        )
        .bind(new_salience)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        
        updated_count += 1;
    }
    
    tx.commit().await?;
    
    if updated_count > 0 {
        info!("🧠 Decay cycle updated salience for {} memories", updated_count);
    }
    
    Ok(())
}

/// Manual trigger for decay cycle (for testing/maintenance)
pub async fn trigger_decay_cycle(app_state: Arc<AppState>) -> Result<()> {
    info!("Manually triggering decay cycle");
    run_decay_cycle(app_state).await
}

/// Get decay statistics for monitoring
pub async fn get_decay_stats(pool: &SqlitePool) -> Result<DecayStats> {
    let stats = sqlx::query_as::<_, DecayStats>(
        r#"
        SELECT
            COUNT(DISTINCT m.id) as total_memories,
            AVG(a.salience) as avg_salience,
            MIN(a.salience) as min_salience,
            MAX(a.salience) as max_salience,
            COUNT(CASE WHEN a.salience <= 0.1 THEN 1 END) as near_floor_count,
            COUNT(CASE WHEN a.last_recalled IS NOT NULL THEN 1 END) as recalled_count
        FROM memory_entries m
        INNER JOIN message_analysis a ON m.id = a.message_id
        WHERE a.salience IS NOT NULL
        "#
    )
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
    pub near_floor_count: i64,
    pub recalled_count: i64,
}
