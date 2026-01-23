// db/proxy.rs
// Database operations for proxy usage tracking

use crate::db::Database;
use crate::proxy::UsageRecord;
use anyhow::Result;
use rusqlite::params;

impl Database {
    /// Insert a proxy usage record
    pub fn insert_proxy_usage(&self, record: &UsageRecord) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO proxy_usage (
                backend_name, model, input_tokens, output_tokens,
                cache_creation_tokens, cache_read_tokens, cost_estimate,
                request_id, session_id, project_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                record.backend_name,
                record.model,
                record.input_tokens as i64,
                record.output_tokens as i64,
                record.cache_creation_tokens as i64,
                record.cache_read_tokens as i64,
                record.cost_estimate,
                record.request_id,
                record.session_id,
                record.project_id,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get usage summary by backend for a date range
    pub fn get_usage_summary(
        &self,
        backend: Option<&str>,
        since: &str,
    ) -> Result<Vec<UsageSummaryRow>> {
        let conn = self.conn();

        let sql = if backend.is_some() {
            "SELECT backend_name, model,
                    SUM(input_tokens) as total_input,
                    SUM(output_tokens) as total_output,
                    SUM(cache_creation_tokens) as total_cache_create,
                    SUM(cache_read_tokens) as total_cache_read,
                    SUM(cost_estimate) as total_cost,
                    COUNT(*) as request_count
             FROM proxy_usage
             WHERE backend_name = ? AND created_at >= ?
             GROUP BY backend_name, model
             ORDER BY total_cost DESC"
        } else {
            "SELECT backend_name, model,
                    SUM(input_tokens) as total_input,
                    SUM(output_tokens) as total_output,
                    SUM(cache_creation_tokens) as total_cache_create,
                    SUM(cache_read_tokens) as total_cache_read,
                    SUM(cost_estimate) as total_cost,
                    COUNT(*) as request_count
             FROM proxy_usage
             WHERE created_at >= ?
             GROUP BY backend_name, model
             ORDER BY total_cost DESC"
        };

        let mut stmt = conn.prepare(sql)?;

        let rows: Vec<UsageSummaryRow> = if let Some(b) = backend {
            stmt.query_map(params![b, since], |row| {
                Ok(UsageSummaryRow {
                    backend_name: row.get(0)?,
                    model: row.get(1)?,
                    total_input_tokens: row.get(2)?,
                    total_output_tokens: row.get(3)?,
                    total_cache_creation_tokens: row.get(4)?,
                    total_cache_read_tokens: row.get(5)?,
                    total_cost: row.get::<_, f64>(6).unwrap_or(0.0),
                    request_count: row.get(7)?,
                })
            })?
            .filter_map(Result::ok)
            .collect()
        } else {
            stmt.query_map(params![since], |row| {
                Ok(UsageSummaryRow {
                    backend_name: row.get(0)?,
                    model: row.get(1)?,
                    total_input_tokens: row.get(2)?,
                    total_output_tokens: row.get(3)?,
                    total_cache_creation_tokens: row.get(4)?,
                    total_cache_read_tokens: row.get(5)?,
                    total_cost: row.get::<_, f64>(6).unwrap_or(0.0),
                    request_count: row.get(7)?,
                })
            })?
            .filter_map(Result::ok)
            .collect()
        };

        Ok(rows)
    }

    /// Get total usage stats
    pub fn get_usage_totals(&self, since: &str) -> Result<UsageTotals> {
        let conn = self.conn();
        let row = conn.query_row(
            "SELECT
                COALESCE(SUM(input_tokens), 0) as total_input,
                COALESCE(SUM(output_tokens), 0) as total_output,
                COALESCE(SUM(cost_estimate), 0) as total_cost,
                COUNT(*) as request_count
             FROM proxy_usage
             WHERE created_at >= ?",
            params![since],
            |row| {
                Ok(UsageTotals {
                    total_input_tokens: row.get(0)?,
                    total_output_tokens: row.get(1)?,
                    total_cost: row.get(2)?,
                    request_count: row.get(3)?,
                })
            },
        )?;
        Ok(row)
    }
}

/// A row from the usage summary query
#[derive(Debug)]
pub struct UsageSummaryRow {
    pub backend_name: String,
    pub model: Option<String>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_creation_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub total_cost: f64,
    pub request_count: i64,
}

/// Total usage statistics
#[derive(Debug)]
pub struct UsageTotals {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cost: f64,
    pub request_count: i64,
}
