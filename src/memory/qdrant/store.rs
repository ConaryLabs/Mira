// src/memory/qdrant/store.rs
// Phase 2-7: Qdrant vector store implementation
// PHASE 2 UPDATE: Force HTTP/1.1 to fix connection errors

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::{info, warn};
use uuid::Uuid;

use crate::memory::traits::MemoryStore;
use crate::memory::types::MemoryEntry;

/// Qdrant-based vector store for semantic memory search
#[derive(Debug)]
pub struct QdrantMemoryStore {
    client: Client,
    pub collection_name: String,
    base_url: String,
}

impl QdrantMemoryStore {
    /// Create a new Qdrant store
    pub async fn new(url: &str, collection_name: &str) -> Result<Self> {
        // Corrected: Build client and force HTTP/1.1 for compatibility
        let client = Client::builder().http1_only().build()?;
        let base_url = url.to_string();

        // Ensure collection exists with proper configuration
        let collection_url = format!("{}/collections/{}", base_url, collection_name);
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

    /// Search for similar memories using vector similarity
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

        // Build search request with session filter
        let search_body = json!({
            "vector": embedding,
            "limit": limit,
            "with_payload": true,
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

        // Convert results to MemoryEntry
        let mut entries = Vec::new();
        if let Some(result_array) = result["result"].as_array() {
            for point in result_array {
                if let Some(payload) = point["payload"].as_object() {
                    if let Ok(entry) = self.payload_to_memory_entry(payload) {
                        entries.push(entry);
                    }
                }
            }
        }

        info!("Found {} similar memories", entries.len());
        Ok(entries)
    }

    /// Convert Qdrant payload to MemoryEntry
    fn payload_to_memory_entry(
        &self,
        payload: &serde_json::Map<String, Value>,
    ) -> Result<MemoryEntry> {
        let entry = MemoryEntry {
            id: payload.get("id").and_then(|v| v.as_i64()),
            session_id: payload
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            role: payload
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("user")
                .to_string(),
            content: payload
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            timestamp: payload
                .get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(chrono::Utc::now),
            embedding: None, // Don't return embedding in search results
            salience: payload
                .get("salience")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32),
            tags: payload.get("tags").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }),
            summary: payload
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from),
            memory_type: payload
                .get("memory_type")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok()),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };

        Ok(entry)
    }
}

#[async_trait]
impl MemoryStore for QdrantMemoryStore {
    async fn save(&self, entry: &MemoryEntry) -> Result<()> {
        // Only save if we have an embedding
        let embedding = entry
            .embedding
            .as_ref()
            .ok_or_else(|| anyhow!("Cannot save to Qdrant without embedding"))?;

        // Generate a unique ID for the point
        let point_id = Uuid::new_v4().to_string();

        // Create payload from entry
        let payload = json!({
            "id": entry.id,
            "session_id": entry.session_id,
            "role": entry.role,
            "content": entry.content,
            "timestamp": entry.timestamp.to_rfc3339(),
            "salience": entry.salience,
            "tags": entry.tags,
            "summary": entry.summary,
            "memory_type": entry.memory_type.as_ref().map(|t| format!("{:?}", t)),
        });

        let upsert_url = format!(
            "{}/collections/{}/points?wait=true", // Added wait=true for consistency
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
        Ok(())
    }

    async fn load_recent(&self, _session_id: &str, _n: usize) -> Result<Vec<MemoryEntry>> {
        // Qdrant is not used for recency queries - use SQLite instead
        warn!("load_recent called on Qdrant store - not supported");
        Ok(Vec::new())
    }

    async fn semantic_search(
        &self,
        session_id: &str,
        embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Delegate to our search method
        self.search_similar_memories(session_id, embedding, k).await
    }

    async fn update_metadata(&self, _id: i64, _updated: &MemoryEntry) -> Result<()> {
        // Metadata update not implemented for Qdrant in this version
        warn!("update_metadata not implemented for Qdrant store");
        Ok(())
    }

    async fn delete(&self, _id: i64) -> Result<()> {
        // Deletion by ID not implemented for Qdrant in this version
        warn!("delete by ID not implemented for Qdrant store");
        Ok(())
    }
}

