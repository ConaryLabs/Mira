// db/memory/ranking.rs
// Memory ranking: boost functions, constants, and scored result types

/// Semantic recall result with metadata inlined to avoid N+1 queries.
#[derive(Debug, Clone)]
pub struct RecallRow {
    pub id: i64,
    pub content: String,
    pub distance: f32,
    pub branch: Option<String>,
    pub team_id: Option<i64>,
    pub fact_type: String,
    pub category: Option<String>,
    pub status: String,
    pub updated_at: Option<String>,
    pub stale_since: Option<String>,
}

/// Lightweight memory struct for ranked export to CLAUDE.local.md
#[derive(Debug, Clone)]
pub struct RankedMemory {
    pub content: String,
    pub fact_type: String,
    pub category: Option<String>,
    pub hotness: f64,
}

// Branch-aware boosting constants (tunable)
// Lower multiplier = better score (distances are minimized)

/// Per-entity match boost factor (10% per match, applied as 0.90^n)
pub(super) const ENTITY_MATCH_BOOST: f32 = 0.90;

/// Maximum number of entity matches to apply boost for (floor = 0.90^3 = 0.729)
const MAX_ENTITY_BOOST_MATCHES: u32 = 3;

/// Boost factor for memories on the same branch (15% boost)
pub(super) const SAME_BRANCH_BOOST: f32 = 0.85;

/// Boost factor for memories on main/master branch (5% boost)
const MAIN_BRANCH_BOOST: f32 = 0.95;

/// Boost factor for memories from the same team (10% boost)
pub(super) const TEAM_SCOPE_BOOST: f32 = 0.90;

/// Apply entity-overlap boosting to a distance score.
///
/// Each matching entity reduces distance by 10%, up to 3 matches (floor 0.729).
/// Returns the original distance if match_count is 0.
pub fn apply_entity_boost(distance: f32, match_count: u32) -> f32 {
    if match_count == 0 {
        return distance;
    }
    let capped = match_count.min(MAX_ENTITY_BOOST_MATCHES);
    distance * ENTITY_MATCH_BOOST.powi(capped as i32)
}

/// Apply branch-aware boosting to a distance score
///
/// Returns a boosted (lower) distance for:
/// - Same branch: 15% reduction (multiply by 0.85)
/// - main/master: 5% reduction (multiply by 0.95)
/// - NULL branch (pre-branch-tracking data): no change
/// - Different branch: no change (keeps cross-branch knowledge accessible)
pub fn apply_branch_boost(
    distance: f32,
    memory_branch: Option<&str>,
    current_branch: Option<&str>,
) -> f32 {
    match (memory_branch, current_branch) {
        // Same branch: strongest boost
        (Some(m), Some(c)) if m == c => distance * SAME_BRANCH_BOOST,
        // main/master memories get a small boost (stable/shared knowledge)
        (Some(m), _) if m == "main" || m == "master" => distance * MAIN_BRANCH_BOOST,
        // NULL branch (pre-branch-tracking data) or different branch: no boost
        // Cross-branch knowledge remains accessible, just not prioritized
        _ => distance,
    }
}

/// Apply recency boost to a distance score.
///
/// Recent memories get a small distance reduction (up to 5%), with a 90-day half-life.
/// Applied uniformly to all memory statuses so that confirmed memories are not
/// displaced from the truncated top-N by boosted candidates (hooks filter to
/// confirmed-only *after* truncation).
/// Formula: distance * (1.0 - 0.05 * exp(-days_ago / 90.0))
pub fn apply_recency_boost(distance: f32, updated_at: Option<&str>) -> f32 {
    let Some(ts) = updated_at else {
        return distance;
    };

    // Parse ISO 8601 datetime (SQLite CURRENT_TIMESTAMP format: "YYYY-MM-DD HH:MM:SS")
    let parsed = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S"));

    let Ok(dt) = parsed else {
        return distance;
    };

    let now = chrono::Utc::now().naive_utc();
    // Clamp to >= 0 so future timestamps (clock skew) don't produce negative distances
    let days_ago = ((now - dt).num_seconds() as f64 / 86400.0).max(0.0);

    // 5% max boost, 90-day half-life exponential decay
    let boost = 1.0 - 0.05 * (-days_ago / 90.0_f64).exp();
    distance * boost as f32
}

/// Staleness penalty: memories whose linked code has changed get deprioritized.
///
/// Penalty increases over time since the memory went stale:
/// - < 1 day stale: 5% penalty (multiply distance by 1.05)
/// - 1-7 days stale: 15% penalty (1.15)
/// - 7-30 days stale: 25% penalty (1.25)
/// - > 30 days stale: 35% penalty (1.35)
///
/// Returns original distance if not stale.
pub fn apply_staleness_penalty(distance: f32, stale_since: Option<&str>) -> f32 {
    let Some(ts) = stale_since else {
        return distance;
    };

    // Parse ISO 8601 datetime (same formats as apply_recency_boost)
    let parsed = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S"));

    let Ok(dt) = parsed else {
        return distance;
    };

    let now = chrono::Utc::now().naive_utc();
    let days_stale = ((now - dt).num_seconds() as f64 / 86400.0).max(0.0);

    let penalty = if days_stale < 1.0 {
        1.05
    } else if days_stale < 7.0 {
        1.15
    } else if days_stale < 30.0 {
        1.25
    } else {
        1.35
    };

    distance * penalty as f32
}

#[cfg(test)]
mod branch_boost_tests {
    use super::*;

    #[test]
    fn test_same_branch_boost() {
        // Same branch should get 15% boost (multiply by 0.85)
        let distance = 1.0;
        let boosted = apply_branch_boost(distance, Some("feature-x"), Some("feature-x"));
        assert!((boosted - 0.85).abs() < 0.001);
    }

    #[test]
    fn test_main_branch_boost() {
        // main branch should get 5% boost (multiply by 0.95)
        let distance = 1.0;
        let boosted = apply_branch_boost(distance, Some("main"), Some("feature-x"));
        assert!((boosted - 0.95).abs() < 0.001);

        // master branch should also get 5% boost
        let boosted_master = apply_branch_boost(distance, Some("master"), Some("feature-x"));
        assert!((boosted_master - 0.95).abs() < 0.001);
    }

    #[test]
    fn test_different_branch_no_boost() {
        // Different branch should get no boost
        let distance = 1.0;
        let boosted = apply_branch_boost(distance, Some("feature-y"), Some("feature-x"));
        assert!((boosted - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_null_branch_no_boost() {
        // NULL branch (pre-branch-tracking data) should get no boost
        let distance = 1.0;

        // Memory has no branch
        let boosted1 = apply_branch_boost(distance, None, Some("feature-x"));
        assert!((boosted1 - 1.0).abs() < 0.001);

        // Current context has no branch
        let boosted2 = apply_branch_boost(distance, Some("feature-x"), None);
        assert!((boosted2 - 1.0).abs() < 0.001);

        // Both have no branch
        let boosted3 = apply_branch_boost(distance, None, None);
        assert!((boosted3 - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_boost_preserves_ordering() {
        // Boosting should improve relative ranking of same-branch memories
        let base_distance = 0.5;

        let same_branch = apply_branch_boost(base_distance, Some("feature-x"), Some("feature-x"));
        let different_branch =
            apply_branch_boost(base_distance, Some("feature-y"), Some("feature-x"));

        // Same branch should have lower (better) distance
        assert!(same_branch < different_branch);
    }

    #[test]
    fn test_main_branch_beats_different_branch() {
        // main/master branch should rank better than different branch
        let base_distance = 0.5;

        let main_branch = apply_branch_boost(base_distance, Some("main"), Some("feature-x"));
        let different_branch =
            apply_branch_boost(base_distance, Some("feature-y"), Some("feature-x"));

        // main should have lower (better) distance
        assert!(main_branch < different_branch);
    }

    #[test]
    fn test_same_branch_beats_main() {
        // Same branch should rank better than main
        let base_distance = 0.5;

        let same_branch = apply_branch_boost(base_distance, Some("feature-x"), Some("feature-x"));
        let main_branch = apply_branch_boost(base_distance, Some("main"), Some("feature-x"));

        // Same branch should have lower (better) distance
        assert!(same_branch < main_branch);
    }
}

#[cfg(test)]
mod recency_boost_tests {
    use super::*;

    #[test]
    fn test_none_updated_at_returns_unchanged() {
        let distance = 0.5;
        let boosted = apply_recency_boost(distance, None);
        assert!((boosted - distance).abs() < f32::EPSILON);
    }

    #[test]
    fn test_invalid_timestamp_returns_unchanged() {
        let distance = 0.5;
        let boosted = apply_recency_boost(distance, Some("not-a-date"));
        assert!((boosted - distance).abs() < f32::EPSILON);
    }

    #[test]
    fn test_recent_memory_gets_boost() {
        let distance = 1.0;
        let now = chrono::Utc::now()
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let boosted = apply_recency_boost(distance, Some(&now));
        // Should be close to 0.95 (5% boost for very recent)
        assert!(boosted < 0.96);
        assert!(boosted > 0.94);
    }

    #[test]
    fn test_old_memory_gets_negligible_boost() {
        let distance = 1.0;
        let old = (chrono::Utc::now() - chrono::Duration::days(365))
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let boosted = apply_recency_boost(distance, Some(&old));
        // After 365 days, boost should be negligible (close to 1.0)
        assert!(boosted > 0.99);
    }

    #[test]
    fn test_future_timestamp_clamped_no_negative_distance() {
        let distance = 1.0;
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let boosted = apply_recency_boost(distance, Some(&future));
        // Should not go negative — clamp to 0 days_ago
        assert!(boosted > 0.0);
        assert!(boosted < 1.0);
    }

    #[test]
    fn test_iso_t_separator_format_parsed() {
        let distance = 1.0;
        let now = chrono::Utc::now()
            .naive_utc()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();
        let boosted = apply_recency_boost(distance, Some(&now));
        assert!(boosted < 0.96);
    }

    #[test]
    fn test_90_day_old_memory_half_life() {
        let distance = 1.0;
        let half_life = (chrono::Utc::now() - chrono::Duration::days(90))
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let boosted = apply_recency_boost(distance, Some(&half_life));
        // At half-life, boost ≈ 0.05 * exp(-1) ≈ 0.0184, so distance ≈ 0.9816
        assert!(boosted > 0.97);
        assert!(boosted < 0.99);
    }
}

#[cfg(test)]
mod staleness_penalty_tests {
    use super::*;

    #[test]
    fn test_staleness_penalty_none() {
        // No stale_since returns original distance unchanged
        let distance = 0.5;
        let result = apply_staleness_penalty(distance, None);
        assert!((result - distance).abs() < f32::EPSILON);
    }

    #[test]
    fn test_staleness_penalty_recent() {
        // Stale < 1 day gets mild 5% penalty (distance * 1.05)
        let distance = 1.0;
        let now = chrono::Utc::now()
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let penalized = apply_staleness_penalty(distance, Some(&now));
        assert!((penalized - 1.05).abs() < 0.01);
    }

    #[test]
    fn test_staleness_penalty_week_old() {
        // Stale 3 days gets medium 15% penalty (distance * 1.15)
        let distance = 1.0;
        let three_days_ago = (chrono::Utc::now() - chrono::Duration::days(3))
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let penalized = apply_staleness_penalty(distance, Some(&three_days_ago));
        assert!((penalized - 1.15).abs() < 0.01);
    }

    #[test]
    fn test_staleness_penalty_month_old() {
        // Stale 15 days gets strong 25% penalty (distance * 1.25)
        let distance = 1.0;
        let fifteen_days_ago = (chrono::Utc::now() - chrono::Duration::days(15))
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let penalized = apply_staleness_penalty(distance, Some(&fifteen_days_ago));
        assert!((penalized - 1.25).abs() < 0.01);
    }

    #[test]
    fn test_staleness_penalty_very_old() {
        // Stale 60 days gets max 35% penalty (distance * 1.35)
        let distance = 1.0;
        let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60))
            .naive_utc()
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let penalized = apply_staleness_penalty(distance, Some(&sixty_days_ago));
        assert!((penalized - 1.35).abs() < 0.01);
    }

    #[test]
    fn test_staleness_penalty_invalid_timestamp() {
        // Invalid timestamp returns original distance
        let distance = 0.5;
        let result = apply_staleness_penalty(distance, Some("not-a-date"));
        assert!((result - distance).abs() < f32::EPSILON);
    }

    #[test]
    fn test_staleness_penalty_preserves_relative_ordering() {
        // Stale memory should have worse (higher) distance than non-stale
        let base_distance = 0.5;
        let not_stale = apply_staleness_penalty(base_distance, None);
        let stale = apply_staleness_penalty(
            base_distance,
            Some(
                &chrono::Utc::now()
                    .naive_utc()
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            ),
        );
        assert!(stale > not_stale);
    }

    #[test]
    fn test_staleness_penalty_iso_t_separator() {
        // ISO 8601 with T separator should also parse
        let distance = 1.0;
        let now = chrono::Utc::now()
            .naive_utc()
            .format("%Y-%m-%dT%H:%M:%S")
            .to_string();
        let penalized = apply_staleness_penalty(distance, Some(&now));
        assert!((penalized - 1.05).abs() < 0.01);
    }
}
