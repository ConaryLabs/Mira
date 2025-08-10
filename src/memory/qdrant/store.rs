//! Implements MemoryStore for Qdrant (semantic/embedding-based memory).

use crate::memory::traits::MemoryStore;
use crate::memory::types::{MemoryEntry, MemoryType};
use async_trait::async_trait;
use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::{json, Value};
use chrono::{DateTime, Utc, TimeZone};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct QdrantMemoryStore {
    pub client: Client,
    pub base_url: String,
    pub collection: String,
}

impl QdrantMemoryStore {
    pub fn new<S: Into<String>>(client: Client, base_url: S, collection: S) -> Self {
        Self {
            client,
            base_url: base_url.into(),
            collection: collection.into(),
        }
    }

    /// Ensures that a Qdrant collection exists with the correct vector size/config for Mira.
    /// Safe to call multiple times; will only create if missing.
    pub async fn ensure_collection(&self, name: &str) -> Result<()> {
        // 1. Check if collection exists (GET)
        let url = format!("{}/collections/{}", self.base_url, name);
        let resp = self.client.get(&url).send().await?;
        if resp.status().is_success() {
            // Collection already exists, nothing to do.
            return Ok(());
        }

        // 2. Try to create the collection (PUT /collections/{name})
        let create_url = format!("{}/collections/{}", self.base_url, name);
        let req_body = json!({
            "vectors": {
                "size": 3072,
                "distance": "Cosine"
            }
        });

        let resp = self.client
            .put(&create_url)
            .json(&req_body)
            .send()
            .await?;

        let status = resp.status();
        let err_body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 409 || err_body.contains("already exists") {
            Ok(())
        } else if status.is_success() {
            Ok(())
        } else {
            Err(anyhow!("Failed to create Qdrant collection: {}", err_body))
        }
    }

    /// Generate a unique numeric ID for Qdrant when entry.id is None.
    /// (Unix millis * 1000 + a tiny counter) — simple and collision-resistant enough here.
    fn gen_point_id() -> i64 {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let ms = now.as_millis() as i64;
        // add a low-bit jitter from nanos to reduce same-millis collisions
        let ns_mod = (now.as_nanos() % 1000) as i64;
        ms * 1000 + ns_mod
    }

    /// Role-scoped semantic search helper (returns vectors for near-dup checks).
    /// Use this in dedup paths; MemoryStore::semantic_search calls it with role=None.
    pub async fn semantic_search_scoped(
        &self,
        session_id: &str,
        role: Option<&str>,
        embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let url = format!(
            "{}/collections/{}/points/search",
            self.base_url, self.collection
        );

        // Build filter: must match session_id (and role if provided)
        let mut must = vec![ json!({
            "key": "session_id",
            "match": { "value": session_id }
        }) ];

        if let Some(r) = role {
            must.push(json!({
                "key": "role",
                "match": { "value": r }
            }));
        }

        let req_body = json!({
            "vector": embedding,
            "limit": k,
            "with_payload": true,
            "with_vectors": true,
            "filter": { "must": must }
        });

        let resp = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| anyhow!("Qdrant search error: {}", e))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Qdrant search failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }

        let resp_json: serde_json::Value = resp.json().await?;
        let mut results = Vec::new();

        if let Some(points) = resp_json.get("result").and_then(|r| r.as_array()) {
            for point in points {
                let payload = point.get("payload").cloned().unwrap_or(json!({}));

                // Vector is present because with_vectors=true
                let embedding = point.get("vector")
                    .and_then(|vec| vec.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|val| val.as_f64().map(|f| f as f32))
                            .collect::<Vec<f32>>()
                    });

                let entry = MemoryEntry {
                    id: point.get("id").and_then(|id| id.as_i64()),
                    session_id: payload.get("session_id").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    role: payload.get("role").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    content: payload.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                    timestamp: payload.get("timestamp").and_then(|v| v.as_i64()).map(millis_to_datetime).unwrap_or_else(|| Utc::now()),
                    embedding,
                    salience: payload.get("salience").and_then(|v| v.as_f64()).map(|f| f as f32),
                    tags: payload.get("tags").and_then(|v| v.as_array()).map(|arr| {
                        arr.iter().filter_map(|tag| tag.as_str().map(|s| s.to_string())).collect()
                    }),
                    summary: payload.get("summary").and_then(|v| v.as_str()).map(|s| s.to_string()),
                    memory_type: payload.get("memory_type").and_then(|v| v.as_str()).and_then(|s| match s {
                        "Feeling" => Some(MemoryType::Feeling),
                        "Fact" => Some(MemoryType::Fact),
                        "Joke" => Some(MemoryType::Joke),
                        "Promise" => Some(MemoryType::Promise),
                        "Event" => Some(MemoryType::Event),
                        _ => Some(MemoryType::Other),
                    }),
                    logprobs: payload.get("logprobs").cloned(),
                    moderation_flag: payload.get("moderation_flag").and_then(|v| v.as_bool()),
                    system_fingerprint: payload.get("system_fingerprint").and_then(|v| v.as_str()).map(|s| s.to_string()),
                };
                results.push(entry);
            }
        }

        Ok(results)
    }
}

// Helper for chrono timestamp conversion (no deprecation warnings)
fn millis_to_datetime(ms: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap())
}

#[async_trait]
impl MemoryStore for QdrantMemoryStore {
    async fn save(&self, entry: &MemoryEntry) -> Result<()> {
        // Qdrant "points" = memory entries.
        let url = format!(
            "{}/collections/{}/points",
            self.base_url, self.collection
        );

        let payload = json!({
            "session_id": entry.session_id,
            "role": entry.role,
            "content": entry.content,
            "timestamp": entry.timestamp.timestamp_millis(),
            "salience": entry.salience,
            "tags": entry.tags,
            "summary": entry.summary,
            "memory_type": entry.memory_type.as_ref().map(|mt| format!("{:?}", mt)),
            "logprobs": entry.logprobs,
            "moderation_flag": entry.moderation_flag,
            "system_fingerprint": entry.system_fingerprint,
        });

        let embedding = match &entry.embedding {
            Some(vec) => vec,
            None => return Err(anyhow!("No embedding for Qdrant memory entry.")),
        };

        // Ensure an ID is present; generate if missing
        let point_id = entry.id.unwrap_or_else(Self::gen_point_id);

        let point = json!({
            "id": point_id,
            "vector": embedding,
            "payload": payload,
        });

        // Each entry is a single-point upsert
        let req_body = json!({ "points": [ point ] });

        let resp = self
            .client
            .put(&url)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| anyhow!("Qdrant save error: {}", e))?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Qdrant save failed: {}",
                resp.text().await.unwrap_or_default()
            ));
        }

        Ok(())
    }

    async fn load_recent(
        &self,
        _session_id: &str,
        _n: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Qdrant doesn't do recency—leave empty.
        Ok(Vec::new())
    }

    async fn semantic_search(
        &self,
        session_id: &str,
        embedding: &[f32],
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        // Backward-compatible: session-scoped only
        self.semantic_search_scoped(session_id, None, embedding, k).await
    }

    async fn update_metadata(&self, _id: i64, _updated: &MemoryEntry) -> Result<()> {
        // (Stub) Qdrant supports payload update via /points/patch.
        Ok(())
    }

    async fn delete(&self, _id: i64) -> Result<()> {
        // (Stub) Qdrant supports deletion via /points/delete.
        Ok(())
    }
}
