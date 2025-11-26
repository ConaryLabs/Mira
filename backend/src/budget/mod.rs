// backend/src/budget/mod.rs

//! Budget tracking for LLM API costs
//!
//! Tracks daily and monthly spending with configurable limits.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use sqlx::{Row, SqlitePool};
use tracing::{debug, warn};

/// Budget tracking for LLM API costs
pub struct BudgetTracker {
    db: SqlitePool,
    daily_limit_usd: f64,
    monthly_limit_usd: f64,
}

/// Budget usage for a time period
#[derive(Debug, Clone)]
pub struct BudgetUsage {
    pub total_cost_usd: f64,
    pub total_requests: i64,
    pub cached_requests: i64,
    pub cache_hit_rate: f64,
    pub tokens_input: i64,
    pub tokens_output: i64,
}

impl BudgetTracker {
    /// Create a new budget tracker
    pub fn new(db: SqlitePool, daily_limit_usd: f64, monthly_limit_usd: f64) -> Self {
        Self {
            db,
            daily_limit_usd,
            monthly_limit_usd,
        }
    }

    /// Record a budget entry for an LLM request
    pub async fn record_request(
        &self,
        user_id: &str,
        operation_id: Option<&str>,
        provider: &str,
        model: &str,
        reasoning_effort: Option<&str>,
        tokens_input: i64,
        tokens_output: i64,
        cost_usd: f64,
        from_cache: bool,
    ) -> Result<()> {
        let timestamp = Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO budget_tracking (
                user_id, operation_id, provider, model, reasoning_effort,
                tokens_input, tokens_output, cost_usd, from_cache, timestamp
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(user_id)
        .bind(operation_id)
        .bind(provider)
        .bind(model)
        .bind(reasoning_effort)
        .bind(tokens_input)
        .bind(tokens_output)
        .bind(cost_usd)
        .bind(from_cache)
        .bind(timestamp)
        .execute(&self.db)
        .await?;

        debug!(
            "Recorded budget entry: user={}, provider={}, model={}, cost=${:.4}, from_cache={}",
            user_id, provider, model, cost_usd, from_cache
        );

        Ok(())
    }

    /// Check if a user can make a request without exceeding daily limit
    pub async fn check_daily_limit(&self, user_id: &str) -> Result<bool> {
        let today_start = self.get_day_start();
        let usage = self.get_usage_since(user_id, today_start).await?;

        if usage.total_cost_usd >= self.daily_limit_usd {
            warn!(
                "User {} exceeded daily budget limit: ${:.2} >= ${:.2}",
                user_id, usage.total_cost_usd, self.daily_limit_usd
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// Check if a user can make a request without exceeding monthly limit
    pub async fn check_monthly_limit(&self, user_id: &str) -> Result<bool> {
        let month_start = self.get_month_start();
        let usage = self.get_usage_since(user_id, month_start).await?;

        if usage.total_cost_usd >= self.monthly_limit_usd {
            warn!(
                "User {} exceeded monthly budget limit: ${:.2} >= ${:.2}",
                user_id, usage.total_cost_usd, self.monthly_limit_usd
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// Check both daily and monthly limits
    pub async fn check_limits(&self, user_id: &str, estimated_cost: f64) -> Result<()> {
        if !self.check_daily_limit(user_id).await? {
            return Err(anyhow!(
                "Daily budget limit exceeded (${:.2}). Current cost would be ${:.2} over limit.",
                self.daily_limit_usd,
                estimated_cost
            ));
        }

        if !self.check_monthly_limit(user_id).await? {
            return Err(anyhow!(
                "Monthly budget limit exceeded (${:.2}). Current cost would be ${:.2} over limit.",
                self.monthly_limit_usd,
                estimated_cost
            ));
        }

        Ok(())
    }

    /// Get total spending for a user since a timestamp
    pub async fn get_usage_since(&self, user_id: &str, since: i64) -> Result<BudgetUsage> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(cost_usd), 0.0) as total_cost,
                COUNT(*) as total_requests,
                SUM(CASE WHEN from_cache THEN 1 ELSE 0 END) as cached_requests,
                COALESCE(SUM(tokens_input), 0) as tokens_input,
                COALESCE(SUM(tokens_output), 0) as tokens_output
            FROM budget_tracking
            WHERE user_id = ? AND timestamp >= ?
            "#,
        )
        .bind(user_id)
        .bind(since)
        .fetch_one(&self.db)
        .await?;

        let total_cost: f64 = row.get("total_cost");
        let total_requests: i64 = row.get("total_requests");
        let cached_requests: i64 = row.get("cached_requests");
        let tokens_input: i64 = row.get("tokens_input");
        let tokens_output: i64 = row.get("tokens_output");

        let cache_hit_rate = if total_requests > 0 {
            cached_requests as f64 / total_requests as f64
        } else {
            0.0
        };

        Ok(BudgetUsage {
            total_cost_usd: total_cost,
            total_requests,
            cached_requests,
            cache_hit_rate,
            tokens_input,
            tokens_output,
        })
    }

    /// Get daily usage for a user
    pub async fn get_daily_usage(&self, user_id: &str) -> Result<BudgetUsage> {
        let today_start = self.get_day_start();
        self.get_usage_since(user_id, today_start).await
    }

    /// Get monthly usage for a user
    pub async fn get_monthly_usage(&self, user_id: &str) -> Result<BudgetUsage> {
        let month_start = self.get_month_start();
        self.get_usage_since(user_id, month_start).await
    }

    /// Generate and store daily summary
    pub async fn generate_daily_summary(&self, user_id: &str, day: DateTime<Utc>) -> Result<()> {
        let day_start = day
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let day_end = day_start + 86400; // 24 hours

        let usage = self
            .get_usage_in_range(user_id, day_start, day_end)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO budget_summary (
                user_id, period_type, period_start, period_end,
                total_requests, cached_requests, total_tokens_input,
                total_tokens_output, total_cost_usd, cache_hit_rate, created_at
            )
            VALUES (?, 'daily', ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, period_type, period_start) DO UPDATE SET
                total_requests = excluded.total_requests,
                cached_requests = excluded.cached_requests,
                total_tokens_input = excluded.total_tokens_input,
                total_tokens_output = excluded.total_tokens_output,
                total_cost_usd = excluded.total_cost_usd,
                cache_hit_rate = excluded.cache_hit_rate
            "#,
        )
        .bind(user_id)
        .bind(day_start)
        .bind(day_end)
        .bind(usage.total_requests)
        .bind(usage.cached_requests)
        .bind(usage.tokens_input)
        .bind(usage.tokens_output)
        .bind(usage.total_cost_usd)
        .bind(usage.cache_hit_rate)
        .bind(Utc::now().timestamp())
        .execute(&self.db)
        .await?;

        debug!(
            "Generated daily summary for user={}, date={}, cost=${:.4}",
            user_id,
            day.format("%Y-%m-%d"),
            usage.total_cost_usd
        );

        Ok(())
    }

    /// Generate and store monthly summary
    pub async fn generate_monthly_summary(
        &self,
        user_id: &str,
        month: DateTime<Utc>,
    ) -> Result<()> {
        let month_start = month
            .date_naive()
            .with_day(1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();

        let next_month = if month.month() == 12 {
            Utc.with_ymd_and_hms(month.year() + 1, 1, 1, 0, 0, 0)
                .unwrap()
        } else {
            Utc.with_ymd_and_hms(month.year(), month.month() + 1, 1, 0, 0, 0)
                .unwrap()
        };
        let month_end = next_month.timestamp();

        let usage = self
            .get_usage_in_range(user_id, month_start, month_end)
            .await?;

        sqlx::query(
            r#"
            INSERT INTO budget_summary (
                user_id, period_type, period_start, period_end,
                total_requests, cached_requests, total_tokens_input,
                total_tokens_output, total_cost_usd, cache_hit_rate, created_at
            )
            VALUES (?, 'monthly', ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id, period_type, period_start) DO UPDATE SET
                total_requests = excluded.total_requests,
                cached_requests = excluded.cached_requests,
                total_tokens_input = excluded.total_tokens_input,
                total_tokens_output = excluded.total_tokens_output,
                total_cost_usd = excluded.total_cost_usd,
                cache_hit_rate = excluded.cache_hit_rate
            "#,
        )
        .bind(user_id)
        .bind(month_start)
        .bind(month_end)
        .bind(usage.total_requests)
        .bind(usage.cached_requests)
        .bind(usage.tokens_input)
        .bind(usage.tokens_output)
        .bind(usage.total_cost_usd)
        .bind(usage.cache_hit_rate)
        .bind(Utc::now().timestamp())
        .execute(&self.db)
        .await?;

        debug!(
            "Generated monthly summary for user={}, month={}, cost=${:.4}",
            user_id,
            month.format("%Y-%m"),
            usage.total_cost_usd
        );

        Ok(())
    }

    /// Get usage in a specific time range
    async fn get_usage_in_range(
        &self,
        user_id: &str,
        start: i64,
        end: i64,
    ) -> Result<BudgetUsage> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(cost_usd), 0.0) as total_cost,
                COUNT(*) as total_requests,
                SUM(CASE WHEN from_cache THEN 1 ELSE 0 END) as cached_requests,
                COALESCE(SUM(tokens_input), 0) as tokens_input,
                COALESCE(SUM(tokens_output), 0) as tokens_output
            FROM budget_tracking
            WHERE user_id = ? AND timestamp >= ? AND timestamp < ?
            "#,
        )
        .bind(user_id)
        .bind(start)
        .bind(end)
        .fetch_one(&self.db)
        .await?;

        let total_cost: f64 = row.get("total_cost");
        let total_requests: i64 = row.get("total_requests");
        let cached_requests: i64 = row.get("cached_requests");
        let tokens_input: i64 = row.get("tokens_input");
        let tokens_output: i64 = row.get("tokens_output");

        let cache_hit_rate = if total_requests > 0 {
            cached_requests as f64 / total_requests as f64
        } else {
            0.0
        };

        Ok(BudgetUsage {
            total_cost_usd: total_cost,
            total_requests,
            cached_requests,
            cache_hit_rate,
            tokens_input,
            tokens_output,
        })
    }

    /// Get start of current day (00:00:00 UTC)
    fn get_day_start(&self) -> i64 {
        Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    /// Get start of current month (first day, 00:00:00 UTC)
    fn get_month_start(&self) -> i64 {
        let now = Utc::now();
        now.date_naive()
            .with_day(1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    /// Get budget status for a user (for budget-aware context selection)
    pub async fn get_budget_status(
        &self,
        user_id: &str,
    ) -> Result<crate::context_oracle::BudgetStatus> {
        let daily_usage = self.get_daily_usage(user_id).await?;
        let monthly_usage = self.get_monthly_usage(user_id).await?;

        Ok(crate::context_oracle::BudgetStatus::new(
            daily_usage.total_cost_usd,
            self.daily_limit_usd,
            monthly_usage.total_cost_usd,
            self.monthly_limit_usd,
        ))
    }

    /// Get the daily limit
    pub fn daily_limit(&self) -> f64 {
        self.daily_limit_usd
    }

    /// Get the monthly limit
    pub fn monthly_limit(&self) -> f64 {
        self.monthly_limit_usd
    }
}
