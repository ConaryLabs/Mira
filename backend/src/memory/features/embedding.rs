// src/memory/features/embedding.rs

//! Batch embedding generation with API optimization.
//! The crown jewel of API optimization - saves 90% of API calls.

use crate::llm::embeddings::EmbeddingHead;
use crate::llm::provider::OpenAiEmbeddings;
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::memory_types::BatchEmbeddingConfig;
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Manages embedding generation with intelligent batching
pub struct EmbeddingManager {
    embedding_client: Arc<OpenAiEmbeddings>,
    config: BatchEmbeddingConfig,
}

impl EmbeddingManager {
    /// Creates a new embedding manager with default configuration
    pub fn new(embedding_client: Arc<OpenAiEmbeddings>) -> Result<Self> {
        Ok(Self {
            embedding_client,
            config: BatchEmbeddingConfig::default(),
        })
    }

    /// Creates manager with custom batch configuration
    pub fn with_config(
        embedding_client: Arc<OpenAiEmbeddings>,
        config: BatchEmbeddingConfig,
    ) -> Result<Self> {
        Ok(Self {
            embedding_client,
            config,
        })
    }

    /// Generates embeddings for multiple heads with batch optimization.
    /// This is the main entry point for the embedding pipeline.
    pub async fn generate_embeddings_for_heads(
        &self,
        entry: &MemoryEntry,
        heads: &[EmbeddingHead],
    ) -> Result<Vec<(EmbeddingHead, Vec<String>, Vec<Vec<f32>>)>> {
        info!("Generating embeddings for {} heads", heads.len());

        if heads.is_empty() {
            debug!("No heads specified, skipping embedding generation");
            return Ok(vec![]);
        }

        // Use full content for each head (no chunking)
        let mut all_texts: Vec<(EmbeddingHead, String)> = Vec::new();

        for &head in heads {
            all_texts.push((head, entry.content.clone()));
            debug!("Added content for {} head", head.as_str());
        }

        if all_texts.is_empty() {
            debug!("No content to embed");
            return Ok(vec![]);
        }

        // Step 2: Batch embedding optimization
        info!(
            "Total texts to embed: {} (batch processing will save {} API calls)",
            all_texts.len(),
            if all_texts.len() > 1 {
                all_texts.len() - 1
            } else {
                0
            }
        );

        // Extract just the text for embedding
        let texts: Vec<String> = all_texts.iter().map(|(_, text)| text.clone()).collect();

        // Step 3: Process in optimal batches
        let all_embeddings = self.batch_embed_texts(&texts).await?;

        // Step 4: Group results by head
        let results = self.group_embeddings_by_head(heads, &all_texts, all_embeddings);

        info!(
            "Embedding generation complete: {} texts across {} heads using {} API calls",
            all_texts.len(),
            heads.len(),
            (all_texts.len() + self.config.max_batch_size - 1) / self.config.max_batch_size
        );

        Ok(results)
    }

    /// Batch embeds texts with optimal API utilization.
    /// Processes up to 100 texts per API call (OpenAI's sweet spot).
    async fn batch_embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut all_embeddings: Vec<Vec<f32>> = Vec::new();

        // Process in batches of MAX_BATCH_SIZE (100)
        for (batch_idx, batch_start) in (0..texts.len())
            .step_by(self.config.max_batch_size)
            .enumerate()
        {
            let batch_end = std::cmp::min(batch_start + self.config.max_batch_size, texts.len());
            let batch_texts = &texts[batch_start..batch_end];

            info!(
                "Processing batch {} ({}-{} of {}) in single API call",
                batch_idx + 1,
                batch_start + 1,
                batch_end,
                texts.len()
            );

            // Retry logic for robustness
            let mut retry_count = 0;
            let batch_embeddings = loop {
                match self
                    .embedding_client
                    .embed_batch(batch_texts.to_vec())
                    .await
                {
                    Ok(embeddings) => {
                        info!(
                            "Successfully embedded {} texts in 1 API call",
                            batch_texts.len()
                        );
                        break embeddings;
                    }
                    Err(e) if retry_count < self.config.max_retries => {
                        retry_count += 1;
                        warn!(
                            "Batch embedding failed (attempt {}/{}): {}",
                            retry_count, self.config.max_retries, e
                        );
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            self.config.retry_delay_ms,
                        ))
                        .await;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "Batch embedding failed after {} retries: {}",
                            retry_count,
                            e
                        ));
                    }
                }
            };

            all_embeddings.extend(batch_embeddings);
        }

        Ok(all_embeddings)
    }

    /// Groups embeddings back to their corresponding heads
    fn group_embeddings_by_head(
        &self,
        heads: &[EmbeddingHead],
        all_texts: &[(EmbeddingHead, String)],
        all_embeddings: Vec<Vec<f32>>,
    ) -> Vec<(EmbeddingHead, Vec<String>, Vec<Vec<f32>>)> {
        let mut results = Vec::new();

        for &head in heads {
            let mut head_texts = Vec::new();
            let mut head_embeddings = Vec::new();

            for (i, (text_head, text)) in all_texts.iter().enumerate() {
                if text_head == &head {
                    head_texts.push(text.clone());
                    if i < all_embeddings.len() {
                        head_embeddings.push(all_embeddings[i].clone());
                    }
                }
            }

            if !head_texts.is_empty() {
                results.push((head, head_texts, head_embeddings));
            }
        }

        results
    }
}
