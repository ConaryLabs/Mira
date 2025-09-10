// src/memory/qdrant/store.rs

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{info, warn};
use uuid::Uuid;

use crate::memory::storage::qdrant::mapping::{memory_entry_to_payload, payload_to_memory_entry};
use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::MemoryEntry;

/// Qdrant-based vector store for semantic memory search.
#[derive(Debug)]
pub struct QdrantMemoryStore {
    client: Client,
    pub collection_name: String,
    base_url: String,
}

impl QdrantMemoryStore {
    /// Create a new Qdrant store.
    pub async fn new(url: &str, collection_name: &str) -> Result<Self> {
        let client = Client::builder().http1_only().build()?;
        let base_url = url.to_string();

        // Ensure collection exists with proper configuration.
        let collection_url = format!("{base_url}/collections/{collection_name}");
        match client.get(&collection_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!("✅ Qdrant collection '{}' already exists", collection_name);
            }
            _ => {
                info!("📦 Creating Qdrant collection '{}'", collection_name);

                let create_body = json!({
                    "vectors": {
                        "size": 3072, // text-embedding-3-large dimension
                        "distance": "Cosine"
                    }
                });

                let resp = client
                    .put(&collection_url)
                    .json(&create_body)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    return Err(anyhow!(
                        "Failed to create Qdrant collection: {}",
                        resp.text().await.unwrap_or_default()
                    ));
                }

                info!("✅ Created Qdrant collection '{}'", collection_name);
            }
        }

        Ok(Self {
            client,
            collection_name: collection_name.to_string(),
            base_url,
        })
    }

    /// Search for similar memories using vector similarity.
    pub async fn search_similar_memories(
        &self,
        session_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        info!(
            "🔍 Searching similar vectors in Qdrant for session: {}",
            session_id
        );

        let search_url = format!(
            "{}/collections/{}/points/search",
            self.base_url, self.collection_name
        );

        let search_body = json!({
            "vector": embedding,
            "limit": limit,
            "with_payload": true,
            "with_vector": true, // Also request the vector
            "filter": {
                "must": [
                    {
                        "key": "session_id",
                        "match": {
                            "value": session_id
                        }
                    }
                ]
            }
        });

        let response = self
            .client
            .post(&search_url)
            .json(&search_body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Qdrant search failed: {}",
                response.text().await.unwrap_or_default()
            ));
        }

        let result: Value = response.json().await?;
        let mut entries = Vec::new();

        if let Some(result_array) = result["result"].as_array() {
            for point in result_array {
                if let (Some(payload), Some(vector_val)) = (point.get("payload"), point.get("vector")) {
                    if let Some(vector) = vector_val.as_array().map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect::<Vec<f32>>()) {
                        let id = point.get("id").and_then(|v| v.as_i64());
                        entries.push(payload_to_memory_entry(payload, &vector, id));
                    }
                }
            }
        }

        info!("Found {} similar memories", entries.len());
        Ok(entries)
    }
}

#[async_trait]
impl MemoryStore for QdrantMemoryStore {
    async fn save(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        let embedding = entry
            .embedding
            .as_ref()
            .ok_or_else(|| anyhow!("Cannot save to Qdrant without embedding"))?;

        let point_id = Uuid::new_v4().to_string();
        let payload = memory_entry_to_payload(entry);

        let upsert_url = format!(
            "{}/collections/{}/points?wait=true",
            self.base_url, self.collection_name
        );

        let upsert_body = json!({
            "points": [
                {
                    "id": point_id,
                    "vector": embedding,
                    "payload": payload
                }
            ]
        });

        let response = self
            .client
            .put(&upsert_url)
            .json(&upsert_body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Qdrant save failed: {}",
                response.text().await.unwrap_or_default()
            ));
        }

        info!(
            "✅ Saved memory to Qdrant (salience: {:?})",
            entry.salience
        );
        Ok(entry.clone())
    }

    async fn load_recent(&self, _session_id: &str, _n: usize) -> Result<Vec<MemoryEntry>> {
        warn!("load_recent called on Qdrant store - not supported");
        Ok(Vec::new())
    }

    async fn semantic_search(
        &self,
        session_id: &str,
        embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        self.search_similar_memories(session_id, embedding, k).await
    }

    async fn update_metadata(&self, _id: i64, updated: &MemoryEntry) -> Result<MemoryEntry> {
        warn!("update_metadata not implemented for Qdrant store");
        Ok(updated.clone())
    }

    async fn delete(&self, _id: i64) -> Result<()> {
        warn!("delete by ID not implemented for Qdrant store");
        Ok(())
    }
}
