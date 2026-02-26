// crates/mira-server/src/jsonl/correlation.rs
// Correlates JSONL token usage with Mira's context_injections table
// to produce a combined efficiency view.

use std::path::Path;

use rusqlite::Connection;

use super::parser::{self, SessionSummary};
use crate::db::injection::{self, InjectionStats};

/// Combined view: JSONL session usage + Mira injection stats.
#[derive(Debug, Clone)]
pub struct CorrelatedSession {
    pub session_id: String,

    // From JSONL
    pub api_turns: usize,
    pub user_prompts: u64,
    pub tool_results: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_billable_input: u64,
    pub total_tool_calls: u64,
    pub compactions: u64,
    pub first_timestamp: Option<String>,
    pub last_timestamp: Option<String>,

    // From injection tracking
    pub injections: u64,
    pub injected_chars: u64,
    pub injected_chars_avg: f64,
    pub injections_deduped: u64,
    pub injections_cached: u64,
    pub injection_avg_latency_ms: Option<f64>,

    // Derived metrics
    /// Estimated tokens Mira injected (using calibrated chars-per-token ratio).
    pub estimated_injected_tokens: u64,
    /// The chars-per-token ratio used for the estimate.
    pub chars_per_token_ratio: f64,
    /// Ratio of Mira-injected tokens to total billable input.
    /// A small ratio means Mira adds little overhead. Null if no input.
    pub injection_overhead_ratio: Option<f64>,
    /// Dedup efficiency: fraction of injections that were suppressed.
    pub dedup_rate: Option<f64>,
    /// Cache hit rate: fraction of injections served from cache.
    pub cache_rate: Option<f64>,
}

impl CorrelatedSession {
    /// Build a correlated view from a JSONL file path + DB connection.
    pub fn from_file_and_db(
        jsonl_path: &Path,
        conn: &Connection,
        session_id: &str,
    ) -> anyhow::Result<Self> {
        let summary = parser::parse_session_file(jsonl_path)?;
        let stats = injection::get_injection_stats_for_session(conn, session_id)?;
        let cal = super::calibration::calibrate_from_summary(&summary);
        Ok(Self::build(session_id, &summary, &stats, &cal))
    }

    /// Build from pre-loaded data (for live watcher integration).
    pub fn from_summary_and_stats(
        session_id: &str,
        summary: &SessionSummary,
        stats: &InjectionStats,
    ) -> Self {
        let cal = super::calibration::calibrate_from_summary(summary);
        Self::build(session_id, summary, stats, &cal)
    }

    /// Build with an explicit calibration.
    pub fn from_summary_and_stats_calibrated(
        session_id: &str,
        summary: &SessionSummary,
        stats: &InjectionStats,
        cal: &super::calibration::Calibration,
    ) -> Self {
        Self::build(session_id, summary, stats, cal)
    }

    fn build(
        session_id: &str,
        summary: &SessionSummary,
        stats: &InjectionStats,
        cal: &super::calibration::Calibration,
    ) -> Self {
        let total_billable_input = summary.total_billable_input();

        let estimated_injected_tokens = cal.chars_to_tokens(stats.total_chars);

        let injection_overhead_ratio = if total_billable_input > 0 {
            Some(estimated_injected_tokens as f64 / total_billable_input as f64)
        } else {
            None
        };

        let dedup_rate = if stats.total_injections > 0 {
            Some(stats.total_deduped as f64 / stats.total_injections as f64)
        } else {
            None
        };

        let cache_rate = if stats.total_injections > 0 {
            Some(stats.total_cached as f64 / stats.total_injections as f64)
        } else {
            None
        };

        Self {
            session_id: session_id.to_string(),

            api_turns: summary.turn_count(),
            user_prompts: summary.user_prompt_count,
            tool_results: summary.tool_result_count,
            total_input_tokens: summary.total_input_tokens(),
            total_output_tokens: summary.total_output_tokens(),
            total_cache_read_tokens: summary.total_cache_read_tokens(),
            total_cache_creation_tokens: summary.total_cache_creation_tokens(),
            total_billable_input,
            total_tool_calls: summary.total_tool_calls(),
            compactions: summary.compaction_count,
            first_timestamp: summary.first_timestamp.clone(),
            last_timestamp: summary.last_timestamp.clone(),

            injections: stats.total_injections,
            injected_chars: stats.total_chars,
            injected_chars_avg: stats.avg_chars,
            injections_deduped: stats.total_deduped,
            injections_cached: stats.total_cached,
            injection_avg_latency_ms: stats.avg_latency_ms,

            estimated_injected_tokens,
            chars_per_token_ratio: cal.chars_per_token,
            injection_overhead_ratio,
            dedup_rate,
            cache_rate,
        }
    }

    /// Format as a human-readable report.
    pub fn format_report(&self) -> String {
        let mut report = String::with_capacity(1024);

        report.push_str(&format!("Session: {}\n", self.session_id));
        if let (Some(first), Some(last)) = (&self.first_timestamp, &self.last_timestamp) {
            report.push_str(&format!("Time range: {} to {}\n", first, last));
        }

        report.push_str("\n--- Token Usage (from JSONL) ---\n");
        report.push_str(&format!("  API turns: {}\n", self.api_turns));
        report.push_str(&format!("  User prompts: {}\n", self.user_prompts));
        report.push_str(&format!("  Tool calls: {}\n", self.total_tool_calls));
        report.push_str(&format!("  Input tokens: {}\n", self.total_input_tokens));
        report.push_str(&format!("  Output tokens: {}\n", self.total_output_tokens));
        report.push_str(&format!("  Cache read: {}\n", self.total_cache_read_tokens));
        report.push_str(&format!(
            "  Cache creation: {}\n",
            self.total_cache_creation_tokens
        ));
        report.push_str(&format!(
            "  Billable input: {}\n",
            self.total_billable_input
        ));
        if self.compactions > 0 {
            report.push_str(&format!("  Compactions: {}\n", self.compactions));
        }

        report.push_str("\n--- Mira Injections ---\n");
        report.push_str(&format!("  Total injections: {}\n", self.injections));
        report.push_str(&format!(
            "  Total chars injected: {}\n",
            self.injected_chars
        ));
        report.push_str(&format!(
            "  Avg chars/injection: {:.0}\n",
            self.injected_chars_avg
        ));
        report.push_str(&format!("  Deduped: {}\n", self.injections_deduped));
        report.push_str(&format!("  Cached: {}\n", self.injections_cached));
        if let Some(lat) = self.injection_avg_latency_ms {
            report.push_str(&format!("  Avg latency: {:.1}ms\n", lat));
        }

        report.push_str("\n--- Correlation ---\n");
        report.push_str(&format!(
            "  Estimated injected tokens: {} (~chars/{:.1})\n",
            self.estimated_injected_tokens, self.chars_per_token_ratio
        ));
        if let Some(ratio) = self.injection_overhead_ratio {
            report.push_str(&format!(
                "  Injection overhead: {:.2}% of billable input\n",
                ratio * 100.0
            ));
        }
        if let Some(rate) = self.dedup_rate {
            report.push_str(&format!("  Dedup rate: {:.1}%\n", rate * 100.0));
        }
        if let Some(rate) = self.cache_rate {
            report.push_str(&format!("  Cache rate: {:.1}%\n", rate * 100.0));
        }

        report
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::injection::InjectionStats;
    use crate::jsonl::parser::SessionSummary;

    fn make_summary() -> SessionSummary {
        let mut s = SessionSummary {
            session_id: Some("test-session".to_string()),
            user_prompt_count: 5,
            tool_result_count: 10,
            compaction_count: 1,
            first_timestamp: Some("2026-01-01T00:00:00Z".to_string()),
            last_timestamp: Some("2026-01-01T01:00:00Z".to_string()),
            ..Default::default()
        };

        // Add turns with usage
        use crate::jsonl::parser::{TokenUsage, TurnSummary};
        s.turns.push(TurnSummary {
            uuid: None,
            timestamp: None,
            usage: TokenUsage {
                input_tokens: 200,
                output_tokens: 500,
                cache_read_input_tokens: 10000,
                cache_creation_input_tokens: 3000,
            },
            tool_calls: vec!["Read".to_string()],
            content_types: vec!["tool_use".to_string()],
            is_sidechain: false,
        });
        s.turns.push(TurnSummary {
            uuid: None,
            timestamp: None,
            usage: TokenUsage {
                input_tokens: 100,
                output_tokens: 300,
                cache_read_input_tokens: 12000,
                cache_creation_input_tokens: 500,
            },
            tool_calls: vec!["Bash".to_string()],
            content_types: vec!["tool_use".to_string()],
            is_sidechain: false,
        });

        s.tool_calls.insert("Read".to_string(), 1);
        s.tool_calls.insert("Bash".to_string(), 1);
        s
    }

    fn make_stats() -> InjectionStats {
        InjectionStats {
            total_injections: 20,
            total_chars: 8000, // ~2000 tokens
            total_deduped: 5,
            total_cached: 3,
            avg_chars: 400.0,
            avg_latency_ms: Some(8.5),
        }
    }

    #[test]
    fn test_correlated_session_build() {
        let summary = make_summary();
        let stats = make_stats();
        let corr = CorrelatedSession::from_summary_and_stats("test-session", &summary, &stats);

        assert_eq!(corr.session_id, "test-session");
        assert_eq!(corr.api_turns, 2);
        assert_eq!(corr.user_prompts, 5);
        assert_eq!(corr.total_input_tokens, 300);
        assert_eq!(corr.total_output_tokens, 800);
        assert_eq!(corr.total_cache_read_tokens, 22000);
        assert_eq!(corr.total_cache_creation_tokens, 3500);
        assert_eq!(corr.total_billable_input, 25800);
        assert_eq!(corr.total_tool_calls, 2);
        assert_eq!(corr.compactions, 1);

        assert_eq!(corr.injections, 20);
        assert_eq!(corr.injected_chars, 8000);
        assert_eq!(corr.estimated_injected_tokens, 2000);

        // overhead = 2000 / 25800 ~ 7.75%
        let overhead = corr.injection_overhead_ratio.expect("should have ratio");
        assert!((overhead - 0.0775).abs() < 0.01);

        // dedup rate = 5/20 = 25%
        let dedup = corr.dedup_rate.expect("should have dedup rate");
        assert!((dedup - 0.25).abs() < 0.001);

        // cache rate = 3/20 = 15%
        let cache = corr.cache_rate.expect("should have cache rate");
        assert!((cache - 0.15).abs() < 0.001);
    }

    #[test]
    fn test_correlated_session_zero_input() {
        let summary = SessionSummary::default();
        let stats = InjectionStats {
            total_injections: 0,
            total_chars: 0,
            total_deduped: 0,
            total_cached: 0,
            avg_chars: 0.0,
            avg_latency_ms: None,
        };
        let corr = CorrelatedSession::from_summary_and_stats("empty", &summary, &stats);

        assert!(corr.injection_overhead_ratio.is_none());
        assert!(corr.dedup_rate.is_none());
        assert!(corr.cache_rate.is_none());
    }

    #[test]
    fn test_format_report_contains_key_sections() {
        let summary = make_summary();
        let stats = make_stats();
        let corr = CorrelatedSession::from_summary_and_stats("test-session", &summary, &stats);
        let report = corr.format_report();

        assert!(report.contains("Session: test-session"));
        assert!(report.contains("Token Usage"));
        assert!(report.contains("Mira Injections"));
        assert!(report.contains("Correlation"));
        assert!(report.contains("Injection overhead:"));
        assert!(report.contains("Dedup rate:"));
    }

    #[test]
    fn test_from_file_and_db() {
        // Integration test: use a real JSONL file + test DB
        let jsonl_dir = dirs::home_dir().map(|h| h.join(".claude/projects/-home-peter-Mira"));

        if let Some(dir) = jsonl_dir
            && dir.exists()
        {
            let mut files: Vec<_> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
                .collect();
            files.sort_by_key(|e| {
                std::cmp::Reverse(e.metadata().ok().and_then(|m| m.modified().ok()))
            });

            if let Some(file) = files.first() {
                let conn = crate::db::test_support::setup_test_connection();
                // Extract session ID from filename
                let path = file.path();
                let session_id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");

                let corr = CorrelatedSession::from_file_and_db(&path, &conn, session_id)
                    .expect("should correlate");

                assert!(corr.api_turns > 0);
                // No injections in test DB, so injection fields should be zero
                assert_eq!(corr.injections, 0);
            }
        }
    }
}
