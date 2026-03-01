// crates/mira-server/src/embeddings/ollama.rs
// Ollama embeddings via OpenAI-compatible /v1/embeddings endpoint

use crate::http::create_fast_client;
use crate::utils::truncate_at_boundary;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;
use tracing::debug;

/// Default Ollama embedding model
const DEFAULT_MODEL: &str = "nomic-embed-text";

/// Default dimensions for nomic-embed-text
const DEFAULT_DIMENSIONS: usize = 768;

/// Max characters to embed per text before truncation.
///
/// nomic-embed-text (and most local embedding models) have an 8192-token context window.
/// Dense code — especially C# with long PascalCase identifiers — tokenizes at roughly
/// 2 chars/token via BPE, so 8192 tokens ≈ 16384 chars. Using 8192 * 4 (32768 chars)
/// as the limit still results in 400 errors on dense code chunks.
///
/// 12000 chars (~6000 tokens at worst-case code density) provides a comfortable safety
/// margin below the 8192-token limit. Full chunk content is always stored in the database
/// — truncation only affects the text sent to the embedding API.
const MAX_TEXT_CHARS: usize = 12_000;

/// Max texts per batch request
pub(crate) const MAX_BATCH_SIZE: usize = 64;

/// Retry attempts
const RETRY_ATTEMPTS: usize = 1;

/// OpenAI-compatible embedding response types (shared with openai.rs)
#[derive(Debug, Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

/// Ollama embeddings client (OpenAI-compatible endpoint, no auth required)
pub struct OllamaEmbeddings {
    base_url: String,
    model: String,
    dimensions: usize,
    http_client: reqwest::Client,
}

impl OllamaEmbeddings {
    /// Create a new Ollama embeddings client.
    /// If `http_client` is `None`, creates one with a 60s timeout suitable for
    /// local embedding batches (longer than `create_fast_client`'s 30s).
    pub fn new(
        base_url: String,
        model: Option<String>,
        dimensions: Option<usize>,
        http_client: Option<reqwest::Client>,
    ) -> Self {
        let model = model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let dimensions = dimensions.unwrap_or(DEFAULT_DIMENSIONS);
        let base_url = base_url.trim_end_matches('/').to_string();

        let http_client = http_client.unwrap_or_else(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .connect_timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| create_fast_client())
        });

        Self {
            base_url,
            model,
            dimensions,
            http_client,
        }
    }

    /// Get embedding dimensions
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// Get model name
    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Embed a single text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let results = self.embed_texts(&[text.to_string()]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Empty embedding response from Ollama"))
    }

    /// Embed multiple texts in batch
    pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        if texts.len() <= MAX_BATCH_SIZE {
            return self.embed_texts(texts).await;
        }

        // Process in chunks
        let mut all_results = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(MAX_BATCH_SIZE) {
            all_results.extend(self.embed_texts(chunk).await?);
        }
        Ok(all_results)
    }

    /// Core embedding call via Ollama's OpenAI-compatible endpoint.
    ///
    /// On a 400 response (typically context overflow), retries with the truncation
    /// limit halved so token-dense inputs that exceed the model's context window
    /// get a second chance with shorter text.
    async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/v1/embeddings", self.base_url);
        let mut max_chars = MAX_TEXT_CHARS;
        let mut last_error = None;

        for attempt in 0..=RETRY_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            }

            let inputs: Vec<&str> = texts
                .iter()
                .map(|t| {
                    if t.len() > max_chars {
                        debug!(
                            "Truncating text from {} to {} chars for Ollama embedding",
                            t.len(),
                            max_chars
                        );
                        truncate_at_boundary(t, max_chars)
                    } else {
                        t.as_str()
                    }
                })
                .collect();

            let input_value = if inputs.len() == 1 {
                serde_json::Value::String(inputs[0].to_string())
            } else {
                serde_json::Value::Array(
                    inputs
                        .iter()
                        .map(|s| serde_json::Value::String(s.to_string()))
                        .collect(),
                )
            };

            let body = serde_json::json!({
                "input": input_value,
                "model": self.model,
            });

            match self
                .http_client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(response) => {
                    if response.status().is_success() {
                        let resp: EmbeddingResponse = response
                            .json()
                            .await
                            .context("Failed to parse Ollama embedding response")?;

                        let mut data = resp.data;
                        data.sort_by_key(|d| d.index);

                        let embeddings: Vec<Vec<f32>> =
                            data.into_iter().map(|d| d.embedding).collect();

                        // Auto-detect dimensions from first response
                        if let Some(first) = embeddings.first()
                            && first.len() != self.dimensions
                        {
                            debug!(
                                "Ollama embedding dimensions: expected {}, got {} — using actual",
                                self.dimensions,
                                first.len()
                            );
                        }

                        return Ok(embeddings);
                    }

                    let status = response.status();
                    let body_text = response.text().await.unwrap_or_default();

                    // On 400 (likely context overflow), halve truncation limit for retry
                    if status == reqwest::StatusCode::BAD_REQUEST && attempt < RETRY_ATTEMPTS {
                        let prev = max_chars;
                        max_chars /= 2;
                        debug!(
                            "Ollama returned 400 (context overflow), reducing truncation \
                             limit from {} to {} chars for retry",
                            prev, max_chars
                        );
                    }

                    last_error = Some(anyhow::anyhow!(
                        "Ollama embedding request failed ({}): {}",
                        status,
                        body_text
                    ));
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Ollama embedding request error: {}", e));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Ollama embedding failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn test_default_dimensions() {
        let client = OllamaEmbeddings::new("http://localhost:11434".to_string(), None, None, None);
        assert_eq!(client.dimensions(), DEFAULT_DIMENSIONS);
        assert_eq!(client.model_name(), DEFAULT_MODEL);
    }

    #[test]
    fn test_custom_model_and_dimensions() {
        let client = OllamaEmbeddings::new(
            "http://localhost:11434".to_string(),
            Some("mxbai-embed-large".to_string()),
            Some(1024),
            None,
        );
        assert_eq!(client.dimensions(), 1024);
        assert_eq!(client.model_name(), "mxbai-embed-large");
    }

    #[test]
    fn test_base_url_normalization() {
        let client = OllamaEmbeddings::new("http://localhost:11434/".to_string(), None, None, None);
        assert_eq!(client.base_url, "http://localhost:11434");
    }

    /// Mock server that returns 400 on the first request and 200 with a valid
    /// embedding on the second, letting us verify the retry-with-halved-truncation
    /// path works end-to-end.
    #[tokio::test]
    async fn test_retry_halves_truncation_on_400() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let call_count = Arc::new(AtomicUsize::new(0));

        // Spawn mock server
        let counter = call_count.clone();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let n = stream.read(&mut buf).await.unwrap();
                let request = String::from_utf8_lossy(&buf[..n]);

                let attempt = counter.fetch_add(1, Ordering::SeqCst);

                // Extract the JSON body after the blank line
                let body_start = request.find("\r\n\r\n").unwrap() + 4;
                let body_json = &request[body_start..];

                if attempt == 0 {
                    // Verify first request has input near MAX_TEXT_CHARS
                    let parsed: serde_json::Value = serde_json::from_str(body_json).unwrap();
                    let input_len = parsed["input"].as_str().unwrap().len();
                    assert!(
                        input_len > MAX_TEXT_CHARS / 2,
                        "First attempt input should be near MAX_TEXT_CHARS, got {input_len}"
                    );

                    let resp = "HTTP/1.1 400 Bad Request\r\n\
                                Content-Type: application/json\r\n\
                                Content-Length: 52\r\n\r\n\
                                {\"error\":\"input length exceeds context length 8192\"}";
                    stream.write_all(resp.as_bytes()).await.unwrap();
                } else {
                    // Verify retry has shorter input (halved limit)
                    let parsed: serde_json::Value = serde_json::from_str(body_json).unwrap();
                    let input_len = parsed["input"].as_str().unwrap().len();
                    assert!(
                        input_len <= MAX_TEXT_CHARS / 2,
                        "Retry input should be <= {}, got {input_len}",
                        MAX_TEXT_CHARS / 2
                    );

                    let embedding = vec![0.1_f32; DEFAULT_DIMENSIONS];
                    let body = serde_json::json!({
                        "data": [{"embedding": embedding, "index": 0}],
                        "model": DEFAULT_MODEL
                    });
                    let body_str = serde_json::to_string(&body).unwrap();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: application/json\r\n\
                         Content-Length: {}\r\n\r\n\
                         {}",
                        body_str.len(),
                        body_str
                    );
                    stream.write_all(resp.as_bytes()).await.unwrap();
                }
                stream.flush().await.unwrap();
            }
        });

        let client = OllamaEmbeddings::new(
            format!("http://127.0.0.1:{port}"),
            Some(DEFAULT_MODEL.to_string()),
            Some(DEFAULT_DIMENSIONS),
            None,
        );

        // Input longer than MAX_TEXT_CHARS to trigger truncation
        let long_input = "x".repeat(MAX_TEXT_CHARS + 5000);
        let result = client.embed_texts(&[long_input]).await;

        assert!(
            result.is_ok(),
            "Should succeed on retry: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap()[0].len(), DEFAULT_DIMENSIONS);
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            2,
            "Should have made exactly 2 attempts"
        );

        server.await.unwrap();
    }
}
