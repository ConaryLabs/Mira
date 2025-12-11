// backend/src/memory/storage/qdrant/multi_store.rs

//! Multi-head Qdrant storage for embeddings
//!
//! Manages 3 collections for different embedding types:
//! - code: Semantic nodes, code elements, design patterns, AST analysis
//! - conversation: Messages, summaries, facts, user patterns, documents
//! - git: Commits, co-change patterns, historical fixes, blame analysis

use anyhow::{Context, Result};
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, Distance, Filter, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder, VectorParamsBuilder, DeletePointsBuilder,
    PointId, Value as QdrantValue,
};
use qdrant_client::Qdrant;
use std::collections::HashMap;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::llm::EmbeddingHead;
use crate::memory::core::types::MemoryEntry;

/// Embedding dimensions for text-embedding-3-large
const EMBEDDING_DIM: u64 = 3072;

/// Multi-head Qdrant store supporting different embedding collections
pub struct QdrantMultiStore {
    client: Qdrant,
    prefix: String,
}

impl QdrantMultiStore {
    /// Create a new multi-store with the given Qdrant URL and collection prefix
    pub async fn new(url: &str, prefix: &str) -> Result<Self> {
        // Skip compatibility check to allow minor version mismatches
        // TODO: Update Qdrant server to match client version (1.15.x)
        let client = Qdrant::from_url(url)
            .skip_compatibility_check()
            .build()
            .context("Failed to connect to Qdrant")?;

        let store = Self {
            client,
            prefix: prefix.to_string(),
        };

        // Ensure collections exist
        store.ensure_collections().await?;

        Ok(store)
    }

    /// Get the collection name for a given embedding head
    fn collection_name(&self, head: EmbeddingHead) -> String {
        format!("{}_{}", self.prefix, head.as_str())
    }

    /// Ensure all required collections exist (3 collections)
    async fn ensure_collections(&self) -> Result<()> {
        let heads = [
            EmbeddingHead::Code,
            EmbeddingHead::Conversation,
            EmbeddingHead::Git,
        ];

        for head in heads {
            let collection = self.collection_name(head);
            self.ensure_collection(&collection).await?;
        }

        Ok(())
    }

    /// Ensure a single collection exists
    async fn ensure_collection(&self, collection: &str) -> Result<()> {
        let exists = self.client.collection_exists(collection).await?;

        if !exists {
            info!("Creating Qdrant collection: {}", collection);
            match self.client
                .create_collection(
                    CreateCollectionBuilder::new(collection)
                        .vectors_config(VectorParamsBuilder::new(EMBEDDING_DIM, Distance::Cosine)),
                )
                .await
            {
                Ok(_) => {},
                Err(e) => {
                    // Handle race condition: if collection was created by another process
                    // between our exists check and create call, that's fine
                    let error_msg = e.to_string();
                    if error_msg.contains("already exists") {
                        debug!("Collection {} already exists (created by another process)", collection);
                    } else {
                        return Err(e).context(format!("Failed to create collection: {}", collection));
                    }
                }
            }
        }

        Ok(())
    }

    /// Save a memory entry to the appropriate collection
    pub async fn save(&self, head: EmbeddingHead, entry: &MemoryEntry) -> Result<String> {
        let collection = self.collection_name(head);

        let embedding = entry
            .embedding
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Entry has no embedding"))?;

        // Generate point ID from entry ID or create new UUID
        let point_id = entry
            .id
            .map(|id| id.to_string())
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        // Build payload with metadata
        let mut payload: HashMap<String, QdrantValue> = HashMap::new();
        payload.insert("session_id".to_string(), entry.session_id.clone().into());
        payload.insert("role".to_string(), entry.role.clone().into());
        payload.insert("content".to_string(), entry.content.clone().into());
        payload.insert(
            "timestamp".to_string(),
            entry.timestamp.timestamp().into(),
        );

        if let Some(id) = entry.id {
            payload.insert("entry_id".to_string(), id.into());
        }

        if let Some(ref tags) = entry.tags {
            let tags_str = tags.join(",");
            payload.insert("tags".to_string(), tags_str.into());
        }

        if let Some(salience) = entry.salience {
            payload.insert("salience".to_string(), (salience as f64).into());
        }

        if let Some(ref mood) = entry.mood {
            payload.insert("mood".to_string(), mood.clone().into());
        }

        if let Some(ref intent) = entry.intent {
            payload.insert("intent".to_string(), intent.clone().into());
        }

        if let Some(ref topics) = entry.topics {
            let topics_str = topics.join(",");
            payload.insert("topics".to_string(), topics_str.into());
        }

        if let Some(ref summary) = entry.summary {
            payload.insert("summary".to_string(), summary.clone().into());
        }

        if let Some(contains_code) = entry.contains_code {
            payload.insert("contains_code".to_string(), contains_code.into());
        }

        if let Some(ref programming_lang) = entry.programming_lang {
            payload.insert("programming_lang".to_string(), programming_lang.clone().into());
        }

        // Parse point_id as u64 or use hash
        let numeric_id: u64 = point_id
            .parse()
            .unwrap_or_else(|_| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                point_id.hash(&mut hasher);
                hasher.finish()
            });

        // Use PointStruct::new for proper vector construction
        let point = PointStruct::new(
            numeric_id,
            embedding.clone(),
            payload,
        );

        self.client
            .upsert_points(UpsertPointsBuilder::new(&collection, vec![point]).wait(true))
            .await
            .context("Failed to upsert point to Qdrant")?;

        debug!(
            "Saved entry to Qdrant collection {} with id {}",
            collection, point_id
        );

        Ok(point_id)
    }

    /// Search a specific collection for similar entries
    pub async fn search(
        &self,
        head: EmbeddingHead,
        session_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let collection = self.collection_name(head);

        let filter = Filter::must([Condition::matches("session_id", session_id.to_string())]);

        let results = self
            .client
            .search_points(
                SearchPointsBuilder::new(&collection, embedding.to_vec(), limit as u64)
                    .filter(filter)
                    .with_payload(true),
            )
            .await
            .context("Failed to search Qdrant")?;

        let entries = results
            .result
            .into_iter()
            .filter_map(|point| self.point_to_entry(point))
            .collect();

        Ok(entries)
    }

    /// Search all collections for similar entries (parallelized for performance)
    /// Returns results grouped by head
    pub async fn search_all(
        &self,
        session_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<(EmbeddingHead, Vec<MemoryEntry>)>> {
        // Run all 3 collection searches in parallel using tokio::join!
        let (code_result, conv_result, git_result) = tokio::join!(
            self.search(EmbeddingHead::Code, session_id, embedding, limit),
            self.search(EmbeddingHead::Conversation, session_id, embedding, limit),
            self.search(EmbeddingHead::Git, session_id, embedding, limit)
        );

        let mut results = Vec::with_capacity(3);

        // Process results, logging errors but continuing
        match code_result {
            Ok(entries) if !entries.is_empty() => results.push((EmbeddingHead::Code, entries)),
            Ok(_) => {}
            Err(e) => warn!("Failed to search code collection: {}", e),
        }

        match conv_result {
            Ok(entries) if !entries.is_empty() => results.push((EmbeddingHead::Conversation, entries)),
            Ok(_) => {}
            Err(e) => warn!("Failed to search conversation collection: {}", e),
        }

        match git_result {
            Ok(entries) if !entries.is_empty() => results.push((EmbeddingHead::Git, entries)),
            Ok(_) => {}
            Err(e) => warn!("Failed to search git collection: {}", e),
        }

        Ok(results)
    }

    /// Delete points matching a filter
    pub async fn delete_by_filter(
        &self,
        head: EmbeddingHead,
        filter: Filter,
    ) -> Result<u64> {
        let collection = self.collection_name(head);

        self.client
            .delete_points(
                DeletePointsBuilder::new(&collection)
                    .points(filter)
                    .wait(true),
            )
            .await
            .context("Failed to delete points from Qdrant")?;

        // Qdrant doesn't return count of deleted points, so we return 0
        debug!("Deleted points from {} collection", head.as_str());

        Ok(0)
    }

    /// Delete points by session ID
    pub async fn delete_by_session(
        &self,
        head: EmbeddingHead,
        session_id: &str,
    ) -> Result<u64> {
        let filter = Filter::must([Condition::matches("session_id", session_id.to_string())]);
        self.delete_by_filter(head, filter).await
    }

    /// Delete points by tag
    pub async fn delete_by_tag(
        &self,
        head: EmbeddingHead,
        tag: &str,
    ) -> Result<u64> {
        // Tags are stored as comma-separated string, so we use contains match
        let filter = Filter::must([Condition::matches("tags", tag.to_string())]);
        self.delete_by_filter(head, filter).await
    }

    /// Delete a point by entry_id (i64 message/element ID)
    /// This is the primary delete method used by cleanup tasks
    pub async fn delete(&self, head: EmbeddingHead, entry_id: i64) -> Result<()> {
        self.delete_point(head, entry_id as u64).await
    }

    /// Delete a specific point by ID
    pub async fn delete_point(
        &self,
        head: EmbeddingHead,
        point_id: u64,
    ) -> Result<()> {
        let collection = self.collection_name(head);

        self.client
            .delete_points(
                DeletePointsBuilder::new(&collection)
                    .points(vec![PointId::from(point_id)]),
            )
            .await
            .context("Failed to delete point from Qdrant")?;

        debug!("Deleted point {} from {} collection", point_id, head.as_str());

        Ok(())
    }

    /// Delete a point from all collections by entry_id
    pub async fn delete_from_all(&self, entry_id: i64) -> Result<()> {
        let heads = self.get_enabled_heads();
        for head in heads {
            // Ignore errors - point may not exist in all collections
            let _ = self.delete(head, entry_id).await;
        }
        Ok(())
    }

    /// Convert a Qdrant search result point to a MemoryEntry
    fn point_to_entry(&self, point: qdrant_client::qdrant::ScoredPoint) -> Option<MemoryEntry> {
        let payload = point.payload;

        let session_id = payload.get("session_id")?.as_str()?.to_string();
        let role = payload.get("role")?.as_str()?.to_string();
        let content = payload.get("content")?.as_str()?.to_string();
        let timestamp_secs = payload.get("timestamp")?.as_integer()?;

        let timestamp = chrono::DateTime::from_timestamp(timestamp_secs, 0)?
            .with_timezone(&chrono::Utc);

        let id = payload
            .get("entry_id")
            .and_then(|v| v.as_integer())
            .map(|i| i as i64);

        let tags = payload
            .get("tags")
            .and_then(|v| v.as_str())
            .map(|s| s.split(',').map(String::from).collect());

        let salience = payload
            .get("salience")
            .and_then(|v| v.as_double())
            .map(|f| f as f32);

        let mood = payload.get("mood").and_then(|v| v.as_str()).map(String::from);

        let intent = payload.get("intent").and_then(|v| v.as_str()).map(String::from);

        let topics = payload
            .get("topics")
            .and_then(|v| v.as_str())
            .map(|s| s.split(',').map(String::from).collect());

        let summary = payload.get("summary").and_then(|v| v.as_str()).map(String::from);

        let contains_code = payload.get("contains_code").and_then(|v| v.as_bool());

        let programming_lang = payload
            .get("programming_lang")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Get embedding from vector if available
        let embedding = point.vectors.and_then(|v| {
            match v.vectors_options {
                Some(qdrant_client::qdrant::vectors_output::VectorsOptions::Vector(vec)) => {
                    match vec.into_vector() {
                        qdrant_client::qdrant::vector_output::Vector::Dense(dense) => Some(dense.data),
                        _ => None,
                    }
                }
                _ => None,
            }
        });

        Some(MemoryEntry {
            id,
            session_id,
            response_id: None,
            parent_id: None,
            role,
            content,
            timestamp,
            tags,
            mood,
            intensity: None,
            salience,
            original_salience: None,
            intent,
            topics,
            summary,
            relationship_impact: None,
            contains_code,
            language: None,
            programming_lang,
            analyzed_at: None,
            analysis_version: None,
            routed_to_heads: None,
            last_recalled: None,
            recall_count: None,
            contains_error: None,
            error_type: None,
            error_severity: None,
            error_file: None,
            model_version: None,
            prompt_tokens: None,
            completion_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            latency_ms: None,
            generation_time_ms: None,
            finish_reason: None,
            tool_calls: None,
            temperature: None,
            max_tokens: None,
            embedding,
            embedding_heads: None,
            qdrant_point_ids: None,
        })
    }

    /// Get list of enabled embedding heads (3 collections)
    pub fn get_enabled_heads(&self) -> Vec<EmbeddingHead> {
        vec![
            EmbeddingHead::Code,
            EmbeddingHead::Conversation,
            EmbeddingHead::Git,
        ]
    }

    /// Check if Qdrant is connected and responsive
    pub fn is_connected(&self) -> bool {
        // The client is connected if it was successfully built
        // For a more thorough check, we'd need an async health_check method
        true
    }

    /// Async health check - verifies connection by checking if collections exist
    pub async fn health_check(&self) -> Result<bool> {
        let collection = self.collection_name(EmbeddingHead::Conversation);
        match self.client.collection_exists(&collection).await {
            Ok(_) => Ok(true),
            Err(e) => {
                warn!("Qdrant health check failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Scroll all points in a collection, returning their IDs
    pub async fn scroll_all_points(&self, head: EmbeddingHead) -> Result<Vec<String>> {
        use qdrant_client::qdrant::ScrollPointsBuilder;

        let collection = self.collection_name(head);
        let mut all_ids = Vec::new();
        let mut offset: Option<qdrant_client::qdrant::PointId> = None;
        let limit = 100u32;

        loop {
            let mut builder = ScrollPointsBuilder::new(&collection)
                .limit(limit)
                .with_payload(false)
                .with_vectors(false);

            if let Some(ref off) = offset {
                builder = builder.offset(off.clone());
            }

            let response = self.client.scroll(builder).await?;

            if response.result.is_empty() {
                break;
            }

            for point in &response.result {
                if let Some(ref id) = point.id {
                    match &id.point_id_options {
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => {
                            all_ids.push(n.to_string());
                        }
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => {
                            all_ids.push(u.clone());
                        }
                        None => {}
                    }
                }
            }

            // Get the last point's ID for pagination
            offset = response.result.last().and_then(|p| p.id.clone());

            // If we got fewer results than the limit, we're done
            if response.result.len() < limit as usize {
                break;
            }
        }

        Ok(all_ids)
    }
}
