// src/memory/features/document_processing/chunker.rs
//! Intelligent document chunking with semantic boundary detection

use anyhow::Result;
use regex::Regex;

/// Chunking strategy for document splitting
#[derive(Debug, Clone)]
pub enum ChunkingStrategy {
    /// Fixed size chunks with overlap
    FixedSize { 
        chunk_size: usize, 
        overlap: usize 
    },
    /// Semantic chunking based on paragraph and sentence boundaries
    Semantic {
        target_size: usize,
        max_size: usize,
        min_size: usize,
    },
    /// Page-based chunking (for PDFs)
    PageBased,
}

impl Default for ChunkingStrategy {
    fn default() -> Self {
        ChunkingStrategy::Semantic {
            target_size: 1000,  // Target ~1000 chars per chunk
            max_size: 1500,     // Never exceed 1500 chars
            min_size: 200,      // Don't create chunks smaller than 200 chars
        }
    }
}

/// Document chunker that splits text into manageable pieces
pub struct DocumentChunker {
    strategy: ChunkingStrategy,
    sentence_regex: Regex,
    paragraph_regex: Regex,
}

impl DocumentChunker {
    /// Create a new document chunker with default semantic strategy
    pub fn new() -> Self {
        Self::with_strategy(ChunkingStrategy::default())
    }
    
    /// Create a chunker with a specific strategy
    pub fn with_strategy(strategy: ChunkingStrategy) -> Self {
        Self {
            strategy,
            // Regex for sentence boundaries
            sentence_regex: Regex::new(r"[.!?]+\s+").unwrap(),
            // Regex for paragraph boundaries (multiple newlines)
            paragraph_regex: Regex::new(r"\n\s*\n").unwrap(),
        }
    }
    
    /// Chunk a document based on the configured strategy
    pub fn chunk_document(
        &self, 
        content: &str,
        _metadata: Option<&super::DocumentMetadata>  // Prefixed with underscore
    ) -> Result<Vec<String>> {
        match &self.strategy {
            ChunkingStrategy::FixedSize { chunk_size, overlap } => {
                self.chunk_fixed_size(content, *chunk_size, *overlap)
            }
            ChunkingStrategy::Semantic { target_size, max_size, min_size } => {
                self.chunk_semantic(content, *target_size, *max_size, *min_size)
            }
            ChunkingStrategy::PageBased => {
                // For page-based, we'd need page markers from the parser
                // For now, fall back to semantic chunking
                self.chunk_semantic(content, 1000, 1500, 200)
            }
        }
    }
    
    /// Fixed-size chunking with overlap
    fn chunk_fixed_size(
        &self,
        content: &str,
        chunk_size: usize,
        overlap: usize
    ) -> Result<Vec<String>> {
        let mut chunks = Vec::new();
        let chars: Vec<char> = content.chars().collect();
        let total_len = chars.len();
        
        let mut start = 0;
        while start < total_len {
            let end = (start + chunk_size).min(total_len);
            let chunk: String = chars[start..end].iter().collect();
            chunks.push(chunk);
            
            if end >= total_len {
                break;
            }
            
            // Move forward by (chunk_size - overlap)
            start += chunk_size.saturating_sub(overlap);
        }
        
        Ok(chunks)
    }
    
    /// Semantic chunking based on paragraph and sentence boundaries
    fn chunk_semantic(
        &self,
        content: &str,
        target_size: usize,
        max_size: usize,
        min_size: usize
    ) -> Result<Vec<String>> {
        // First, split into paragraphs
        let paragraphs = self.split_into_paragraphs(content);
        
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();
        
        for paragraph in paragraphs {
            let paragraph_len = paragraph.len();
            
            // If the paragraph itself is too large, split it into sentences
            if paragraph_len > max_size {
                // Flush current chunk if it exists
                if !current_chunk.is_empty() {
                    chunks.push(current_chunk.trim().to_string());
                    current_chunk = String::new();
                }
                
                // Split large paragraph into sentences
                let sentences = self.split_into_sentences(&paragraph);
                for sentence in sentences {
                    if current_chunk.len() + sentence.len() > max_size {
                        // Flush current chunk
                        if !current_chunk.is_empty() {
                            chunks.push(current_chunk.trim().to_string());
                            current_chunk = String::new();
                        }
                        
                        // If single sentence is still too large, split it
                        if sentence.len() > max_size {
                            chunks.extend(self.split_large_text(&sentence, max_size));
                        } else {
                            current_chunk = sentence.clone();
                        }
                    } else {
                        if !current_chunk.is_empty() {
                            current_chunk.push(' ');
                        }
                        current_chunk.push_str(&sentence);
                    }
                }
            } else if current_chunk.len() + paragraph_len > target_size {
                // Adding this paragraph would exceed target size
                // Check if we should flush current chunk
                if current_chunk.len() >= min_size {
                    chunks.push(current_chunk.trim().to_string());
                    current_chunk = paragraph.clone();
                } else if current_chunk.len() + paragraph_len <= max_size {
                    // We can still add it without exceeding max
                    if !current_chunk.is_empty() {
                        current_chunk.push_str("\n\n");
                    }
                    current_chunk.push_str(&paragraph);
                } else {
                    // Would exceed max, must flush
                    chunks.push(current_chunk.trim().to_string());
                    current_chunk = paragraph.clone();
                }
            } else {
                // Add paragraph to current chunk
                if !current_chunk.is_empty() {
                    current_chunk.push_str("\n\n");
                }
                current_chunk.push_str(&paragraph);
            }
        }
        
        // Add any remaining content
        if !current_chunk.is_empty() {
            chunks.push(current_chunk.trim().to_string());
        }
        
        // Filter out any chunks that are too small (unless it's the only chunk)
        if chunks.len() > 1 {
            chunks = chunks.into_iter()
                .filter(|chunk| chunk.len() >= min_size)
                .collect();
        }
        
        // Ensure we have at least one chunk
        if chunks.is_empty() && !content.trim().is_empty() {
            chunks.push(content.trim().to_string());
        }
        
        Ok(chunks)
    }
    
    /// Split content into paragraphs
    fn split_into_paragraphs(&self, content: &str) -> Vec<String> {
        let paragraphs: Vec<String> = self.paragraph_regex
            .split(content)
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect();
        
        if paragraphs.is_empty() && !content.trim().is_empty() {
            vec![content.trim().to_string()]
        } else {
            paragraphs
        }
    }
    
    /// Split text into sentences
    fn split_into_sentences(&self, text: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        let mut current = String::new();
        let mut chars = text.chars().peekable();
        
        while let Some(ch) = chars.next() {
            current.push(ch);
            
            // Check for sentence boundaries
            if ch == '.' || ch == '!' || ch == '?' {
                // Look ahead to see if this is really a sentence boundary
                if let Some(&next_ch) = chars.peek() {
                    if next_ch.is_whitespace() {
                        // Check if the next non-whitespace char is uppercase
                        let mut temp_chars = chars.clone();
                        while let Some(&ws) = temp_chars.peek() {
                            if !ws.is_whitespace() {
                                if ws.is_uppercase() {
                                    // This is likely a sentence boundary
                                    sentences.push(current.trim().to_string());
                                    current = String::new();
                                    // Skip whitespace
                                    while let Some(&ws) = chars.peek() {
                                        if !ws.is_whitespace() {
                                            break;
                                        }
                                        chars.next();
                                    }
                                }
                                break;
                            }
                            temp_chars.next();
                        }
                    }
                }
            }
        }
        
        // Add any remaining text
        if !current.trim().is_empty() {
            sentences.push(current.trim().to_string());
        }
        
        if sentences.is_empty() && !text.trim().is_empty() {
            vec![text.trim().to_string()]
        } else {
            sentences
        }
    }
    
    /// Split large text that doesn't have natural boundaries
    fn split_large_text(&self, text: &str, max_size: usize) -> Vec<String> {
        let mut chunks = Vec::new();
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut current = String::new();
        
        for word in words {
            if current.len() + word.len() + 1 > max_size {
                if !current.is_empty() {
                    chunks.push(current.trim().to_string());
                    current = String::new();
                }
            }
            
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        
        if !current.is_empty() {
            chunks.push(current.trim().to_string());
        }
        
        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_fixed_size_chunking() {
        let chunker = DocumentChunker::with_strategy(ChunkingStrategy::FixedSize {
            chunk_size: 10,
            overlap: 2,
        });
        
        let content = "This is a test document for chunking.";
        let chunks = chunker.chunk_document(content, None).unwrap();
        
        assert!(chunks.len() > 1);
        assert!(chunks[0].len() <= 10);
    }
    
    #[test]
    fn test_semantic_chunking() {
        let chunker = DocumentChunker::new();
        
        let content = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph.";
        let chunks = chunker.chunk_document(content, None).unwrap();
        
        // Should respect paragraph boundaries when possible
        assert!(chunks.len() > 0);
    }
    
    #[test]
    fn test_sentence_splitting() {
        let chunker = DocumentChunker::new();
        
        let text = "This is the first sentence. This is the second sentence! Is this the third?";
        let sentences = chunker.split_into_sentences(text);
        
        assert_eq!(sentences.len(), 3);
        assert_eq!(sentences[0], "This is the first sentence.");
        assert_eq!(sentences[1], "This is the second sentence!");
        assert_eq!(sentences[2], "Is this the third?");
    }
}
