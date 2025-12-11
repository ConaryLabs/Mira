// backend/src/memory/features/code_intelligence/semantic.rs
// Semantic graph management: nodes, edges, concept indexing, and LLM-based analysis

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info};

use crate::llm::provider::LlmProvider;
use crate::memory::features::prompts::code_intelligence as prompts;

// ============================================================================
// Data Structures
// ============================================================================

/// Semantic node representing the semantic meaning of a code element
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticNode {
    pub id: Option<i64>,
    pub symbol_id: i64,
    pub purpose: String,
    pub description: Option<String>,
    pub concepts: Vec<String>,
    pub domain_labels: Vec<String>,
    pub confidence_score: f64,
    pub embedding_point_id: Option<String>,
    pub last_analyzed: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Edge connecting two semantic nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEdge {
    pub id: Option<i64>,
    pub source_node_id: i64,
    pub target_node_id: i64,
    pub relationship_type: SemanticRelationType,
    pub strength: f64,
    pub metadata: Option<String>,
    pub created_at: i64,
}

/// Types of semantic relationships between nodes
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SemanticRelationType {
    SimilarPurpose,
    SharedDomain,
    CoChange,
    Calls,
    CalledBy,
    UsesType,
    TypeUsedBy,
    SameModule,
    RelatedConcept,
}

impl SemanticRelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SimilarPurpose => "similar_purpose",
            Self::SharedDomain => "shared_domain",
            Self::CoChange => "co_change",
            Self::Calls => "calls",
            Self::CalledBy => "called_by",
            Self::UsesType => "uses_type",
            Self::TypeUsedBy => "type_used_by",
            Self::SameModule => "same_module",
            Self::RelatedConcept => "related_concept",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "similar_purpose" => Some(Self::SimilarPurpose),
            "shared_domain" => Some(Self::SharedDomain),
            "co_change" => Some(Self::CoChange),
            "calls" => Some(Self::Calls),
            "called_by" => Some(Self::CalledBy),
            "uses_type" => Some(Self::UsesType),
            "type_used_by" => Some(Self::TypeUsedBy),
            "same_module" => Some(Self::SameModule),
            "related_concept" => Some(Self::RelatedConcept),
            _ => None,
        }
    }
}

/// Entry in the concept index for fast concept-based search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConceptEntry {
    pub id: Option<i64>,
    pub concept: String,
    pub symbol_ids: Vec<i64>,
    pub confidence: f64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// LLM analysis result for a code element
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticAnalysis {
    pub purpose: String,
    pub description: String,
    pub concepts: Vec<String>,
    pub domain_labels: Vec<String>,
    pub confidence: f64,
}

// ============================================================================
// Semantic Graph Service
// ============================================================================

/// Service for managing the semantic graph
pub struct SemanticGraphService {
    pool: SqlitePool,
    llm_provider: Arc<dyn LlmProvider>,
}

impl SemanticGraphService {
    pub fn new(pool: SqlitePool, llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { pool, llm_provider }
    }

    // ========================================================================
    // Semantic Node Operations
    // ========================================================================

    /// Analyze a code element and create/update its semantic node
    pub async fn analyze_and_store_element(
        &self,
        symbol_id: i64,
        element_type: &str,
        name: &str,
        content: &str,
        file_path: &str,
    ) -> Result<SemanticNode> {
        info!(
            "Analyzing semantic meaning for {} '{}' (id: {})",
            element_type, name, symbol_id
        );

        // Check if we have a cached analysis
        if let Some(cached) = self.get_cached_analysis(symbol_id, content).await? {
            debug!("Using cached semantic analysis for symbol {}", symbol_id);
            return self.store_semantic_node(symbol_id, &cached).await;
        }

        // Perform LLM analysis
        let analysis = self
            .analyze_code_semantics(element_type, name, content, file_path)
            .await?;

        // Cache the analysis
        self.cache_analysis(symbol_id, content, &analysis).await?;

        // Store the semantic node
        let node = self.store_semantic_node(symbol_id, &analysis).await?;

        // Update concept index
        self.update_concept_index(symbol_id, &analysis.concepts, analysis.confidence)
            .await?;

        Ok(node)
    }

    /// Get a semantic node by symbol ID
    pub async fn get_node_by_symbol(&self, symbol_id: i64) -> Result<Option<SemanticNode>> {
        let row = sqlx::query!(
            r#"
            SELECT id, symbol_id, purpose, description, concepts, domain_labels,
                   confidence_score, embedding_point_id, last_analyzed, created_at, updated_at
            FROM semantic_nodes
            WHERE symbol_id = ?
            "#,
            symbol_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| SemanticNode {
            id: r.id,
            symbol_id: r.symbol_id,
            purpose: r.purpose,
            description: r.description,
            concepts: parse_json_array(&r.concepts),
            domain_labels: parse_json_array(&r.domain_labels),
            confidence_score: r.confidence_score,
            embedding_point_id: r.embedding_point_id,
            last_analyzed: r.last_analyzed,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }

    /// Get all semantic nodes for a project
    pub async fn get_nodes_for_project(&self, project_id: &str) -> Result<Vec<SemanticNode>> {
        let rows = sqlx::query!(
            r#"
            SELECT sn.id, sn.symbol_id, sn.purpose, sn.description, sn.concepts,
                   sn.domain_labels, sn.confidence_score, sn.embedding_point_id,
                   sn.last_analyzed, sn.created_at, sn.updated_at
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
            .map(|r| SemanticNode {
                id: r.id,
                symbol_id: r.symbol_id,
                purpose: r.purpose,
                description: r.description,
                concepts: parse_json_array(&r.concepts),
                domain_labels: parse_json_array(&r.domain_labels),
                confidence_score: r.confidence_score,
                embedding_point_id: r.embedding_point_id,
                last_analyzed: r.last_analyzed,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }

    /// Store a semantic node from analysis
    async fn store_semantic_node(
        &self,
        symbol_id: i64,
        analysis: &SemanticAnalysis,
    ) -> Result<SemanticNode> {
        let now = chrono::Utc::now().timestamp();
        let concepts_json = serde_json::to_string(&analysis.concepts)?;
        let domain_labels_json = serde_json::to_string(&analysis.domain_labels)?;

        let result = sqlx::query!(
            r#"
            INSERT INTO semantic_nodes (
                symbol_id, purpose, description, concepts, domain_labels,
                confidence_score, last_analyzed, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(symbol_id) DO UPDATE SET
                purpose = excluded.purpose,
                description = excluded.description,
                concepts = excluded.concepts,
                domain_labels = excluded.domain_labels,
                confidence_score = excluded.confidence_score,
                last_analyzed = excluded.last_analyzed,
                updated_at = excluded.updated_at
            RETURNING id
            "#,
            symbol_id,
            analysis.purpose,
            analysis.description,
            concepts_json,
            domain_labels_json,
            analysis.confidence,
            now,
            now,
            now
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SemanticNode {
            id: Some(result.id),
            symbol_id,
            purpose: analysis.purpose.clone(),
            description: Some(analysis.description.clone()),
            concepts: analysis.concepts.clone(),
            domain_labels: analysis.domain_labels.clone(),
            confidence_score: analysis.confidence,
            embedding_point_id: None,
            last_analyzed: now,
            created_at: now,
            updated_at: now,
        })
    }

    // ========================================================================
    // Semantic Edge Operations
    // ========================================================================

    /// Create an edge between two semantic nodes
    pub async fn create_edge(
        &self,
        source_node_id: i64,
        target_node_id: i64,
        relationship_type: SemanticRelationType,
        strength: f64,
        metadata: Option<&str>,
    ) -> Result<SemanticEdge> {
        let now = chrono::Utc::now().timestamp();
        let rel_type_str = relationship_type.as_str();

        let result = sqlx::query!(
            r#"
            INSERT INTO semantic_edges (
                source_node_id, target_node_id, relationship_type, strength, metadata, created_at
            ) VALUES (?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
            source_node_id,
            target_node_id,
            rel_type_str,
            strength,
            metadata,
            now
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(SemanticEdge {
            id: result.id,
            source_node_id,
            target_node_id,
            relationship_type,
            strength,
            metadata: metadata.map(String::from),
            created_at: now,
        })
    }

    /// Get edges from a node
    pub async fn get_edges_from(&self, source_node_id: i64) -> Result<Vec<SemanticEdge>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_node_id, target_node_id, relationship_type,
                   strength, metadata, created_at
            FROM semantic_edges
            WHERE source_node_id = ?
            "#,
            source_node_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                SemanticRelationType::from_str(&r.relationship_type).map(|rel_type| SemanticEdge {
                    id: r.id,
                    source_node_id: r.source_node_id,
                    target_node_id: r.target_node_id,
                    relationship_type: rel_type,
                    strength: r.strength,
                    metadata: r.metadata,
                    created_at: r.created_at,
                })
            })
            .collect())
    }

    /// Get edges to a node
    pub async fn get_edges_to(&self, target_node_id: i64) -> Result<Vec<SemanticEdge>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, source_node_id, target_node_id, relationship_type,
                   strength, metadata, created_at
            FROM semantic_edges
            WHERE target_node_id = ?
            "#,
            target_node_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                SemanticRelationType::from_str(&r.relationship_type).map(|rel_type| SemanticEdge {
                    id: r.id,
                    source_node_id: r.source_node_id,
                    target_node_id: r.target_node_id,
                    relationship_type: rel_type,
                    strength: r.strength,
                    metadata: r.metadata,
                    created_at: r.created_at,
                })
            })
            .collect())
    }

    /// Find related nodes by relationship type
    pub async fn find_related_nodes(
        &self,
        node_id: i64,
        relationship_type: SemanticRelationType,
        min_strength: f64,
    ) -> Result<Vec<(SemanticNode, f64)>> {
        let rel_type_str = relationship_type.as_str();

        let rows = sqlx::query!(
            r#"
            SELECT sn.id, sn.symbol_id, sn.purpose, sn.description, sn.concepts,
                   sn.domain_labels, sn.confidence_score, sn.embedding_point_id,
                   sn.last_analyzed, sn.created_at, sn.updated_at, se.strength
            FROM semantic_nodes sn
            JOIN semantic_edges se ON sn.id = se.target_node_id
            WHERE se.source_node_id = ?
              AND se.relationship_type = ?
              AND se.strength >= ?
            ORDER BY se.strength DESC
            "#,
            node_id,
            rel_type_str,
            min_strength
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    SemanticNode {
                        id: r.id,
                        symbol_id: r.symbol_id,
                        purpose: r.purpose,
                        description: r.description,
                        concepts: parse_json_array(&r.concepts),
                        domain_labels: parse_json_array(&r.domain_labels),
                        confidence_score: r.confidence_score,
                        embedding_point_id: r.embedding_point_id,
                        last_analyzed: r.last_analyzed,
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    },
                    r.strength,
                )
            })
            .collect())
    }

    // ========================================================================
    // Concept Index Operations
    // ========================================================================

    /// Search for symbols by concept
    pub async fn search_by_concept(&self, concept: &str) -> Result<Vec<i64>> {
        let pattern = format!("%{}%", concept.to_lowercase());

        let rows = sqlx::query!(
            r#"
            SELECT symbol_ids
            FROM concept_index
            WHERE LOWER(concept) LIKE ?
            ORDER BY confidence DESC
            "#,
            pattern
        )
        .fetch_all(&self.pool)
        .await?;

        let mut symbol_ids = Vec::new();
        for row in rows {
            let ids: Vec<i64> = parse_json_array_i64(&row.symbol_ids);
            symbol_ids.extend(ids);
        }

        // Deduplicate
        symbol_ids.sort();
        symbol_ids.dedup();

        Ok(symbol_ids)
    }

    /// Update concept index for a symbol
    async fn update_concept_index(
        &self,
        symbol_id: i64,
        concepts: &[String],
        confidence: f64,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        for concept in concepts {
            let concept_lower = concept.to_lowercase();

            // Check if concept exists
            let existing = sqlx::query!(
                "SELECT id, symbol_ids FROM concept_index WHERE concept = ?",
                concept_lower
            )
            .fetch_optional(&self.pool)
            .await?;

            match existing {
                Some(row) => {
                    // Update existing concept entry
                    let mut symbol_ids: Vec<i64> = parse_json_array_i64(&row.symbol_ids);
                    if !symbol_ids.contains(&symbol_id) {
                        symbol_ids.push(symbol_id);
                    }
                    let symbol_ids_json = serde_json::to_string(&symbol_ids)?;

                    sqlx::query!(
                        r#"
                        UPDATE concept_index
                        SET symbol_ids = ?, confidence = MAX(confidence, ?), updated_at = ?
                        WHERE id = ?
                        "#,
                        symbol_ids_json,
                        confidence,
                        now,
                        row.id
                    )
                    .execute(&self.pool)
                    .await?;
                }
                None => {
                    // Create new concept entry
                    let symbol_ids_json = serde_json::to_string(&vec![symbol_id])?;

                    sqlx::query!(
                        r#"
                        INSERT INTO concept_index (concept, symbol_ids, confidence, created_at, updated_at)
                        VALUES (?, ?, ?, ?, ?)
                        "#,
                        concept_lower,
                        symbol_ids_json,
                        confidence,
                        now,
                        now
                    )
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(())
    }

    /// Get all concepts for a project
    pub async fn get_project_concepts(&self, project_id: &str) -> Result<Vec<ConceptEntry>> {
        // Get all symbol IDs for the project
        let symbol_rows = sqlx::query!(
            "SELECT id FROM code_elements WHERE project_id = ?",
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        let symbol_ids: Vec<i64> = symbol_rows.iter().filter_map(|r| r.id).collect();
        if symbol_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Get concepts that contain any of these symbol IDs
        let rows = sqlx::query!(
            r#"
            SELECT id, concept, symbol_ids, confidence, created_at, updated_at
            FROM concept_index
            ORDER BY confidence DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        // Filter concepts that contain project symbols
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let ids: Vec<i64> = parse_json_array_i64(&r.symbol_ids);
                let has_project_symbol = ids.iter().any(|id| symbol_ids.contains(id));
                if has_project_symbol {
                    Some(ConceptEntry {
                        id: r.id,
                        concept: r.concept,
                        symbol_ids: ids,
                        confidence: r.confidence,
                        created_at: r.created_at,
                        updated_at: r.updated_at,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    // ========================================================================
    // Analysis Cache Operations
    // ========================================================================

    /// Get cached analysis if available and valid
    async fn get_cached_analysis(
        &self,
        symbol_id: i64,
        content: &str,
    ) -> Result<Option<SemanticAnalysis>> {
        let code_hash = compute_hash(content);

        let row = sqlx::query!(
            r#"
            SELECT analysis_result, confidence
            FROM semantic_analysis_cache
            WHERE symbol_id = ? AND code_hash = ?
            "#,
            symbol_id,
            code_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            // Update hit count
            let now = chrono::Utc::now().timestamp();
            sqlx::query!(
                "UPDATE semantic_analysis_cache SET last_used = ?, hit_count = hit_count + 1 WHERE symbol_id = ?",
                now,
                symbol_id
            )
            .execute(&self.pool)
            .await?;

            // Parse cached result
            if let Ok(analysis) = serde_json::from_str::<SemanticAnalysis>(&r.analysis_result) {
                return Ok(Some(analysis));
            }
        }

        Ok(None)
    }

    /// Cache an analysis result
    async fn cache_analysis(
        &self,
        symbol_id: i64,
        content: &str,
        analysis: &SemanticAnalysis,
    ) -> Result<()> {
        let code_hash = compute_hash(content);
        let now = chrono::Utc::now().timestamp();
        let analysis_json = serde_json::to_string(analysis)?;

        sqlx::query!(
            r#"
            INSERT INTO semantic_analysis_cache (
                symbol_id, code_hash, analysis_result, confidence, created_at, last_used, hit_count
            ) VALUES (?, ?, ?, ?, ?, ?, 0)
            ON CONFLICT(symbol_id) DO UPDATE SET
                code_hash = excluded.code_hash,
                analysis_result = excluded.analysis_result,
                confidence = excluded.confidence,
                last_used = excluded.last_used
            "#,
            symbol_id,
            code_hash,
            analysis_json,
            analysis.confidence,
            now,
            now
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ========================================================================
    // LLM Analysis
    // ========================================================================

    /// Analyze code semantics using LLM
    async fn analyze_code_semantics(
        &self,
        element_type: &str,
        name: &str,
        content: &str,
        file_path: &str,
    ) -> Result<SemanticAnalysis> {
        let prompt = format!(
            r#"Analyze this code element and extract its semantic meaning.

Element Type: {}
Name: {}
File Path: {}

Code:
```
{}
```

Respond in JSON format:
{{
  "purpose": "Brief description of what this code does (1-2 sentences)",
  "description": "Detailed explanation of functionality and behavior",
  "concepts": ["list", "of", "key", "concepts", "this", "code", "implements"],
  "domain_labels": ["domain", "areas", "like", "authentication", "database", "api"],
  "confidence": 0.95
}}

Focus on:
- What problem does this code solve?
- What are the key concepts it implements?
- What domain/area does it belong to?
"#,
            element_type, name, file_path, content
        );

        let messages = vec![crate::llm::provider::Message::user(prompt)];
        let response = self
            .llm_provider
            .chat(messages, prompts::SEMANTIC_ANALYZER.to_string())
            .await
            .context("LLM analysis failed")?;

        // Parse LLM response
        parse_semantic_analysis(&response.content)
    }

    // ========================================================================
    // Graph Building
    // ========================================================================

    /// Build edges based on shared concepts between nodes
    pub async fn build_concept_edges(&self, project_id: &str, min_overlap: usize) -> Result<usize> {
        info!(
            "Building concept edges for project {} (min overlap: {})",
            project_id, min_overlap
        );

        let nodes = self.get_nodes_for_project(project_id).await?;
        let mut edges_created = 0;

        // Build concept -> node mapping
        let mut concept_nodes: HashMap<String, Vec<i64>> = HashMap::new();
        for node in &nodes {
            if let Some(node_id) = node.id {
                for concept in &node.concepts {
                    concept_nodes
                        .entry(concept.to_lowercase())
                        .or_default()
                        .push(node_id);
                }
            }
        }

        // Find pairs with overlapping concepts
        let mut processed_pairs: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();

        for node in &nodes {
            if let Some(source_id) = node.id {
                // Count concept overlap with other nodes
                let mut overlap_counts: HashMap<i64, usize> = HashMap::new();

                for concept in &node.concepts {
                    if let Some(related_nodes) = concept_nodes.get(&concept.to_lowercase()) {
                        for &target_id in related_nodes {
                            if target_id != source_id {
                                *overlap_counts.entry(target_id).or_insert(0) += 1;
                            }
                        }
                    }
                }

                // Create edges for pairs with sufficient overlap
                for (target_id, overlap) in overlap_counts {
                    if overlap >= min_overlap {
                        let pair = if source_id < target_id {
                            (source_id, target_id)
                        } else {
                            (target_id, source_id)
                        };

                        if !processed_pairs.contains(&pair) {
                            processed_pairs.insert(pair);

                            // Calculate strength based on overlap
                            let max_concepts = node.concepts.len().max(1);
                            let strength = overlap as f64 / max_concepts as f64;

                            self.create_edge(
                                source_id,
                                target_id,
                                SemanticRelationType::RelatedConcept,
                                strength,
                                Some(&format!("{{\"overlap_count\": {}}}", overlap)),
                            )
                            .await?;

                            edges_created += 1;
                        }
                    }
                }
            }
        }

        info!(
            "Created {} concept-based edges for project {}",
            edges_created, project_id
        );
        Ok(edges_created)
    }

    /// Build edges based on shared domain labels
    pub async fn build_domain_edges(&self, project_id: &str) -> Result<usize> {
        info!("Building domain edges for project {}", project_id);

        let nodes = self.get_nodes_for_project(project_id).await?;
        let mut edges_created = 0;

        // Build domain -> node mapping
        let mut domain_nodes: HashMap<String, Vec<i64>> = HashMap::new();
        for node in &nodes {
            if let Some(node_id) = node.id {
                for domain in &node.domain_labels {
                    domain_nodes
                        .entry(domain.to_lowercase())
                        .or_default()
                        .push(node_id);
                }
            }
        }

        // Create edges for nodes in the same domain
        let mut processed_pairs: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();

        for (domain, node_ids) in &domain_nodes {
            if node_ids.len() > 1 {
                for (i, &source_id) in node_ids.iter().enumerate() {
                    for &target_id in &node_ids[i + 1..] {
                        let pair = (source_id.min(target_id), source_id.max(target_id));

                        if !processed_pairs.contains(&pair) {
                            processed_pairs.insert(pair);

                            self.create_edge(
                                source_id,
                                target_id,
                                SemanticRelationType::SharedDomain,
                                0.8, // High strength for shared domain
                                Some(&format!("{{\"domain\": \"{}\"}}", domain)),
                            )
                            .await?;

                            edges_created += 1;
                        }
                    }
                }
            }
        }

        info!(
            "Created {} domain-based edges for project {}",
            edges_created, project_id
        );
        Ok(edges_created)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse JSON array of strings
fn parse_json_array(json: &str) -> Vec<String> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Parse JSON array of i64
fn parse_json_array_i64(json: &str) -> Vec<i64> {
    serde_json::from_str(json).unwrap_or_default()
}

/// Compute SHA-256 hash of content
fn compute_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Parse LLM response into SemanticAnalysis
fn parse_semantic_analysis(response: &str) -> Result<SemanticAnalysis> {
    // Try to extract JSON from response
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };

    serde_json::from_str(json_str).context("Failed to parse semantic analysis from LLM response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relationship_type_round_trip() {
        let types = vec![
            SemanticRelationType::SimilarPurpose,
            SemanticRelationType::SharedDomain,
            SemanticRelationType::CoChange,
            SemanticRelationType::Calls,
            SemanticRelationType::CalledBy,
        ];

        for rel_type in types {
            let str_form = rel_type.as_str();
            let parsed = SemanticRelationType::from_str(str_form);
            assert_eq!(parsed, Some(rel_type));
        }
    }

    #[test]
    fn test_parse_json_array() {
        let json = r#"["auth", "database", "api"]"#;
        let result = parse_json_array(json);
        assert_eq!(result, vec!["auth", "database", "api"]);

        let empty = "";
        let result = parse_json_array(empty);
        assert!(result.is_empty());
    }

    #[test]
    fn test_compute_hash() {
        let hash1 = compute_hash("fn main() {}");
        let hash2 = compute_hash("fn main() {}");
        let hash3 = compute_hash("fn other() {}");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_parse_semantic_analysis() {
        let response = r#"
        Here's the analysis:
        {
            "purpose": "Validates user credentials",
            "description": "This function checks username and password",
            "concepts": ["authentication", "validation"],
            "domain_labels": ["security", "user-management"],
            "confidence": 0.92
        }
        "#;

        let analysis = parse_semantic_analysis(response).unwrap();
        assert_eq!(analysis.purpose, "Validates user credentials");
        assert_eq!(analysis.concepts.len(), 2);
        assert_eq!(analysis.confidence, 0.92);
    }
}
