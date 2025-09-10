// src/memory/decay_scheduler.rs
//! Subject-aware salience decay scheduler (SQLite-backed).
//!
//! Runs on an interval and gently decays salience for non-pinned rows,
//! with a slightly stronger decay for long-unaccessed memories.
//!
//! This module intentionally performs SQL-only mutations to avoid
//! constructing `MemoryEntry` (keeps dependencies light and compile-safe).

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use chrono::{NaiveDateTime, TimeZone, Utc};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};
use tracing::{info, warn};

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
                warn!("decay cycle failed: {err:#}");
            }
            tokio::time::sleep(interval).await;
        }
    })
}

/// One decay pass. Safe/idempotent.
pub async fn run_decay_cycle(app: Arc<AppState>) -> Result<()> {
    let pool: &SqlitePool = &app.sqlite_store.pool;

    // Pull a modest batch to avoid long write locks. Tune as needed.
    // We skip pinned memories.
    let rows = sqlx::query(
        r#"
        SELECT id, salience, last_accessed, timestamp, pinned
        FROM chat_history
        WHERE COALESCE(pinned, 0) = 0
        ORDER BY last_accessed NULLS FIRST, timestamp ASC
        LIMIT 500
        "#,
    )
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(());
    }

    let now = Utc::now();

    // Mild decay factors. You can tune these env-driven if desired.
    let recent_half_life_days: i64 = 7;   // "recent" boundary
    let gentle_decay: f32 = 0.98;         // applied for recent entries
    let stronger_decay: f32 = 0.93;       // applied for old/unaccessed entries
    let floor: f32 = 0.01;                // don't go below this

    let mut tx: Transaction<'_, Sqlite> = pool.begin().await?;

    // Iterate by reference so we don't move `rows`.
    for r in &rows {
        let id: i64 = r.get("id");
        let pinned: i64 = r.get::<Option<i64>, _>("pinned").unwrap_or(0);
        if pinned != 0 {
            continue;
        }

        // Current salience (default some mid value if null)
        let current_salience: f32 = r.get::<Option<f32>, _>("salience").unwrap_or(0.5);

        // Determine recency: prefer last_accessed; fall back to timestamp.
        let last_accessed_dt = r
            .get::<Option<NaiveDateTime>, _>("last_accessed")
            .map(|naive| Utc.from_utc_datetime(&naive));
        let created_dt = r.get::<NaiveDateTime, _>("timestamp");
        let baseline_dt = last_accessed_dt.unwrap_or_else(|| Utc.from_utc_datetime(&created_dt));

        let age_days = (now - baseline_dt).num_days();

        let factor = if age_days <= recent_half_life_days {
            gentle_decay
        } else {
            stronger_decay
        };

        let new_salience = (current_salience * factor).max(floor);

        // Update salience; also refresh last_accessed so next pass uses now as baseline.
        sqlx::query(
            r#"
            UPDATE chat_history
            SET salience = ?, last_accessed = ?
            WHERE id = ?
            "#,
        )
        .bind(new_salience)
        .bind(now.naive_utc())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    info!("🫧 decay cycle updated salience for {} rows", rows.len());
    Ok(())
}
