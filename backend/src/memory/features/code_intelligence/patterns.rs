// backend/src/memory/features/code_intelligence/patterns.rs
// Design pattern detection: LLM-based pattern recognition and caching

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm::provider::LlmProvider;

// ============================================================================
// Data Structures
// ============================================================================

/// Common design patterns we detect
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PatternType {
    // Creational
    Factory,
    AbstractFactory,
    Builder,
    Singleton,
    Prototype,
    // Structural
    Adapter,
    Bridge,
    Composite,
    Decorator,
    Facade,
    Proxy,
    // Behavioral
    Command,
    Iterator,
    Observer,
    Strategy,
    TemplateMethod,
    Visitor,
    State,
    // Architectural
    Repository,
    ServiceLayer,
    MVC,
    EventDriven,
    DependencyInjection,
    // Other
    Custom(String),
}

impl PatternType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Factory => "factory",
            Self::AbstractFactory => "abstract_factory",
            Self::Builder => "builder",
            Self::Singleton => "singleton",
            Self::Prototype => "prototype",
            Self::Adapter => "adapter",
            Self::Bridge => "bridge",
            Self::Composite => "composite",
            Self::Decorator => "decorator",
            Self::Facade => "facade",
            Self::Proxy => "proxy",
            Self::Command => "command",
            Self::Iterator => "iterator",
            Self::Observer => "observer",
            Self::Strategy => "strategy",
            Self::TemplateMethod => "template_method",
            Self::Visitor => "visitor",
            Self::State => "state",
            Self::Repository => "repository",
            Self::ServiceLayer => "service_layer",
            Self::MVC => "mvc",
            Self::EventDriven => "event_driven",
            Self::DependencyInjection => "dependency_injection",
            Self::Custom(name) => name,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "factory" => Self::Factory,
            "abstract_factory" => Self::AbstractFactory,
            "builder" => Self::Builder,
            "singleton" => Self::Singleton,
            "prototype" => Self::Prototype,
            "adapter" => Self::Adapter,
            "bridge" => Self::Bridge,
            "composite" => Self::Composite,
            "decorator" => Self::Decorator,
            "facade" => Self::Facade,
            "proxy" => Self::Proxy,
            "command" => Self::Command,
            "iterator" => Self::Iterator,
            "observer" => Self::Observer,
            "strategy" => Self::Strategy,
            "template_method" => Self::TemplateMethod,
            "visitor" => Self::Visitor,
            "state" => Self::State,
            "repository" => Self::Repository,
            "service_layer" => Self::ServiceLayer,
            "mvc" => Self::MVC,
            "event_driven" => Self::EventDriven,
            "dependency_injection" => Self::DependencyInjection,
            _ => Self::Custom(s.to_string()),
        }
    }

    /// Get all standard pattern types for detection
    pub fn all_standard() -> Vec<Self> {
        vec![
            Self::Factory,
            Self::Builder,
            Self::Singleton,
            Self::Adapter,
            Self::Decorator,
            Self::Facade,
            Self::Observer,
            Self::Strategy,
            Self::Repository,
            Self::ServiceLayer,
            Self::DependencyInjection,
        ]
    }
}

/// A detected design pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignPattern {
    pub id: Option<i64>,
    pub project_id: String,
    pub pattern_name: String,
    pub pattern_type: PatternType,
    pub confidence: f64,
    pub involved_symbols: Vec<i64>,
    pub description: Option<String>,
    pub embedding_point_id: Option<String>,
    pub detected_at: i64,
}

/// Pattern detection result from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternDetectionResult {
    pub pattern_type: String,
    pub confidence: f64,
    pub description: String,
    pub evidence: Vec<String>,
    pub involved_elements: Vec<String>,
}

/// Cached pattern validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternValidation {
    pub id: Option<i64>,
    pub pattern_type: String,
    pub code_hash: String,
    pub validation_result: PatternDetectionResult,
    pub confidence: f64,
    pub created_at: i64,
    pub last_used: i64,
    pub hit_count: i64,
}

// ============================================================================
// Pattern Detection Service
// ============================================================================

/// Service for detecting and managing design patterns
pub struct PatternDetectionService {
    pool: SqlitePool,
    llm_provider: Arc<dyn LlmProvider>,
}

impl PatternDetectionService {
    pub fn new(pool: SqlitePool, llm_provider: Arc<dyn LlmProvider>) -> Self {
        Self { pool, llm_provider }
    }

    // ========================================================================
    // Pattern Detection
    // ========================================================================

    /// Detect patterns in a set of code elements
    pub async fn detect_patterns(
        &self,
        project_id: &str,
        element_ids: &[i64],
    ) -> Result<Vec<DesignPattern>> {
        info!(
            "Detecting patterns in project {} ({} elements)",
            project_id,
            element_ids.len()
        );

        // Fetch code elements
        let elements = self.fetch_elements(element_ids).await?;
        if elements.is_empty() {
            return Ok(Vec::new());
        }

        // Build code context
        let code_context = self.build_code_context(&elements);
        let code_hash = compute_hash(&code_context);

        // Check cache for each pattern type
        let mut detected_patterns: Vec<DesignPattern> = Vec::new();

        for pattern_type in PatternType::all_standard() {
            // Check cache
            if let Some(cached) = self
                .get_cached_validation(&pattern_type.as_str().to_string(), &code_hash)
                .await?
            {
                if cached.confidence > 0.5 {
                    debug!("Found cached pattern: {} ({})", pattern_type.as_str(), cached.confidence);
                    let pattern = DesignPattern {
                        id: None,
                        project_id: project_id.to_string(),
                        pattern_name: pattern_type.as_str().to_string(),
                        pattern_type: pattern_type.clone(),
                        confidence: cached.confidence,
                        involved_symbols: element_ids.to_vec(),
                        description: Some(cached.validation_result.description),
                        embedding_point_id: None,
                        detected_at: chrono::Utc::now().timestamp(),
                    };
                    detected_patterns.push(pattern);
                }
                continue;
            }

            // Detect with LLM
            if let Ok(result) = self
                .detect_pattern_with_llm(&pattern_type, &code_context)
                .await
            {
                // Cache the result
                self.cache_validation(&pattern_type.as_str().to_string(), &code_hash, &result)
                    .await?;

                if result.confidence > 0.5 {
                    info!(
                        "Detected pattern: {} with confidence {:.2}",
                        pattern_type.as_str(),
                        result.confidence
                    );

                    let pattern = DesignPattern {
                        id: None,
                        project_id: project_id.to_string(),
                        pattern_name: pattern_type.as_str().to_string(),
                        pattern_type: pattern_type.clone(),
                        confidence: result.confidence,
                        involved_symbols: element_ids.to_vec(),
                        description: Some(result.description),
                        embedding_point_id: None,
                        detected_at: chrono::Utc::now().timestamp(),
                    };
                    detected_patterns.push(pattern);
                }
            }
        }

        // Store detected patterns
        for pattern in &mut detected_patterns {
            let id = self.store_pattern(pattern).await?;
            pattern.id = Some(id);
        }

        Ok(detected_patterns)
    }

    /// Detect a specific pattern type using LLM
    async fn detect_pattern_with_llm(
        &self,
        pattern_type: &PatternType,
        code_context: &str,
    ) -> Result<PatternDetectionResult> {
        let prompt = format!(
            r#"Analyze this code to determine if it implements the {} design pattern.

Code:
```
{}
```

Respond in JSON format:
{{
  "pattern_type": "{}",
  "confidence": 0.0 to 1.0,
  "description": "Brief description of how the pattern is implemented",
  "evidence": ["List", "of", "specific", "code", "elements", "that", "indicate", "the", "pattern"],
  "involved_elements": ["Names", "of", "classes/functions", "involved"]
}}

If the pattern is NOT present, set confidence to 0.0.
If partially present, set confidence between 0.3-0.6.
If clearly present, set confidence > 0.7.
"#,
            pattern_type.as_str(),
            code_context,
            pattern_type.as_str()
        );

        let messages = vec![crate::llm::provider::Message::user(prompt)];
        let response = self
            .llm_provider
            .chat(messages, "You are an expert at identifying design patterns in code.".to_string())
            .await
            .context("LLM pattern detection failed")?;

        parse_pattern_result(&response.content)
    }

    /// Fetch code elements by IDs
    async fn fetch_elements(&self, element_ids: &[i64]) -> Result<Vec<CodeElementInfo>> {
        let mut elements = Vec::new();

        for id in element_ids {
            let row = sqlx::query!(
                r#"
                SELECT id as "id!", name, element_type, content, file_path
                FROM code_elements
                WHERE id = ?
                "#,
                id
            )
            .fetch_optional(&self.pool)
            .await?;

            if let Some(r) = row {
                elements.push(CodeElementInfo {
                    id: r.id,
                    name: r.name,
                    element_type: r.element_type,
                    content: r.content.unwrap_or_default(),
                    file_path: r.file_path.unwrap_or_default(),
                });
            }
        }

        Ok(elements)
    }

    /// Build code context from elements
    fn build_code_context(&self, elements: &[CodeElementInfo]) -> String {
        elements
            .iter()
            .map(|e| {
                format!(
                    "// {} ({}) from {}\n{}",
                    e.name, e.element_type, e.file_path, e.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    // ========================================================================
    // Pattern Storage
    // ========================================================================

    /// Store a detected pattern
    async fn store_pattern(&self, pattern: &DesignPattern) -> Result<i64> {
        let involved_symbols_json = serde_json::to_string(&pattern.involved_symbols)?;
        let pattern_type_str = pattern.pattern_type.as_str();

        let result = sqlx::query!(
            r#"
            INSERT INTO design_patterns (
                project_id, pattern_name, pattern_type, confidence,
                involved_symbols, description, detected_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?)
            RETURNING id
            "#,
            pattern.project_id,
            pattern.pattern_name,
            pattern_type_str,
            pattern.confidence,
            involved_symbols_json,
            pattern.description,
            pattern.detected_at
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(result.id)
    }

    /// Get patterns for a project
    pub async fn get_patterns(&self, project_id: &str) -> Result<Vec<DesignPattern>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, pattern_name, pattern_type, confidence,
                   involved_symbols, description, embedding_point_id, detected_at
            FROM design_patterns
            WHERE project_id = ?
            ORDER BY confidence DESC
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let involved_symbols: Vec<i64> =
                    serde_json::from_str(&r.involved_symbols).unwrap_or_default();
                DesignPattern {
                    id: r.id,
                    project_id: r.project_id,
                    pattern_name: r.pattern_name,
                    pattern_type: PatternType::from_str(&r.pattern_type),
                    confidence: r.confidence,
                    involved_symbols,
                    description: r.description,
                    embedding_point_id: r.embedding_point_id,
                    detected_at: r.detected_at,
                }
            })
            .collect())
    }

    /// Get patterns by type
    pub async fn get_patterns_by_type(
        &self,
        project_id: &str,
        pattern_type: &PatternType,
    ) -> Result<Vec<DesignPattern>> {
        let pattern_type_str = pattern_type.as_str();

        let rows = sqlx::query!(
            r#"
            SELECT id, project_id, pattern_name, pattern_type, confidence,
                   involved_symbols, description, embedding_point_id, detected_at
            FROM design_patterns
            WHERE project_id = ? AND pattern_type = ?
            ORDER BY confidence DESC
            "#,
            project_id,
            pattern_type_str
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let involved_symbols: Vec<i64> =
                    serde_json::from_str(&r.involved_symbols).unwrap_or_default();
                DesignPattern {
                    id: r.id,
                    project_id: r.project_id,
                    pattern_name: r.pattern_name,
                    pattern_type: PatternType::from_str(&r.pattern_type),
                    confidence: r.confidence,
                    involved_symbols,
                    description: r.description,
                    embedding_point_id: r.embedding_point_id,
                    detected_at: r.detected_at,
                }
            })
            .collect())
    }

    /// Delete patterns for a project
    pub async fn delete_patterns(&self, project_id: &str) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM design_patterns WHERE project_id = ?", project_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    // ========================================================================
    // Validation Cache
    // ========================================================================

    /// Get cached validation result
    async fn get_cached_validation(
        &self,
        pattern_type: &str,
        code_hash: &str,
    ) -> Result<Option<PatternValidation>> {
        let row = sqlx::query!(
            r#"
            SELECT id, pattern_type, code_hash, validation_result, confidence,
                   created_at, last_used, hit_count
            FROM pattern_validation_cache
            WHERE pattern_type = ? AND code_hash = ?
            "#,
            pattern_type,
            code_hash
        )
        .fetch_optional(&self.pool)
        .await?;

        if let Some(r) = row {
            // Update hit count
            let now = chrono::Utc::now().timestamp();
            sqlx::query!(
                "UPDATE pattern_validation_cache SET last_used = ?, hit_count = hit_count + 1 WHERE id = ?",
                now,
                r.id
            )
            .execute(&self.pool)
            .await?;

            if let Ok(validation_result) =
                serde_json::from_str::<PatternDetectionResult>(&r.validation_result)
            {
                return Ok(Some(PatternValidation {
                    id: r.id,
                    pattern_type: r.pattern_type,
                    code_hash: r.code_hash,
                    validation_result,
                    confidence: r.confidence,
                    created_at: r.created_at,
                    last_used: r.last_used,
                    hit_count: r.hit_count.unwrap_or(0),
                }));
            }
        }

        Ok(None)
    }

    /// Cache a validation result
    async fn cache_validation(
        &self,
        pattern_type: &str,
        code_hash: &str,
        result: &PatternDetectionResult,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        let result_json = serde_json::to_string(result)?;

        sqlx::query!(
            r#"
            INSERT INTO pattern_validation_cache (
                pattern_type, code_hash, validation_result, confidence, created_at, last_used, hit_count
            ) VALUES (?, ?, ?, ?, ?, ?, 0)
            ON CONFLICT(pattern_type, code_hash) DO UPDATE SET
                validation_result = excluded.validation_result,
                confidence = excluded.confidence,
                last_used = excluded.last_used
            "#,
            pattern_type,
            code_hash,
            result_json,
            result.confidence,
            now,
            now
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Clean up old cache entries
    pub async fn cleanup_cache(&self, max_age_days: i64) -> Result<u64> {
        let cutoff = chrono::Utc::now().timestamp() - (max_age_days * 86400);

        let result = sqlx::query!(
            "DELETE FROM pattern_validation_cache WHERE last_used < ?",
            cutoff
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

// ============================================================================
// Helper Structures
// ============================================================================

/// Code element info for pattern detection
#[derive(Debug, Clone)]
struct CodeElementInfo {
    id: i64,
    name: String,
    element_type: String,
    content: String,
    file_path: String,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Compute SHA-256 hash
fn compute_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Parse LLM response into PatternDetectionResult
fn parse_pattern_result(response: &str) -> Result<PatternDetectionResult> {
    // Extract JSON from response
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            response
        }
    } else {
        response
    };

    serde_json::from_str(json_str).context("Failed to parse pattern detection result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_type_round_trip() {
        let types = vec![
            PatternType::Factory,
            PatternType::Builder,
            PatternType::Singleton,
            PatternType::Repository,
            PatternType::Observer,
        ];

        for pattern_type in types {
            let str_form = pattern_type.as_str();
            let parsed = PatternType::from_str(str_form);
            assert_eq!(parsed.as_str(), pattern_type.as_str());
        }
    }

    #[test]
    fn test_custom_pattern_type() {
        let custom = PatternType::Custom("my_pattern".to_string());
        assert_eq!(custom.as_str(), "my_pattern");

        let parsed = PatternType::from_str("unknown_pattern");
        match parsed {
            PatternType::Custom(name) => assert_eq!(name, "unknown_pattern"),
            _ => panic!("Expected Custom variant"),
        }
    }

    #[test]
    fn test_all_standard_patterns() {
        let standard = PatternType::all_standard();
        assert!(standard.len() >= 10);
        assert!(standard.contains(&PatternType::Factory));
        assert!(standard.contains(&PatternType::Repository));
    }

    #[test]
    fn test_parse_pattern_result() {
        let response = r#"
        Analysis result:
        {
            "pattern_type": "repository",
            "confidence": 0.85,
            "description": "This implements the Repository pattern",
            "evidence": ["interface definition", "CRUD methods"],
            "involved_elements": ["UserRepository", "User"]
        }
        "#;

        let result = parse_pattern_result(response).unwrap();
        assert_eq!(result.pattern_type, "repository");
        assert_eq!(result.confidence, 0.85);
        assert_eq!(result.evidence.len(), 2);
    }
}
