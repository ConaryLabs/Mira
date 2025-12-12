// src/tools/semantic.rs
// Semantic search utilities for MCP tools using Google Gemini embeddings + Qdrant

use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, Filter, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder, VectorParamsBuilder, DeletePointsBuilder, PointId,
    Value as QdrantValue,
};
use qdrant_client::Qdrant;
use std::collections::HashMap;
use tracing::{debug, info, warn};

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

        Self {
            qdrant,
            gemini_key,
            http_client: reqwest::Client::new(),
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
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let api_key = self.gemini_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Gemini API key not configured"))?;

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent?key={}",
            api_key
        );

        let response = self.http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": "models/gemini-embedding-001",
                "content": {
                    "parts": [{
                        "text": text
                    }]
                },
                "outputDimensionality": EMBEDDING_DIM
            }))
            .send()
            .await?;

        let json: serde_json::Value = response.json().await?;

        if let Some(error) = json.get("error") {
            anyhow::bail!("Gemini API error: {}", error);
        }

        let embedding = json["embedding"]["values"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Invalid embedding response: {:?}", json))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
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
