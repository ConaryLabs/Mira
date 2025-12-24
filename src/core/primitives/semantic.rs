//! Semantic search with Qdrant + Gemini embeddings
//! Test index: 2025-12-23 v3
//!
//! High-level semantic search operations including:
//! - Embedding via Google Gemini (single and batch)
//! - Vector storage and search via Qdrant
//! - Collection management
//! - Bulk operations (store_batch, delete_by_field)

use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, DeletePointsBuilder, Distance, Filter, PointId,
    PointStruct, SearchPointsBuilder, UpsertPointsBuilder, Value as QdrantValue,
    VectorParamsBuilder,
};
use qdrant_client::Qdrant;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

use super::limits::{
    EMBED_BATCH_MAX, EMBED_RETRY_ATTEMPTS, EMBED_TEXT_MAX_CHARS, EMBEDDING_DIM,
    HTTP_TIMEOUT_SECS, RETRY_DELAY_MS, SEMANTIC_SEARCH_MAX_LIMIT, SEMANTIC_SEARCH_MIN_SCORE,
};

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
                    warn!(
                        "Failed to connect to Qdrant: {} - semantic search disabled",
                        e
                    );
                    None
                }
            }
        } else {
            debug!("No Qdrant URL configured - semantic search disabled");
            None
        };

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
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
        let qdrant = self
            .qdrant
            .as_ref()
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
    /// Text is truncated to EMBED_TEXT_MAX_CHARS if too long
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let api_key = self
            .gemini_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Gemini API key not configured"))?;

        // Truncate text if too long (Gemini has token limits)
        let text = if text.len() > EMBED_TEXT_MAX_CHARS {
            debug!("Truncating text from {} to {} chars for embedding", text.len(), EMBED_TEXT_MAX_CHARS);
            &text[..EMBED_TEXT_MAX_CHARS]
        } else {
            text
        };

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

        let retry_delay = Duration::from_millis(RETRY_DELAY_MS);
        let mut last_error = None;

        for attempt in 0..=EMBED_RETRY_ATTEMPTS {
            if attempt > 0 {
                debug!("Retrying embed (attempt {})", attempt + 1);
                tokio::time::sleep(retry_delay).await;
            }

            let result = self
                .http_client
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

    /// Batch embed multiple texts in a single API call (more efficient)
    /// Returns embeddings in the same order as input texts
    /// Texts are truncated to EMBED_TEXT_MAX_CHARS and batches are chunked to EMBED_BATCH_MAX
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        // For small batches, just use sequential embedding
        if texts.len() <= 2 {
            let mut results = Vec::with_capacity(texts.len());
            for text in texts {
                results.push(self.embed(text).await?);
            }
            return Ok(results);
        }

        // Truncate texts that are too long
        let truncated: Vec<String> = texts
            .iter()
            .map(|t| {
                if t.len() > EMBED_TEXT_MAX_CHARS {
                    t[..EMBED_TEXT_MAX_CHARS].to_string()
                } else {
                    t.clone()
                }
            })
            .collect();

        // If batch is too large, process in chunks
        if truncated.len() > EMBED_BATCH_MAX {
            let mut all_results = Vec::with_capacity(truncated.len());
            for chunk in truncated.chunks(EMBED_BATCH_MAX) {
                let chunk_results = self.embed_batch_inner(chunk).await?;
                all_results.extend(chunk_results);
            }
            return Ok(all_results);
        }

        self.embed_batch_inner(&truncated).await
    }

    /// Internal batch embed (assumes texts are already truncated and within batch limit)
    async fn embed_batch_inner(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let api_key = self
            .gemini_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Gemini API key not configured"))?;

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:batchEmbedContents?key={}",
            api_key
        );

        // Build batch request
        let requests: Vec<serde_json::Value> = texts
            .iter()
            .map(|text| {
                serde_json::json!({
                    "model": "models/gemini-embedding-001",
                    "content": {
                        "parts": [{
                            "text": text
                        }]
                    },
                    "outputDimensionality": EMBEDDING_DIM
                })
            })
            .collect();

        let body = serde_json::json!({
            "requests": requests
        });

        let retry_delay = Duration::from_millis(RETRY_DELAY_MS);
        let mut last_error = None;

        for attempt in 0..=EMBED_RETRY_ATTEMPTS {
            if attempt > 0 {
                debug!("Retrying batch embed (attempt {})", attempt + 1);
                tokio::time::sleep(retry_delay).await;
            }

            let result = self
                .http_client
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
                            last_error =
                                Some(anyhow::anyhow!("Failed to parse batch response: {}", e));
                            continue;
                        }
                    };

                    if let Some(error) = json.get("error") {
                        let error_str = error.to_string();
                        if error_str.contains("API_KEY") || error_str.contains("QUOTA") {
                            anyhow::bail!("Gemini API error: {}", error);
                        }
                        last_error = Some(anyhow::anyhow!("Gemini batch API error: {}", error));
                        continue;
                    }

                    let embeddings = json["embeddings"]
                        .as_array()
                        .ok_or_else(|| anyhow::anyhow!("Invalid batch embedding response"))?
                        .iter()
                        .map(|emb| {
                            emb["values"]
                                .as_array()
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                                        .collect()
                                })
                                .unwrap_or_default()
                        })
                        .collect();

                    return Ok(embeddings);
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Batch request failed: {}", e));
                    continue;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Batch embedding failed after retries")))
    }

    /// Store multiple points in a collection (more efficient than individual stores)
    pub async fn store_batch(
        &self,
        collection: &str,
        items: Vec<(String, String, HashMap<String, serde_json::Value>)>, // (id, content, metadata)
    ) -> Result<usize> {
        if items.is_empty() {
            return Ok(0);
        }

        let qdrant = self
            .qdrant
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        // Get all embeddings in one batch call (with timeout to prevent hanging)
        let texts: Vec<String> = items.iter().map(|(_, content, _)| content.clone()).collect();
        tracing::debug!("[SEMANTIC] Calling embed_batch for {} items...", texts.len());
        let embeddings = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.embed_batch(&texts)
        ).await
            .map_err(|_| anyhow::anyhow!("Gemini embedding API timeout after 30s"))?
            ?;
        tracing::debug!("[SEMANTIC] embed_batch complete: {} embeddings", embeddings.len());

        if embeddings.len() != items.len() {
            anyhow::bail!(
                "Embedding count mismatch: got {} for {} items",
                embeddings.len(),
                items.len()
            );
        }

        // Build points
        let points: Vec<PointStruct> = items
            .iter()
            .zip(embeddings.iter())
            .map(|((id, content, metadata), embedding)| {
                let mut payload: HashMap<String, QdrantValue> = HashMap::new();
                payload.insert("content".to_string(), content.clone().into());

                for (key, value) in metadata {
                    let qdrant_value = match value {
                        serde_json::Value::String(s) => s.clone().into(),
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                i.into()
                            } else if let Some(f) = n.as_f64() {
                                f.into()
                            } else {
                                continue;
                            }
                        }
                        serde_json::Value::Bool(b) => (*b).into(),
                        _ => continue,
                    };
                    payload.insert(key.clone(), qdrant_value);
                }

                let numeric_id = hash_string(id);
                PointStruct::new(numeric_id, embedding.clone(), payload)
            })
            .collect();

        let count = points.len();

        // Upsert all points at once (with timeout to prevent hanging)
        tracing::debug!("[SEMANTIC] Upserting {} points to Qdrant...", count);
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            qdrant.upsert_points(UpsertPointsBuilder::new(collection, points).wait(true))
        ).await
            .map_err(|_| anyhow::anyhow!("Qdrant upsert timeout after 30s"))?
            .context("Failed to batch store points")?;

        debug!("Batch stored {} points in {}", count, collection);
        tracing::debug!("[SEMANTIC] Qdrant upsert complete");
        Ok(count)
    }

    /// Store a point in a collection
    pub async fn store(
        &self,
        collection: &str,
        id: &str,
        content: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let qdrant = self
            .qdrant
            .as_ref()
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
    /// Limit is capped at SEMANTIC_SEARCH_MAX_LIMIT and results below SEMANTIC_SEARCH_MIN_SCORE are filtered
    pub async fn search(
        &self,
        collection: &str,
        query: &str,
        limit: usize,
        filter: Option<Filter>,
    ) -> Result<Vec<SearchResult>> {
        let qdrant = self
            .qdrant
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        // Enforce maximum limit
        let limit = limit.min(SEMANTIC_SEARCH_MAX_LIMIT);

        // Get query embedding
        let embedding = self.embed(query).await?;

        // Search
        let mut search = SearchPointsBuilder::new(collection, embedding, limit as u64).with_payload(true);

        if let Some(f) = filter {
            search = search.filter(f);
        }

        let results = qdrant.search_points(search).await?;

        let entries: Vec<SearchResult> = results
            .result
            .into_iter()
            .filter_map(|point| {
                let score = point.score;

                // Filter out low-quality matches
                if score < SEMANTIC_SEARCH_MIN_SCORE {
                    return None;
                }

                let content = point.payload.get("content")?.as_str()?.to_string();

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
        let qdrant = self
            .qdrant
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        let numeric_id = hash_string(id);

        qdrant
            .delete_points(
                DeletePointsBuilder::new(collection).points(vec![PointId::from(numeric_id)]),
            )
            .await
            .context("Failed to delete point")?;

        Ok(())
    }

    /// Delete all points matching a field value (e.g., all embeddings for a file)
    pub async fn delete_by_field(&self, collection: &str, field: &str, value: &str) -> Result<u64> {
        let qdrant = self
            .qdrant
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Qdrant not available"))?;

        // Check if collection exists first (with timeout)
        let exists = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            qdrant.collection_exists(collection)
        ).await
            .map_err(|_| anyhow::anyhow!("Qdrant collection_exists timeout"))?
            ?;
        if !exists {
            return Ok(0);
        }

        // Delete with timeout to prevent hanging
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            qdrant.delete_points(
                DeletePointsBuilder::new(collection)
                    .points(Filter::must([Condition::matches(field, value.to_string())]))
                    .wait(true),
            )
        ).await
            .map_err(|_| anyhow::anyhow!("Qdrant delete_points timeout after 30s"))?
            .context("Failed to delete points by filter")?;

        let deleted = result.result.map(|r| r.status).unwrap_or(0) as u64;
        debug!(
            "Deleted points where {}={} from {}",
            field, value, collection
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_string_deterministic() {
        let id = "test-point-id";
        let hash1 = hash_string(id);
        let hash2 = hash_string(id);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_string_unique() {
        let hash1 = hash_string("point-a");
        let hash2 = hash_string("point-b");
        assert_ne!(hash1, hash2);
    }
}
