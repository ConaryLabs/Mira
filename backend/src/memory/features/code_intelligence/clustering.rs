// backend/src/memory/features/code_intelligence/clustering.rs
// Domain clustering: group code elements by domain/functionality

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info};

use crate::llm::provider::LlmProvider;

// ============================================================================
// Data Structures
// ============================================================================

/// A domain cluster grouping related code elements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainCluster {
    pub id: Option<i64>,
    pub project_id: String,
    pub domain_name: String,
    pub symbol_ids: Vec<i64>,
    pub file_paths: Vec<String>,
    pub cohesion_score: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Result of LLM domain analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainAnalysis {
    pub domain_name: String,
    pub description: String,
    pub confidence: f64,
    pub related_concepts: Vec<String>,
}

/// Cluster suggestion from analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSuggestion {
    pub domain_name: String,
    pub element_ids: Vec<i64>,
    pub file_paths: Vec<String>,
    pub confidence: f64,
    pub reason: String,
}

// ============================================================================
// Domain Clustering Service
// ============================================================================

/// Service for clustering code elements by domain
pub struct DomainClusteringService {
    pool: SqlitePool,
    llm_provider: Arc<dyn LlmProvider>,
}

impl DomainClusteringService {
    pub fn new(pool: SqlitePool, llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { pool, llm_provider }
    }

    // ========================================================================
    // Clustering Operations
    // ========================================================================

    /// Cluster all code elements in a project by domain
    pub async fn cluster_project(&self, project_id: &str) -> Result<Vec<DomainCluster>> {
        info!("Clustering code elements for project {}", project_id);

        // Fetch all semantic nodes for the project
        let nodes = self.fetch_semantic_nodes(project_id).await?;
        if nodes.is_empty() {
            debug!("No semantic nodes found for project {}", project_id);
            return Ok(Vec::new());
        }

        // Group by domain labels
        let mut domain_groups: HashMap<String, Vec<NodeInfo>> = HashMap::new();
        for node in &nodes {
            for domain in &node.domain_labels {
                domain_groups
                    .entry(domain.to_lowercase())
                    .or_default()
                    .push(node.clone());
            }
        }

        // Create or update clusters
        let mut clusters: Vec<DomainCluster> = Vec::new();
        let now = chrono::Utc::now().timestamp();

        for (domain_name, nodes_in_domain) in domain_groups {
            if nodes_in_domain.len() < 2 {
                // Skip domains with only one element
                continue;
            }

            let symbol_ids: Vec<i64> = nodes_in_domain.iter().map(|n| n.symbol_id).collect();
            let file_paths: HashSet<String> = nodes_in_domain
                .iter()
                .map(|n| n.file_path.clone())
                .collect();
            let file_paths: Vec<String> = file_paths.into_iter().collect();

            // Calculate cohesion score based on concept overlap
            let cohesion_score = self.calculate_cohesion(&nodes_in_domain);

            let cluster = DomainCluster {
                id: None,
                project_id: project_id.to_string(),
                domain_name: domain_name.clone(),
                symbol_ids: symbol_ids.clone(),
                file_paths: file_paths.clone(),
                cohesion_score,
                created_at: now,
                updated_at: now,
            };

            // Store the cluster
            let id = self.store_cluster(&cluster).await?;

            clusters.push(DomainCluster {
                id: Some(id),
                ..cluster
            });
        }

        info!(
            "Created {} domain clusters for project {}",
            clusters.len(),
            project_id
        );
        Ok(clusters)
    }

    /// Calculate cohesion score for a group of nodes
    fn calculate_cohesion(&self, nodes: &[NodeInfo]) -> f64 {
        if nodes.len() <= 1 {
            return 1.0;
        }

        // Calculate based on shared concepts
        let mut all_concepts: HashMap<String, usize> = HashMap::new();
        for node in nodes {
            for concept in &node.concepts {
                *all_concepts.entry(concept.to_lowercase()).or_insert(0) += 1;
            }
        }

        // Count concepts shared by multiple nodes
        let shared_concepts = all_concepts.values().filter(|&&count| count > 1).count();
        let total_concepts = all_concepts.len();

        if total_concepts == 0 {
            return 0.5; // Default for no concepts
        }

        // Cohesion = ratio of shared concepts
        let base_cohesion = shared_concepts as f64 / total_concepts as f64;

        // Bonus for having many elements
        let size_bonus = (nodes.len() as f64).ln() / 10.0;

        (base_cohesion + size_bonus).min(1.0)
    }

    /// Suggest new clusters based on LLM analysis
    pub async fn suggest_clusters(&self, project_id: &str) -> Result<Vec<ClusterSuggestion>> {
        info!("Generating cluster suggestions for project {}", project_id);

        // Fetch unclustered elements
        let unclustered = self.fetch_unclustered_elements(project_id).await?;
        if unclustered.is_empty() {
            return Ok(Vec::new());
        }

        // Build context for LLM
        let context = unclustered
            .iter()
            .map(|e| format!("{} ({}) - {}", e.name, e.element_type, e.file_path))
            .collect::<Vec<_>>()
            .join("\n");

        // Ask LLM for suggestions
        let prompt = format!(
            r#"Analyze these code elements and suggest logical domain groupings:

{}

Respond in JSON format with an array of suggestions:
[
  {{
    "domain_name": "lowercase_domain_name",
    "element_names": ["names", "of", "elements", "in", "this", "domain"],
    "confidence": 0.0 to 1.0,
    "reason": "Why these elements belong together"
  }}
]

Focus on:
- Functional groupings (auth, database, api, etc.)
- Elements that work together
- High-confidence, meaningful clusters only
"#,
            context
        );

        let messages = vec![crate::llm::provider::Message::user(prompt)];
        let response = self
            .llm_provider
            .chat(messages, "You are an expert at analyzing code and identifying domain patterns.".to_string())
            .await
            .context("LLM cluster suggestion failed")?;
        let response = response.content;

        // Parse suggestions
        let raw_suggestions = parse_cluster_suggestions(&response)?;

        // Map element names to IDs
        let name_to_element: HashMap<String, &ElementInfo> = unclustered
            .iter()
            .map(|e| (e.name.to_lowercase(), e))
            .collect();

        let mut suggestions: Vec<ClusterSuggestion> = Vec::new();
        for raw in raw_suggestions {
            let mut element_ids: Vec<i64> = Vec::new();
            let mut file_paths: HashSet<String> = HashSet::new();

            for name in &raw.element_names {
                if let Some(element) = name_to_element.get(&name.to_lowercase()) {
                    element_ids.push(element.id);
                    file_paths.insert(element.file_path.clone());
                }
            }

            if element_ids.len() >= 2 {
                suggestions.push(ClusterSuggestion {
                    domain_name: raw.domain_name,
                    element_ids,
                    file_paths: file_paths.into_iter().collect(),
                    confidence: raw.confidence,
                    reason: raw.reason,
                });
            }
        }

        Ok(suggestions)
    }

    /// Apply a cluster suggestion
    pub async fn apply_suggestion(&self, project_id: &str, suggestion: &ClusterSuggestion) -> Result<DomainCluster> {
        let now = chrono::Utc::now().timestamp();

        let cluster = DomainCluster {
            id: None,
            project_id: project_id.to_string(),
            domain_name: suggestion.domain_name.clone(),
            symbol_ids: suggestion.element_ids.clone(),
            file_paths: suggestion.file_paths.clone(),
            cohesion_score: suggestion.confidence,
            created_at: now,
            updated_at: now,
        };

        let id = self.store_cluster(&cluster).await?;

        Ok(DomainCluster {
            id: Some(id),
            ..cluster
        })
    }

    // ========================================================================
    // Query Operations
    // ========================================================================

    /// Get all clusters for a project
    pub async fn get_clusters(&self, project_id: &str) -> Result<Vec<DomainCluster>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, domain_name, symbol_ids, file_paths,
                   cohesion_score, created_at, updated_at
            FROM domain_clusters
            WHERE project_id = ?
            ORDER BY cohesion_score DESC
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| DomainCluster {
                id: r.id,
                project_id: r.project_id,
                domain_name: r.domain_name,
                symbol_ids: parse_json_array_i64(&r.symbol_ids),
                file_paths: parse_json_array(&r.file_paths),
                cohesion_score: r.cohesion_score,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Get cluster by domain name
    pub async fn get_cluster_by_domain(
        &self,
        project_id: &str,
        domain_name: &str,
    ) -> Result<Option<DomainCluster>> {
        let row = sqlx::query!(
            r#"
            SELECT id, project_id, domain_name, symbol_ids, file_paths,
                   cohesion_score, created_at, updated_at
            FROM domain_clusters
            WHERE project_id = ? AND domain_name = ?
            "#,
            project_id,
            domain_name
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| DomainCluster {
            id: r.id,
            project_id: r.project_id,
            domain_name: r.domain_name,
            symbol_ids: parse_json_array_i64(&r.symbol_ids),
            file_paths: parse_json_array(&r.file_paths),
            cohesion_score: r.cohesion_score,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    /// Find clusters containing a specific element
    pub async fn find_clusters_for_element(&self, symbol_id: i64) -> Result<Vec<DomainCluster>> {
        // SQLite doesn't have native array contains, so we use LIKE
        let pattern = format!("%{}%", symbol_id);

        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, domain_name, symbol_ids, file_paths,
                   cohesion_score, created_at, updated_at
            FROM domain_clusters
            WHERE symbol_ids LIKE ?
            "#,
            pattern
        )
        .fetch_all(&self.pool)
        .await?;

        // Filter to actually contain the ID (LIKE might match partial)
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let symbol_ids = parse_json_array_i64(&r.symbol_ids);
                if symbol_ids.contains(&symbol_id) {
                    Some(DomainCluster {
                        id: Some(r.id),
                        project_id: r.project_id,
                        domain_name: r.domain_name,
                        symbol_ids,
                        file_paths: parse_json_array(&r.file_paths),
                        cohesion_score: r.cohesion_score,
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    /// Delete clusters for a project
    pub async fn delete_clusters(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM domain_clusters WHERE project_id = ?", project_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    // ========================================================================
    // Storage Operations
    // ========================================================================

    /// Store a domain cluster
    async fn store_cluster(&self, cluster: &DomainCluster) -> Result<i64> {
        let symbol_ids_json = serde_json::to_string(&cluster.symbol_ids)?;
        let file_paths_json = serde_json::to_string(&cluster.file_paths)?;

        let result = sqlx::query!(
            r#"
            INSERT INTO domain_clusters (
                project_id, domain_name, symbol_ids, file_paths,
                cohesion_score, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(project_id, domain_name) DO UPDATE SET
                symbol_ids = excluded.symbol_ids,
                file_paths = excluded.file_paths,
                cohesion_score = excluded.cohesion_score,
                updated_at = excluded.updated_at
            RETURNING id
            "#,
            cluster.project_id,
            cluster.domain_name,
            symbol_ids_json,
            file_paths_json,
            cluster.cohesion_score,
            cluster.created_at,
            cluster.updated_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    // ========================================================================
    // Helper Queries
    // ========================================================================

    /// Fetch semantic nodes for a project
    async fn fetch_semantic_nodes(&self, project_id: &str) -> Result<Vec<NodeInfo>> {
        let rows = sqlx::query!(
            r#"
            SELECT sn.symbol_id, sn.concepts, sn.domain_labels, ce.file_path
            FROM semantic_nodes sn
            JOIN code_elements ce ON sn.symbol_id = ce.id
            WHERE ce.project_id = ?
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| NodeInfo {
                symbol_id: r.symbol_id,
                concepts: parse_json_array(&r.concepts),
                domain_labels: parse_json_array(&r.domain_labels),
                file_path: r.file_path.unwrap_or_default(),
            })
            .collect())
    }

    /// Fetch elements not in any cluster
    async fn fetch_unclustered_elements(&self, project_id: &str) -> Result<Vec<ElementInfo>> {
        // Get all clustered symbol IDs
        let clusters = self.get_clusters(project_id).await?;
        let clustered_ids: HashSet<i64> = clusters
            .iter()
            .flat_map(|c| c.symbol_ids.iter().copied())
            .collect();

        // Fetch all elements
        let rows = sqlx::query!(
            r#"
            SELECT id as "id!", name, element_type, file_path
            FROM code_elements
            WHERE project_id = ?
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        // Filter to unclustered
        Ok(rows
            .into_iter()
            .filter(|r| !clustered_ids.contains(&r.id))
            .map(|r| ElementInfo {
                id: r.id,
                name: r.name,
                element_type: r.element_type,
                file_path: r.file_path.unwrap_or_default(),
            })
            .collect())
    }
}

// ============================================================================
// Helper Structures
// ============================================================================

#[derive(Debug, Clone)]
struct NodeInfo {
    symbol_id: i64,
    concepts: Vec<String>,
    domain_labels: Vec<String>,
    file_path: String,
}

#[derive(Debug, Clone)]
struct ElementInfo {
    id: i64,
    name: String,
    element_type: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct RawClusterSuggestion {
    domain_name: String,
    element_names: Vec<String>,
    confidence: f64,
    reason: String,
}

// ============================================================================
// Helper Functions
// ============================================================================

fn parse_json_array(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_json_array_i64(json: &str) -> Vec<i64> {
    serde_json::from_str(json).unwrap_or_default()
}

fn parse_cluster_suggestions(response: &str) -> Result<Vec<RawClusterSuggestion>> {
    // Extract JSON array from response
    let json_str = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };

    serde_json::from_str(json_str).context("Failed to parse cluster suggestions")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_cluster_creation() {
        let cluster = DomainCluster {
            id: Some(1),
            project_id: "test-project".to_string(),
            domain_name: "authentication".to_string(),
            symbol_ids: vec![1, 2, 3],
            file_paths: vec!["src/auth.rs".to_string()],
            cohesion_score: 0.85,
            created_at: 1000,
            updated_at: 1000,
        };

        assert_eq!(cluster.domain_name, "authentication");
        assert_eq!(cluster.symbol_ids.len(), 3);
        assert_eq!(cluster.cohesion_score, 0.85);
    }

    #[test]
    fn test_parse_json_arrays() {
        let strings = r#"["auth", "db", "api"]"#;
        let result = parse_json_array(strings);
        assert_eq!(result, vec!["auth", "db", "api"]);

        let numbers = r#"[1, 2, 3]"#;
        let result = parse_json_array_i64(numbers);
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_cluster_suggestions() {
        let response = r#"
        Here are my suggestions:
        [
            {
                "domain_name": "authentication",
                "element_names": ["login", "logout"],
                "confidence": 0.9,
                "reason": "Auth related"
            }
        ]
        "#;

        let suggestions = parse_cluster_suggestions(response).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].domain_name, "authentication");
        assert_eq!(suggestions[0].element_names.len(), 2);
    }
}
