// crates/mira-server/src/background/change_patterns.rs
// Mines diff_analyses + diff_outcomes for recurring change patterns.
//
// Three mining strategies:
// 1. Module hotspots — directories with disproportionately bad outcomes
// 2. File co-change gaps — files that cause issues when changed without their usual companions
// 3. Change size risk — correlate diff size with outcome rates

use crate::proactive::PatternType;
use crate::proactive::patterns::{BehaviorPattern, OutcomeStats, PatternData};
use anyhow::Result;
use rusqlite::Connection;

/// Minimum number of observations before creating a pattern
const MIN_OBSERVATIONS: i64 = 3;

/// Minimum bad outcome rate to flag as a pattern
const MIN_BAD_RATE: f64 = 0.3;

/// Maximum number of patterns to produce per strategy
const MAX_PATTERNS_PER_STRATEGY: usize = 20;

/// Run all change pattern mining strategies for a project
pub fn mine_change_patterns(conn: &Connection, project_id: i64) -> Result<usize> {
    let mut total = 0;

    // Check if we have enough outcome data to mine
    let outcome_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM diff_outcomes WHERE project_id = ?",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if outcome_count < MIN_OBSERVATIONS {
        return Ok(0);
    }

    total += mine_module_hotspots(conn, project_id)?;
    total += mine_co_change_gaps(conn, project_id)?;
    total += mine_size_risk(conn, project_id)?;

    Ok(total)
}

/// Strategy 1: Module hotspots — directories with high bad-outcome rates.
///
/// Groups diff outcomes by the top-level directory of changed files,
/// then flags directories where the bad-outcome rate exceeds the threshold.
fn mine_module_hotspots(conn: &Connection, project_id: i64) -> Result<usize> {
    // For each diff with outcomes, extract the top-level directory from files_json
    // and aggregate outcome types per directory.
    //
    // We use a CTE to unnest files from files_json (stored as JSON array),
    // extract the first path component as the "module", then join with outcomes.
    let sql = r#"
        WITH diff_files AS (
            SELECT
                da.id as diff_id,
                da.files_json,
                da.to_commit
            FROM diff_analyses da
            WHERE da.project_id = ?
              AND da.files_json IS NOT NULL
              AND length(da.to_commit) = 40
        ),
        -- Extract first path segment as module for each diff
        diff_modules AS (
            SELECT DISTINCT
                df.diff_id,
                df.to_commit,
                CASE
                    WHEN instr(jf.value, '/') > 0
                    THEN substr(jf.value, 1, instr(jf.value, '/') - 1)
                    ELSE jf.value
                END as module
            FROM diff_files df, json_each(df.files_json) jf
        ),
        -- Join with outcomes and aggregate per module
        module_outcomes AS (
            SELECT
                dm.module,
                COUNT(DISTINCT dm.diff_id) as total_diffs,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'clean' THEN dm.diff_id END) as clean,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'revert' THEN dm.diff_id END) as reverted,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'follow_up_fix' THEN dm.diff_id END) as fixed,
                GROUP_CONCAT(DISTINCT dm.to_commit) as sample_commits
            FROM diff_modules dm
            JOIN diff_outcomes do2 ON do2.diff_analysis_id = dm.diff_id
            WHERE do2.project_id = ?
            GROUP BY dm.module
            HAVING total_diffs >= ?
        )
        SELECT module, total_diffs, clean, reverted, fixed, sample_commits
        FROM module_outcomes
        ORDER BY (CAST(reverted + fixed AS REAL) / total_diffs) DESC
        LIMIT ?
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![
            project_id,
            project_id,
            MIN_OBSERVATIONS,
            MAX_PATTERNS_PER_STRATEGY as i64
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )?;

    let mut stored = 0;
    for row in rows.filter_map(crate::db::log_and_discard) {
        let (module, total, clean, reverted, fixed, sample_commits_str) = row;

        let bad_rate = (reverted + fixed) as f64 / total as f64;
        if bad_rate < MIN_BAD_RATE {
            continue;
        }

        let sample_commits: Vec<String> = sample_commits_str
            .split(',')
            .take(5)
            .map(|s| s.to_string())
            .collect();

        let confidence = compute_confidence(total, bad_rate);
        let pattern_key = format!("module_hotspot:{}", module);

        let pattern = BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ChangePattern,
            pattern_key,
            pattern_data: PatternData::ChangePattern {
                files: vec![],
                module: Some(module),
                pattern_subtype: "module_hotspot".to_string(),
                outcome_stats: OutcomeStats {
                    total,
                    clean,
                    reverted,
                    follow_up_fix: fixed,
                },
                sample_commits,
            },
            confidence,
            occurrence_count: total,
        };

        crate::proactive::patterns::upsert_pattern(conn, &pattern)?;
        stored += 1;
    }

    Ok(stored)
}

/// Strategy 2: File co-change gaps — find file pairs that usually change together,
/// and flag when one changes without the other and outcomes are worse.
///
/// Approach: find file pairs that appear together in >=3 diffs. Then check if diffs
/// containing file A but NOT file B have worse outcomes than diffs containing both.
fn mine_co_change_gaps(conn: &Connection, project_id: i64) -> Result<usize> {
    // Find file pairs that frequently appear together in diffs
    let pairs_sql = r#"
        WITH diff_file_list AS (
            SELECT
                da.id as diff_id,
                jf.value as file_path
            FROM diff_analyses da, json_each(da.files_json) jf
            WHERE da.project_id = ?
              AND da.files_json IS NOT NULL
              AND length(da.to_commit) = 40
        ),
        file_pairs AS (
            SELECT
                a.file_path as file_a,
                b.file_path as file_b,
                COUNT(DISTINCT a.diff_id) as together_count
            FROM diff_file_list a
            JOIN diff_file_list b ON a.diff_id = b.diff_id AND a.file_path < b.file_path
            GROUP BY a.file_path, b.file_path
            HAVING together_count >= ?
        )
        SELECT file_a, file_b, together_count
        FROM file_pairs
        ORDER BY together_count DESC
        LIMIT 50
    "#;

    let mut pairs_stmt = conn.prepare(pairs_sql)?;
    let pairs: Vec<(String, String, i64)> = pairs_stmt
        .query_map(rusqlite::params![project_id, MIN_OBSERVATIONS], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .flatten()
        .collect();

    let mut stored = 0;

    // For each pair, check outcome rates when A changes without B
    let gap_sql = r#"
        WITH diff_with_file AS (
            SELECT DISTINCT da.id as diff_id
            FROM diff_analyses da, json_each(da.files_json) jf
            WHERE da.project_id = ?
              AND da.files_json IS NOT NULL
              AND length(da.to_commit) = 40
              AND jf.value = ?
        ),
        diff_without_file AS (
            SELECT dwf.diff_id
            FROM diff_with_file dwf
            WHERE NOT EXISTS (
                SELECT 1 FROM diff_analyses da2, json_each(da2.files_json) jf2
                WHERE da2.id = dwf.diff_id AND jf2.value = ?
            )
        ),
        gap_outcomes AS (
            SELECT
                COUNT(DISTINCT dwo.diff_id) as total,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'clean' THEN dwo.diff_id END) as clean,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'revert' THEN dwo.diff_id END) as reverted,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'follow_up_fix' THEN dwo.diff_id END) as fixed
            FROM diff_without_file dwo
            JOIN diff_outcomes do2 ON do2.diff_analysis_id = dwo.diff_id
            WHERE do2.project_id = ?
        )
        SELECT total, clean, reverted, fixed FROM gap_outcomes
    "#;

    let mut gap_stmt = conn.prepare(gap_sql)?;

    for (file_a, file_b, _together_count) in &pairs {
        // Check: A without B
        if let Ok(Some(stats)) = query_gap_stats(&mut gap_stmt, project_id, file_a, file_b)
            && stats.total >= MIN_OBSERVATIONS
        {
            let bad_rate = (stats.reverted + stats.follow_up_fix) as f64 / stats.total as f64;
            if bad_rate >= MIN_BAD_RATE {
                let confidence = compute_confidence(stats.total, bad_rate);
                let occurrence_count = stats.total;
                let pattern_key = format!("co_change_gap:{}|{}", file_a, file_b);

                let pattern = BehaviorPattern {
                    id: None,
                    project_id,
                    pattern_type: PatternType::ChangePattern,
                    pattern_key,
                    pattern_data: PatternData::ChangePattern {
                        files: vec![file_a.clone(), file_b.clone()],
                        module: None,
                        pattern_subtype: "co_change_gap".to_string(),
                        outcome_stats: stats,
                        sample_commits: vec![],
                    },
                    confidence,
                    occurrence_count,
                };

                crate::proactive::patterns::upsert_pattern(conn, &pattern)?;
                stored += 1;

                if stored >= MAX_PATTERNS_PER_STRATEGY {
                    break;
                }
            }
        }

        // Check: B without A
        if stored >= MAX_PATTERNS_PER_STRATEGY {
            break;
        }
        if let Ok(Some(stats)) = query_gap_stats(&mut gap_stmt, project_id, file_b, file_a)
            && stats.total >= MIN_OBSERVATIONS
        {
            let bad_rate = (stats.reverted + stats.follow_up_fix) as f64 / stats.total as f64;
            if bad_rate >= MIN_BAD_RATE {
                let confidence = compute_confidence(stats.total, bad_rate);
                let occurrence_count = stats.total;
                let pattern_key = format!("co_change_gap:{}|{}", file_b, file_a);

                let pattern = BehaviorPattern {
                    id: None,
                    project_id,
                    pattern_type: PatternType::ChangePattern,
                    pattern_key,
                    pattern_data: PatternData::ChangePattern {
                        files: vec![file_b.clone(), file_a.clone()],
                        module: None,
                        pattern_subtype: "co_change_gap".to_string(),
                        outcome_stats: stats,
                        sample_commits: vec![],
                    },
                    confidence,
                    occurrence_count,
                };

                crate::proactive::patterns::upsert_pattern(conn, &pattern)?;
                stored += 1;

                if stored >= MAX_PATTERNS_PER_STRATEGY {
                    break;
                }
            }
        }
    }

    Ok(stored)
}

/// Helper to query gap outcome stats using a prepared statement.
/// Parameters: project_id, file_a (has this file), file_b (missing this file), project_id (for outcome filter).
fn query_gap_stats(
    stmt: &mut rusqlite::Statement,
    project_id: i64,
    file_a: &str,
    file_b: &str,
) -> Result<Option<OutcomeStats>> {
    let result = stmt.query_row(
        rusqlite::params![project_id, file_a, file_b, project_id],
        |row| {
            Ok(OutcomeStats {
                total: row.get(0)?,
                clean: row.get(1)?,
                reverted: row.get(2)?,
                follow_up_fix: row.get(3)?,
            })
        },
    );

    match result {
        Ok(stats) if stats.total > 0 => Ok(Some(stats)),
        Ok(_) => Ok(None),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Strategy 3: Change size risk — correlate diff size buckets with outcome rates.
///
/// Buckets: small (1-3 files), medium (4-10 files), large (11+ files).
fn mine_size_risk(conn: &Connection, project_id: i64) -> Result<usize> {
    let sql = r#"
        WITH sized_diffs AS (
            SELECT
                da.id as diff_id,
                da.files_changed,
                da.to_commit,
                CASE
                    WHEN da.files_changed <= 3 THEN 'small'
                    WHEN da.files_changed <= 10 THEN 'medium'
                    ELSE 'large'
                END as size_bucket
            FROM diff_analyses da
            WHERE da.project_id = ?
              AND da.files_changed IS NOT NULL
              AND length(da.to_commit) = 40
        ),
        bucket_outcomes AS (
            SELECT
                sd.size_bucket,
                COUNT(DISTINCT sd.diff_id) as total,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'clean' THEN sd.diff_id END) as clean,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'revert' THEN sd.diff_id END) as reverted,
                COUNT(DISTINCT CASE WHEN do2.outcome_type = 'follow_up_fix' THEN sd.diff_id END) as fixed,
                GROUP_CONCAT(DISTINCT sd.to_commit) as sample_commits
            FROM sized_diffs sd
            JOIN diff_outcomes do2 ON do2.diff_analysis_id = sd.diff_id
            WHERE do2.project_id = ?
            GROUP BY sd.size_bucket
            HAVING total >= ?
        )
        SELECT size_bucket, total, clean, reverted, fixed, sample_commits
        FROM bucket_outcomes
        ORDER BY (CAST(reverted + fixed AS REAL) / total) DESC
    "#;

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(
        rusqlite::params![project_id, project_id, MIN_OBSERVATIONS],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    )?;

    let mut stored = 0;
    for row in rows.filter_map(crate::db::log_and_discard) {
        let (bucket, total, clean, reverted, fixed, sample_commits_str) = row;

        let bad_rate = (reverted + fixed) as f64 / total as f64;
        if bad_rate < MIN_BAD_RATE {
            continue;
        }

        let sample_commits: Vec<String> = sample_commits_str
            .split(',')
            .take(5)
            .map(|s| s.to_string())
            .collect();

        let confidence = compute_confidence(total, bad_rate);
        let pattern_key = format!("size_risk:{}", bucket);

        let pattern = BehaviorPattern {
            id: None,
            project_id,
            pattern_type: PatternType::ChangePattern,
            pattern_key,
            pattern_data: PatternData::ChangePattern {
                files: vec![],
                module: None,
                pattern_subtype: "size_risk".to_string(),
                outcome_stats: OutcomeStats {
                    total,
                    clean,
                    reverted,
                    follow_up_fix: fixed,
                },
                sample_commits,
            },
            confidence,
            occurrence_count: total,
        };

        crate::proactive::patterns::upsert_pattern(conn, &pattern)?;
        stored += 1;
    }

    Ok(stored)
}

/// Compute confidence from observation count and bad outcome rate.
/// More observations + higher bad rate = higher confidence.
/// Caps at 0.95.
fn compute_confidence(total: i64, bad_rate: f64) -> f64 {
    // Base confidence from bad rate (0.3 -> 0.3, 1.0 -> 1.0)
    let rate_factor = bad_rate;
    // Volume factor: ramps from 0.5 at MIN_OBSERVATIONS to 1.0 at 20+ observations
    let volume_factor = (0.5 + 0.5 * ((total as f64 - MIN_OBSERVATIONS as f64) / 17.0)).min(1.0);
    (rate_factor * volume_factor).min(0.95)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_confidence_minimum_observations() {
        // At minimum observations with 50% bad rate
        let c = compute_confidence(MIN_OBSERVATIONS, 0.5);
        assert!(c > 0.0 && c < 1.0, "confidence = {}", c);
    }

    #[test]
    fn test_compute_confidence_high_volume() {
        let low = compute_confidence(3, 0.5);
        let high = compute_confidence(20, 0.5);
        assert!(high > low, "high={} should be > low={}", high, low);
    }

    #[test]
    fn test_compute_confidence_caps_at_095() {
        let c = compute_confidence(100, 1.0);
        assert!(c <= 0.95, "confidence = {} should be <= 0.95", c);
    }

    #[test]
    fn test_compute_confidence_high_bad_rate() {
        let low_rate = compute_confidence(10, 0.3);
        let high_rate = compute_confidence(10, 0.9);
        assert!(
            high_rate > low_rate,
            "high_rate={} should be > low_rate={}",
            high_rate,
            low_rate
        );
    }
}
