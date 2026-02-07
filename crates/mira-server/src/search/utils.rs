// crates/mira-server/src/search/utils.rs
// Shared utilities for search operations

use mira_types::ProjectContext;
use std::collections::HashMap;

/// Convert embedding vector to bytes for sqlite-vec queries
pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Format project context header for tool responses
pub fn format_project_header(project: Option<&ProjectContext>) -> String {
    match project {
        Some(p) => format!(
            "[Project: {} @ {}]\n\n",
            p.name.as_deref().unwrap_or("Unknown"),
            p.path
        ),
        None => String::new(),
    }
}

/// Convert distance to similarity score (0.0 to 1.0)
pub fn distance_to_score(distance: f32) -> f32 {
    1.0 - distance.clamp(0.0, 1.0)
}

/// Trait for search results that can be deduplicated by file location.
pub trait Locatable: Clone {
    fn file_path(&self) -> &str;
    /// Return start line normalized to i64
    fn start_line(&self) -> i64;
    fn score(&self) -> f32;
}

/// Deduplicate search results by (file_path, start_line), keeping the highest-scoring
/// entry for each location. Returns results sorted by score descending.
pub fn deduplicate_by_location<T: Locatable>(items: Vec<T>) -> Vec<T> {
    let mut best: HashMap<(String, i64), T> = HashMap::new();

    for item in items {
        let key = (item.file_path().to_string(), item.start_line());
        best.entry(key)
            .and_modify(|existing| {
                if item.score() > existing.score() {
                    *existing = item.clone();
                }
            })
            .or_insert(item);
    }

    let mut deduped: Vec<T> = best.into_values().collect();
    deduped.sort_by(|a, b| {
        b.score()
            .partial_cmp(&a.score())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // embedding_to_bytes tests
    // ============================================================================

    #[test]
    fn test_embedding_to_bytes_empty() {
        let embedding: [f32; 0] = [];
        let bytes = embedding_to_bytes(&embedding);
        assert!(bytes.is_empty());
    }

    #[test]
    fn test_embedding_to_bytes_single_value() {
        let embedding = [1.0f32];
        let bytes = embedding_to_bytes(&embedding);
        assert_eq!(bytes.len(), 4); // f32 = 4 bytes
        // Verify it's little-endian encoding of 1.0
        assert_eq!(bytes, 1.0f32.to_le_bytes().to_vec());
    }

    #[test]
    fn test_embedding_to_bytes_multiple_values() {
        let embedding = [1.0f32, 2.0, 3.0];
        let bytes = embedding_to_bytes(&embedding);
        assert_eq!(bytes.len(), 12); // 3 * 4 bytes
    }

    #[test]
    fn test_embedding_to_bytes_roundtrip() {
        let original = [0.5f32, -1.0, 2.5, 0.0];
        let bytes = embedding_to_bytes(&original);

        // Convert back
        let recovered: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        assert_eq!(recovered, original.to_vec());
    }

    #[test]
    fn test_embedding_to_bytes_special_values() {
        let embedding = [f32::MIN, f32::MAX, f32::EPSILON, -0.0];
        let bytes = embedding_to_bytes(&embedding);
        assert_eq!(bytes.len(), 16);
    }

    // ============================================================================
    // format_project_header tests
    // ============================================================================

    #[test]
    fn test_format_project_header_none() {
        let header = format_project_header(None);
        assert!(header.is_empty());
    }

    #[test]
    fn test_format_project_header_with_name() {
        let project = ProjectContext {
            id: 1,
            name: Some("MyProject".to_string()),
            path: "/home/user/myproject".to_string(),
        };
        let header = format_project_header(Some(&project));
        assert_eq!(header, "[Project: MyProject @ /home/user/myproject]\n\n");
    }

    #[test]
    fn test_format_project_header_without_name() {
        let project = ProjectContext {
            id: 2,
            name: None,
            path: "/tmp/project".to_string(),
        };
        let header = format_project_header(Some(&project));
        assert_eq!(header, "[Project: Unknown @ /tmp/project]\n\n");
    }

    #[test]
    fn test_format_project_header_empty_name() {
        let project = ProjectContext {
            id: 3,
            name: Some("".to_string()),
            path: "/test".to_string(),
        };
        let header = format_project_header(Some(&project));
        // Empty name should still be used (not replaced with Unknown)
        assert_eq!(header, "[Project:  @ /test]\n\n");
    }

    // ============================================================================
    // distance_to_score tests
    // ============================================================================

    #[test]
    fn test_distance_to_score_zero() {
        assert_eq!(distance_to_score(0.0), 1.0);
    }

    #[test]
    fn test_distance_to_score_one() {
        assert_eq!(distance_to_score(1.0), 0.0);
    }

    #[test]
    fn test_distance_to_score_half() {
        assert_eq!(distance_to_score(0.5), 0.5);
    }

    #[test]
    fn test_distance_to_score_negative_clamped() {
        // Negative distances should clamp to 0, giving score of 1.0
        assert_eq!(distance_to_score(-0.5), 1.0);
        assert_eq!(distance_to_score(-100.0), 1.0);
    }

    #[test]
    fn test_distance_to_score_over_one_clamped() {
        // Distances over 1.0 should clamp to 1.0, giving score of 0.0
        assert_eq!(distance_to_score(1.5), 0.0);
        assert_eq!(distance_to_score(100.0), 0.0);
    }

    #[test]
    fn test_distance_to_score_precision() {
        // Test a few specific values
        assert!((distance_to_score(0.25) - 0.75).abs() < f32::EPSILON);
        assert!((distance_to_score(0.75) - 0.25).abs() < f32::EPSILON);
        assert!((distance_to_score(0.1) - 0.9).abs() < f32::EPSILON);
    }

    // ============================================================================
    // deduplicate_by_location tests
    // ============================================================================

    #[derive(Debug, Clone, PartialEq)]
    struct TestResult {
        path: String,
        line: i64,
        s: f32,
    }

    impl Locatable for TestResult {
        fn file_path(&self) -> &str {
            &self.path
        }
        fn start_line(&self) -> i64 {
            self.line
        }
        fn score(&self) -> f32 {
            self.s
        }
    }

    #[test]
    fn test_dedup_empty() {
        let results: Vec<TestResult> = vec![];
        assert!(deduplicate_by_location(results).is_empty());
    }

    #[test]
    fn test_dedup_no_duplicates() {
        let results = vec![
            TestResult {
                path: "a.rs".into(),
                line: 1,
                s: 0.9,
            },
            TestResult {
                path: "b.rs".into(),
                line: 2,
                s: 0.8,
            },
        ];
        let deduped = deduplicate_by_location(results);
        assert_eq!(deduped.len(), 2);
        assert!((deduped[0].s - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_dedup_keeps_higher_score() {
        let results = vec![
            TestResult {
                path: "a.rs".into(),
                line: 1,
                s: 0.5,
            },
            TestResult {
                path: "a.rs".into(),
                line: 1,
                s: 0.9,
            },
        ];
        let deduped = deduplicate_by_location(results);
        assert_eq!(deduped.len(), 1);
        assert!((deduped[0].s - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_dedup_sorted_by_score_desc() {
        let results = vec![
            TestResult {
                path: "a.rs".into(),
                line: 1,
                s: 0.3,
            },
            TestResult {
                path: "b.rs".into(),
                line: 1,
                s: 0.9,
            },
            TestResult {
                path: "c.rs".into(),
                line: 1,
                s: 0.6,
            },
        ];
        let deduped = deduplicate_by_location(results);
        assert_eq!(deduped.len(), 3);
        assert!(deduped[0].s >= deduped[1].s);
        assert!(deduped[1].s >= deduped[2].s);
    }
}
