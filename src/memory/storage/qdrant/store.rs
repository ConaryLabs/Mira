// src/memory/storage/qdrant/store.rs
// Qdrant store for individual collection management

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use crate::memory::storage::qdrant::mapping::{memory_entry_to_payload, payload_to_memory_entry};
use crate::memory::core::traits::MemoryStore;
use crate::memory::core::types::MemoryEntry;

/// Qdrant-based vector store for semantic memory search.
#[derive(Debug, Clone)]
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
                info!("Qdrant collection '{}' already exists", collection_name);
            }
            _ => {
                info!("Creating Qdrant collection '{}'", collection_name);

                let create_body = json!({
                    "vectors": {
                        "size": 3072, // text-embedding-3-large dimension
                        "distance": "Cosine"
                    },
                    "optimizers_config": {
                        "default_segment_number": 2
                    },
                    "replication_factor": 1
                });

                let resp = client
                    .put(&collection_url)
                    .json(&create_body)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    let error_text = resp.text().await.unwrap_or_default();
                    // Check if it's just an "already exists" error
                    if !error_text.contains("already exists") {
                        return Err(anyhow!(
                            "Failed to create Qdrant collection: {}",
                            error_text
                        ));
                    }
                }

                info!("Created Qdrant collection '{}'", collection_name);
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
        debug!(
            "Searching similar vectors in Qdrant collection '{}' for session: {}",
            self.collection_name, session_id
        );

        let search_url = format!(
            "{}/collections/{}/points/search",
            self.base_url, self.collection_name
        );

        let search_body = json!({
            "vector": embedding,
            "limit": limit,
            "with_payload": true,
            "with_vector": true,
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
                    if let Some(vector) = vector_val.as_array().map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                            .collect::<Vec<f32>>()
                    }) {
                        // Get message_id from payload
                        let id = payload.get("id").and_then(|v| v.as_i64());
                        entries.push(payload_to_memory_entry(payload, &vector, id));
                    }
                }
            }
        }

        debug!("Found {} similar memories in {}", entries.len(), self.collection_name);
        Ok(entries)
    }
    
    /// Delete a point by its numeric ID
    pub async fn delete_by_id(&self, message_id: i64) -> Result<()> {
        // Ensure the ID is positive for Qdrant
        if message_id < 0 {
            return Err(anyhow!("Message ID must be positive for Qdrant"));
        }
        
        let point_id = message_id as u64;
        
        let delete_url = format!(
            "{}/collections/{}/points/delete?wait=true",
            self.base_url, self.collection_name
        );

        // Send as integer, not string
        let delete_body = json!({
            "points": [point_id]
        });

        let response = self
            .client
            .post(&delete_url)
            .json(&delete_body)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Qdrant delete failed: {}",
                response.text().await.unwrap_or_default()
            ));
        }

        debug!("Deleted point {} from {}", point_id, self.collection_name);
        Ok(())
    }
    
    /// Count points in the collection
    pub async fn count_points(&self) -> Result<usize> {
        let count_url = format!(
            "{}/collections/{}/points/count",
            self.base_url, self.collection_name
        );

        let response = self.client.post(&count_url).send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow!(
                "Qdrant count failed: {}",
                response.text().await.unwrap_or_default()
            ));
        }

        let result: Value = response.json().await?;
        let count = result["result"]["count"]
            .as_u64()
            .unwrap_or(0) as usize;
        
        Ok(count)
    }
}

#[async_trait]
impl MemoryStore for QdrantMemoryStore {
    async fn save(&self, entry: &MemoryEntry) -> Result<MemoryEntry> {
        let embedding = entry
            .embedding
            .as_ref()
            .ok_or_else(|| anyhow!("Cannot save to Qdrant without embedding"))?;

        // Get message_id and ensure it's positive for Qdrant
        let message_id = entry.id
            .ok_or_else(|| anyhow!("Cannot save to Qdrant without message_id"))?;
        
        if message_id < 0 {
            return Err(anyhow!("Message ID must be positive for Qdrant"));
        }
        
        // Use message_id as unsigned integer - Qdrant point ID
        let point_id = message_id as u64;

        let payload = memory_entry_to_payload(entry);

        let upsert_url = format!(
            "{}/collections/{}/points?wait=true",
            self.base_url, self.collection_name
        );

        // CRITICAL FIX: Send point_id as integer, NOT as string
        let upsert_body = json!({
            "points": [
                {
                    "id": point_id,  // Integer, not string!
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

        debug!(
            "Saved memory {} to Qdrant collection '{}' (point_id: {}, salience: {:?})",
            message_id,
            self.collection_name,
            point_id,
            entry.salience
        );
        Ok(entry.clone())
    }

    async fn load_recent(&self, _session_id: &str, _n: usize) -> Result<Vec<MemoryEntry>> {
        // Qdrant doesn't support time-based queries efficiently
        // This is handled by SQLite instead
        debug!("load_recent called on Qdrant store - not supported");
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
        // Qdrant uses point IDs, not SQLite IDs
        // Metadata updates should be handled through SQLite
        debug!("update_metadata not implemented for Qdrant store");
        Ok(updated.clone())
    }

    async fn delete(&self, id: i64) -> Result<()> {
        // Delete from Qdrant using message_id as point_id
        match self.delete_by_id(id).await {
            Ok(_) => {
                debug!("Deleted message {} from Qdrant collection '{}'", id, self.collection_name);
                Ok(())
            }
            Err(e) => {
                warn!(
                    "Failed to delete message {} from Qdrant collection '{}': {}",
                    id, self.collection_name, e
                );
                // Don't fail - the point might not exist in this collection
                Ok(())
            }
        }
    }
}
