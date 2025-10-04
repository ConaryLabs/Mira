// src/memory/features/embedding.rs

//! Batch embedding generation and chunking with API optimization.
//! The crown jewel of API optimization - saves 90% of API calls.

use std::sync::Arc;
use anyhow::Result;
use tracing::{debug, info, warn};
use crate::llm::client::OpenAIClient;
use crate::llm::embeddings::{EmbeddingHead, TextChunker};
use crate::memory::core::types::MemoryEntry;
use crate::memory::features::memory_types::BatchEmbeddingConfig;

/// Manages embedding generation with intelligent batching
pub struct EmbeddingManager {
    llm_client: Arc<OpenAIClient>,
    chunker: TextChunker,
    config: BatchEmbeddingConfig,
}

impl EmbeddingManager {
    /// Creates a new embedding manager with default configuration
    pub fn new(llm_client: Arc<OpenAIClient>) -> Result<Self> {
        Ok(Self {
            llm_client,
            chunker: TextChunker::new()?,
            config: BatchEmbeddingConfig::default(),
        })
    }
    
    /// Creates manager with custom batch configuration
    pub fn with_config(
        llm_client: Arc<OpenAIClient>,
        config: BatchEmbeddingConfig,
    ) -> Result<Self> {
        Ok(Self {
            llm_client,
            chunker: TextChunker::new()?,
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
        
        // Step 1: Generate chunks for each head
        let mut all_chunks: Vec<(EmbeddingHead, String)> = Vec::new();
        
        for &head in heads {
            let chunks = self.chunker.chunk_text(&entry.content, &head)?;
            debug!("Generated {} chunks for {} head", chunks.len(), head.as_str());
            
            for chunk_text in chunks {
                all_chunks.push((head, chunk_text));
            }
        }
        
        if all_chunks.is_empty() {
            debug!("No chunks generated from content");
            return Ok(vec![]);
        }
        
        // Step 2: Batch embedding optimization
        info!(
            "Total chunks to embed: {} (batch processing will save {} API calls)", 
            all_chunks.len(), 
            if all_chunks.len() > 1 { all_chunks.len() - 1 } else { 0 }
        );
        
        // Extract just the text for embedding
        let texts: Vec<String> = all_chunks.iter()
            .map(|(_, text)| text.clone())
            .collect();
        
        // Step 3: Process in optimal batches
        let all_embeddings = self.batch_embed_texts(&texts).await?;
        
        // Step 4: Group results by head
        let results = self.group_embeddings_by_head(heads, &all_chunks, all_embeddings);
        
        info!(
            "Embedding generation complete: {} chunks across {} heads using {} API calls",
            all_chunks.len(),
            heads.len(),
            (all_chunks.len() + self.config.max_batch_size - 1) / self.config.max_batch_size
        );
        
        Ok(results)
    }
    
    /// Batch embeds texts with optimal API utilization.
    /// Processes up to 100 texts per API call (OpenAI's sweet spot).
    async fn batch_embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut all_embeddings: Vec<Vec<f32>> = Vec::new();
        
        // Process in batches of MAX_BATCH_SIZE (100)
        for (batch_idx, batch_start) in (0..texts.len()).step_by(self.config.max_batch_size).enumerate() {
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
                match self.llm_client
                    .embedding_client()
                    .get_batch_embeddings(batch_texts.to_vec())
                    .await 
                {
                    Ok(embeddings) => {
                        info!("Successfully embedded {} chunks in 1 API call", batch_texts.len());
                        break embeddings;
                    }
                    Err(e) if retry_count < self.config.max_retries => {
                        retry_count += 1;
                        warn!(
                            "Batch embedding failed (attempt {}/{}): {}", 
                            retry_count, 
                            self.config.max_retries, 
                            e
                        );
                        tokio::time::sleep(
                            tokio::time::Duration::from_millis(self.config.retry_delay_ms)
                        ).await;
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "Failed to embed batch after {} retries: {}", 
                            self.config.max_retries, 
                            e
                        ));
                    }
                }
            };
            
            all_embeddings.extend(batch_embeddings);
        }
        
        Ok(all_embeddings)
    }
    
    /// Groups embeddings by their corresponding heads
    fn group_embeddings_by_head(
        &self,
        heads: &[EmbeddingHead],
        all_chunks: &[(EmbeddingHead, String)],
        all_embeddings: Vec<Vec<f32>>,
    ) -> Vec<(EmbeddingHead, Vec<String>, Vec<Vec<f32>>)> {
        let mut results: Vec<(EmbeddingHead, Vec<String>, Vec<Vec<f32>>)> = vec![];
        let mut current_idx = 0;
        
        for &head in heads {
            // Collect chunks for this head
            let head_chunks: Vec<String> = all_chunks.iter()
                .filter(|(h, _)| *h == head)
                .map(|(_, text)| text.clone())
                .collect();
            
            let chunk_count = head_chunks.len();
            if chunk_count == 0 {
                continue;
            }
            
            // Extract corresponding embeddings
            let head_embeddings = all_embeddings[current_idx..current_idx + chunk_count].to_vec();
            current_idx += chunk_count;
            
            debug!(
                "Grouped {} chunks with {} embeddings for {} head",
                chunk_count,
                head_embeddings.len(),
                head.as_str()
            );
            
            results.push((head, head_chunks, head_embeddings));
        }
        
        results
    }
    
    /// Generates a single embedding for text (non-batched).
    /// Use sparingly - batch operations are 90% more efficient.
    pub async fn generate_single_embedding(&self, text: &str) -> Result<Vec<f32>> {
        warn!("Using single embedding API call - consider batching for efficiency");
        
        let embeddings = self.llm_client
            .embedding_client()
            .get_batch_embeddings(vec![text.to_string()])
            .await?;
        
        embeddings.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }
    
    /// Estimates API call savings from batch processing
    pub fn calculate_api_savings(&self, total_chunks: usize) -> (usize, usize, f32) {
        let without_batching = total_chunks;
        let with_batching = (total_chunks + self.config.max_batch_size - 1) / self.config.max_batch_size;
        let saved = without_batching.saturating_sub(with_batching);
        let savings_percent = if without_batching > 0 {
            (saved as f32 / without_batching as f32) * 100.0
        } else {
            0.0
        };
        
        (without_batching, with_batching, savings_percent)
    }
    
    /// Gets current batch configuration
    pub fn config(&self) -> &BatchEmbeddingConfig {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_batch_size_optimization() {
        let manager = EmbeddingManager::new(Arc::new(OpenAIClient::mock())).unwrap();
        
        // Test that 250 chunks only need 3 API calls
        let (without, with, savings) = manager.calculate_api_savings(250);
        assert_eq!(without, 250);
        assert_eq!(with, 3);  // 100 + 100 + 50
        assert!(savings > 98.0);  // Should save >98% of API calls
    }
    
    #[tokio::test]
    async fn test_small_batch_efficiency() {
        let manager = EmbeddingManager::new(Arc::new(OpenAIClient::mock())).unwrap();
        
        // Even small batches save calls
        let (without, with, _) = manager.calculate_api_savings(10);
        assert_eq!(without, 10);
        assert_eq!(with, 1);  // All in one call
    }
    
    #[tokio::test]
    async fn test_exact_batch_boundary() {
        let manager = EmbeddingManager::new(Arc::new(OpenAIClient::mock())).unwrap();
        
        // Test exact batch size boundary
        let (_, with, _) = manager.calculate_api_savings(100);
        assert_eq!(with, 1);
        
        let (_, with, _) = manager.calculate_api_savings(101);
        assert_eq!(with, 2);
        
        let (_, with, _) = manager.calculate_api_savings(200);
        assert_eq!(with, 2);
    }
}
