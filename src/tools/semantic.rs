// src/tools/semantic.rs
// Semantic search utilities for MCP tools using Google Gemini embeddings + Qdrant

use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, Distance, Filter, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder, VectorParamsBuilder, DeletePointsBuilder, PointId,
    Value as QdrantValue,
};
use qdrant_client::Qdrant;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Timeouts for external API calls
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const EMBED_RETRY_ATTEMPTS: u32 = 2;
const RETRY_DELAY: Duration = Duration::from_millis(500);

/// Embedding dimensions for gemini-embedding-001 (max precision)
const EMBEDDING_DIM: u64 = 3072;

/// Collection names
pub const COLLECTION_CODE: &str = "mira_code";
pub const COLLECTION_CONVERSATION: &str = "mira_conversation";
pub const COLLECTION_DOCS: &str = "mira_docs";

/// Semantic search client wrapping Google Gemini embeddings + Qdrant
pub struct SemanticSearch {
    qdrant: Option<Qdrant>,
    gemini_key: Option<String>,
    http_client: reqwest::Client,
}

impl SemanticSearch {
    /// Create a new semantic search client
    /// Gracefully handles missing Qdrant or Gemini config
    pub async fn new(qdrant_url: Option<&str>, gemini_key: Option<String>) -> Self {
        let qdrant = if let Some(url) = qdrant_url {
            match Qdrant::from_url(url).skip_compatibility_check().build() {
                Ok(client) => {
                    info!("Connected to Qdrant at {}", url);
                    Some(client)
                }
                Err(e) => {
                    warn!("Failed to connect to Qdrant: {} - semantic search disabled", e);
                    None
                }
            }
        } else {
            debug!("No Qdrant URL configured - semantic search disabled");
            None
        };

        let http_client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            qdrant,
            gemini_key,
            http_client,
        }
    }

    /// Check if semantic search is available
    pub fn is_available(&self) -> bool {
        self.qdrant.is_some() && self.gemini_key.is_some()
    }

    /// Ensure a collection exists
    pub async fn ensure_collection(&self, collection: &str) -> Result<()> {
        let qdrant = self.qdrant.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        let exists = qdrant.collection_exists(collection).await?;
        if !exists {
            info!("Creating Qdrant collection: {}", collection);
            qdrant
                .create_collection(
                    CreateCollectionBuilder::new(collection)
                        .vectors_config(VectorParamsBuilder::new(EMBEDDING_DIM, Distance::Cosine)),
                )
                .await
                .context(format!("Failed to create collection: {}", collection))?;
        }
        Ok(())
    }

    /// Get embedding for text using Google Gemini
    /// Includes retry logic for transient failures
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let api_key = self.gemini_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Gemini API key not configured"))?;

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent?key={}",
            api_key
        );

        let body = serde_json::json!({
            "model": "models/gemini-embedding-001",
            "content": {
                "parts": [{
                    "text": text
                }]
            },
            "outputDimensionality": EMBEDDING_DIM
        });

        let mut last_error = None;
        for attempt in 0..=EMBED_RETRY_ATTEMPTS {
            if attempt > 0 {
                debug!("Retrying embed (attempt {})", attempt + 1);
                tokio::time::sleep(RETRY_DELAY).await;
            }

            let result = self.http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await;

            match result {
                Ok(response) => {
                    let json: serde_json::Value = match response.json().await {
                        Ok(j) => j,
                        Err(e) => {
                            last_error = Some(anyhow::anyhow!("Failed to parse response: {}", e));
                            continue;
                        }
                    };

                    if let Some(error) = json.get("error") {
                        // Don't retry on auth/quota errors
                        let error_str = error.to_string();
                        if error_str.contains("API_KEY") || error_str.contains("QUOTA") {
                            anyhow::bail!("Gemini API error: {}", error);
                        }
                        last_error = Some(anyhow::anyhow!("Gemini API error: {}", error));
                        continue;
                    }

                    let embedding = json["embedding"]["values"]
                        .as_array()
                        .ok_or_else(|| anyhow::anyhow!("Invalid embedding response"))?
                        .iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect();

                    return Ok(embedding);
                }
                Err(e) => {
                    // Retry on network errors
                    last_error = Some(anyhow::anyhow!("Request failed: {}", e));
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Embedding failed after retries")))
    }

    /// Store a point in a collection
    pub async fn store(
        &self,
        collection: &str,
        id: &str,
        content: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let qdrant = self.qdrant.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        // Get embedding
        let embedding = self.embed(content).await?;

        // Convert metadata to Qdrant payload
        let mut payload: HashMap<String, QdrantValue> = HashMap::new();
        payload.insert("content".to_string(), content.to_string().into());

        for (key, value) in metadata {
            let qdrant_value = match value {
                serde_json::Value::String(s) => s.into(),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        i.into()
                    } else if let Some(f) = n.as_f64() {
                        f.into()
                    } else {
                        continue;
                    }
                }
                serde_json::Value::Bool(b) => b.into(),
                _ => continue,
            };
            payload.insert(key, qdrant_value);
        }

        // Hash ID to u64
        let numeric_id = hash_string(id);

        let point = PointStruct::new(numeric_id, embedding, payload);

        qdrant
            .upsert_points(UpsertPointsBuilder::new(collection, vec![point]).wait(true))
            .await
            .context("Failed to store point")?;

        debug!("Stored point {} in {}", id, collection);
        Ok(())
    }

    /// Search a collection for similar content
    pub async fn search(
        &self,
        collection: &str,
        query: &str,
        limit: usize,
        filter: Option<Filter>,
    ) -> Result<Vec<SearchResult>> {
        let qdrant = self.qdrant.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        // Get query embedding
        let embedding = self.embed(query).await?;

        // Search
        let mut search = SearchPointsBuilder::new(collection, embedding, limit as u64)
            .with_payload(true);

        if let Some(f) = filter {
            search = search.filter(f);
        }

        let results = qdrant.search_points(search).await?;

        let entries: Vec<SearchResult> = results
            .result
            .into_iter()
            .filter_map(|point| {
                let content = point.payload.get("content")?.as_str()?.to_string();
                let score = point.score;

                // Extract all metadata
                let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
                for (key, value) in &point.payload {
                    if key != "content" {
                        if let Some(s) = value.as_str() {
                            metadata.insert(key.clone(), serde_json::Value::String(s.to_string()));
                        } else if let Some(i) = value.as_integer() {
                            metadata.insert(key.clone(), serde_json::Value::Number(i.into()));
                        } else if let Some(b) = value.as_bool() {
                            metadata.insert(key.clone(), serde_json::Value::Bool(b));
                        }
                    }
                }

                Some(SearchResult {
                    content,
                    score,
                    metadata,
                })
            })
            .collect();

        Ok(entries)
    }

    /// Delete a point by ID
    pub async fn delete(&self, collection: &str, id: &str) -> Result<()> {
        let qdrant = self.qdrant.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        let numeric_id = hash_string(id);

        qdrant
            .delete_points(
                DeletePointsBuilder::new(collection)
                    .points(vec![PointId::from(numeric_id)]),
            )
            .await
            .context("Failed to delete point")?;

        Ok(())
    }

    /// Delete all points matching a field value (e.g., all embeddings for a file)
    pub async fn delete_by_field(&self, collection: &str, field: &str, value: &str) -> Result<u64> {
        let qdrant = self.qdrant.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        // Check if collection exists first
        if !qdrant.collection_exists(collection).await? {
            return Ok(0);
        }

        let result = qdrant
            .delete_points(
                DeletePointsBuilder::new(collection)
                    .points(Filter::must([Condition::matches(
                        field,
                        value.to_string(),
                    )]))
                    .wait(true),
            )
            .await
            .context("Failed to delete points by filter")?;

        let deleted = result.result.map(|r| r.status).unwrap_or(0) as u64;
        debug!("Deleted points where {}={} from {}", field, value, collection);
        Ok(deleted)
    }
}

/// Search result from Qdrant
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub content: String,
    pub score: f32,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Hash a string to u64 for Qdrant point ID
fn hash_string(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
