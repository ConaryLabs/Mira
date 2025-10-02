// src/llm/embeddings.rs
// Embeddings functionality using text-embedding-3-large with multi-head support

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use tokenizers::Tokenizer;

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    pub chunk_size: usize,
    pub overlap: usize,
}

impl ChunkConfig {
    pub fn for_head(head: &EmbeddingHead) -> Self {
        match head {
            EmbeddingHead::Semantic => Self { chunk_size: 500, overlap: 100 },
            EmbeddingHead::Code => Self { chunk_size: 1000, overlap: 200 },
            EmbeddingHead::Summary => Self { chunk_size: 2000, overlap: 0 },
            EmbeddingHead::Documents => Self { chunk_size: 1000, overlap: 200 },
        }
    }
}

pub struct EmbeddingConfig {
    pub model: String,
    pub dimensions: usize,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "text-embedding-3-large".to_string(),
            dimensions: 3072,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingResponse {
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingData {
    pub embedding: Vec<f32>,
    pub index: usize,
}

#[derive(Debug, Deserialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: usize,
    pub total_tokens: usize,
}

pub mod utils {
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    pub fn normalize_embedding(embedding: &mut [f32]) {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in embedding.iter_mut() {
                *value /= norm;
            }
        }
    }

    pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return f32::MAX;
        }

        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f32>()
            .sqrt()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmbeddingHead {
    Semantic,
    Code,
    Summary,
    Documents,
}

impl EmbeddingHead {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmbeddingHead::Semantic => "semantic",
            EmbeddingHead::Code => "code",
            EmbeddingHead::Summary => "summary",
            EmbeddingHead::Documents => "documents",
        }
    }
    
    pub fn all() -> Vec<EmbeddingHead> {
        vec![
            EmbeddingHead::Semantic,
            EmbeddingHead::Code,
            EmbeddingHead::Summary,
            EmbeddingHead::Documents,
        ]
    }
    
    pub fn should_route(&self, content: &str, role: &str) -> bool {
        match self {
            EmbeddingHead::Semantic => {
                role == "user" || role == "assistant"
            },
            EmbeddingHead::Code => {
                contains_code_indicators(content)
            },
            EmbeddingHead::Summary => {
                role == "system" && content.contains("Summary:")
            },
            EmbeddingHead::Documents => {
                role == "document"
            },
        }
    }
}

impl fmt::Display for EmbeddingHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for EmbeddingHead {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "semantic" => Ok(EmbeddingHead::Semantic),
            "code" => Ok(EmbeddingHead::Code),
            "summary" => Ok(EmbeddingHead::Summary),
            "documents" => Ok(EmbeddingHead::Documents),
            _ => Err(anyhow::anyhow!("Unknown embedding head: {}", s)),
        }
    }
}

pub struct TextChunker {
    tokenizer: Tokenizer,
}

impl TextChunker {
    pub fn new() -> Result<Self> {
        let tokenizer = Tokenizer::from_bytes(include_bytes!("../../tokenizer.json"))
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(Self { tokenizer })
    }

    pub fn chunk_text(&self, text: &str, head: &EmbeddingHead) -> Result<Vec<String>> {
        let config = ChunkConfig::for_head(head);
        let chunk_size = config.chunk_size;
        let chunk_overlap = config.overlap;

        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let token_ids = encoding.get_ids();

        if token_ids.len() <= chunk_size {
            return Ok(vec![text.to_string()]);
        }

        let mut chunks = Vec::new();
        let mut start_idx = 0;
        let step = chunk_size.saturating_sub(chunk_overlap);

        while start_idx < token_ids.len() {
            let end_idx = std::cmp::min(start_idx + chunk_size, token_ids.len());
            let chunk_ids = &token_ids[start_idx..end_idx];

            let chunk_text = self
                .tokenizer
                .decode(chunk_ids, true)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            chunks.push(chunk_text);

            if step == 0 {
                break;
            }
            if end_idx == token_ids.len() {
                break;
            }
            start_idx += step;
        }

        Ok(chunks)
    }
}

fn contains_code_indicators(content: &str) -> bool {
    let code_patterns = [
        "```",           
        "fn ",           
        "def ",          
        "function ",     
        "class ",        
        "import ",       
        "const ",        
        "let ",          
        "var ",          
        "return ",       
        "if (",          
        "for (",         
        "while (",       
        "SELECT ",       
        "CREATE TABLE",  
        "pub struct",    
        "async fn",      
        "#include",      
    ];
    
    let lower_content = content.to_lowercase();
    code_patterns.iter().any(|pattern| lower_content.contains(&pattern.to_lowercase()))
}
