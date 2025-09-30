// src/memory/features/document_processing/parser.rs
//! Document parser for extracting text and metadata from various file formats

use anyhow::Result;
use std::path::Path;
use tokio::io::AsyncReadExt;

/// Raw document with extracted content and metadata
pub struct RawDocument {
    pub content: String,
    pub metadata: super::DocumentMetadata,
}

/// Document parser that handles multiple file formats
pub struct DocumentParser;

impl DocumentParser {
    /// Create a new document parser
    pub fn new() -> Self {
        Self
    }
    
    /// Parse a document file based on its extension
    pub async fn parse(&self, file_path: &Path) -> Result<RawDocument> {
        let extension = file_path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
            
        match extension.as_str() {
            "pdf" => self.parse_pdf(file_path).await,
            "docx" => self.parse_docx(file_path).await,
            "doc" => Err(anyhow::anyhow!("Legacy .doc format not supported. Please convert to .docx")),
            "txt" | "md" | "markdown" => self.parse_text(file_path).await,
            _ => Err(anyhow::anyhow!("Unsupported file type: {}", extension))
        }
    }
    
    /// Parse PDF files using pdf-extract and lopdf for metadata
    async fn parse_pdf(&self, file_path: &Path) -> Result<RawDocument> {
        use pdf_extract::extract_text;
        
        // First check if PDF is encrypted using lopdf
        let doc = lopdf::Document::load(file_path)?;
        if doc.is_encrypted() {
            return Err(anyhow::anyhow!("PDF is password protected. Please provide an unencrypted version."));
        }
        
        // Extract text content
        let content = extract_text(file_path)
            .map_err(|e| anyhow::anyhow!("Failed to extract PDF text: {}", e))?;
        
        // Extract metadata from PDF
        let metadata = self.extract_pdf_metadata(&doc);
        
        // Clean up the extracted text
        let cleaned_content = self.clean_text(&content);
        
        Ok(RawDocument {
            content: cleaned_content,
            metadata,
        })
    }
    
    /// Extract metadata from PDF document
    fn extract_pdf_metadata(&self, doc: &lopdf::Document) -> super::DocumentMetadata {
        let mut metadata = super::DocumentMetadata::default();
        
        // Get page count
        metadata.page_count = Some(doc.get_pages().len());
        
        // Try to extract document info from trailer dictionary
        // The trailer is a Dictionary, and Info contains an object ID reference
        if let Ok(info_ref) = doc.trailer.get(b"Info") {
            // info_ref is an Object that should be a Reference
            if let lopdf::Object::Reference(obj_id) = info_ref {
                // Now get the actual Info dictionary using the object ID
                if let Ok(info) = doc.get_object(*obj_id) {
                    if let lopdf::Object::Dictionary(dict) = info {
                        // Title - directly check the dictionary values
                        if let Ok(title) = dict.get(b"Title") {
                            if let lopdf::Object::String(ref s, _) = *title {
                                metadata.title = Some(String::from_utf8_lossy(s).to_string());
                            }
                        }
                        
                        // Author - directly check the dictionary values
                        if let Ok(author) = dict.get(b"Author") {
                            if let lopdf::Object::String(ref s, _) = *author {
                                metadata.author = Some(String::from_utf8_lossy(s).to_string());
                            }
                        }
                        
                        // Creation date - directly check the dictionary values
                        if let Ok(date) = dict.get(b"CreationDate") {
                            if let lopdf::Object::String(ref s, _) = *date {
                                metadata.creation_date = Some(String::from_utf8_lossy(s).to_string());
                            }
                        }
                    }
                }
            }
        }
        
        metadata
    }
    
    /// Parse DOCX files by extracting text from XML
    async fn parse_docx(&self, file_path: &Path) -> Result<RawDocument> {
        use zip::ZipArchive;
        use quick_xml::events::Event;
        use quick_xml::Reader;
        
        // Open DOCX as ZIP archive
        let file = std::fs::File::open(file_path)?;
        let mut archive = ZipArchive::new(file)?;
        
        // Extract main document content
        let mut content = String::new();
        
        // Read document.xml
        if let Ok(mut doc_file) = archive.by_name("word/document.xml") {
            let mut xml_content = String::new();
            std::io::Read::read_to_string(&mut doc_file, &mut xml_content)?;
            
            // Parse XML to extract text
            let mut reader = Reader::from_str(&xml_content);
            reader.config_mut().trim_text(true);
            
            let mut buf = Vec::new();
            let mut in_text = false;
            
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        // Look for text elements (w:t)
                        if e.name().as_ref() == b"w:t" {
                            in_text = true;
                        }
                    }
                    Ok(Event::Text(e)) => {
                        if in_text {
                            let text = e.unescape()
                                .map_err(|err| anyhow::anyhow!("XML decode error: {}", err))?;
                            content.push_str(&text);
                            content.push(' '); // Add space between text runs
                        }
                    }
                    Ok(Event::End(ref e)) => {
                        if e.name().as_ref() == b"w:t" {
                            in_text = false;
                        }
                        // Add newline for paragraph ends
                        if e.name().as_ref() == b"w:p" {
                            content.push('\n');
                        }
                    }
                    Ok(Event::Eof) => break,
                    Err(e) => return Err(anyhow::anyhow!("XML parsing error: {}", e)),
                    _ => {}
                }
                buf.clear();
            }
        } else {
            return Err(anyhow::anyhow!("Could not find document.xml in DOCX file"));
        }
        
        // Try to extract metadata from docProps/core.xml
        let metadata = self.extract_docx_metadata(&mut archive).unwrap_or_default();
        
        // Clean up the extracted text
        let cleaned_content = self.clean_text(&content);
        
        Ok(RawDocument {
            content: cleaned_content,
            metadata,
        })
    }
    
    /// Extract metadata from DOCX file
    fn extract_docx_metadata(&self, archive: &mut zip::ZipArchive<std::fs::File>) -> Result<super::DocumentMetadata> {
        use quick_xml::events::Event;
        use quick_xml::Reader;
        
        let mut metadata = super::DocumentMetadata::default();
        
        // Try to read core properties
        if let Ok(mut props_file) = archive.by_name("docProps/core.xml") {
            let mut xml_content = String::new();
            std::io::Read::read_to_string(&mut props_file, &mut xml_content)?;
            
            let mut reader = Reader::from_str(&xml_content);
            reader.config_mut().trim_text(true);
            
            let mut buf = Vec::new();
            let mut current_element = String::new();
            
            loop {
                match reader.read_event_into(&mut buf) {
                    Ok(Event::Start(ref e)) => {
                        current_element = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    }
                    Ok(Event::Text(e)) => {
                        let text = e.unescape()
                            .map_err(|err| anyhow::anyhow!("XML decode error: {}", err))?;
                        
                        match current_element.as_str() {
                            "dc:title" => metadata.title = Some(text.to_string()),
                            "dc:creator" | "cp:lastModifiedBy" => {
                                if metadata.author.is_none() {
                                    metadata.author = Some(text.to_string());
                                }
                            }
                            "dcterms:created" | "dcterms:modified" => {
                                if metadata.creation_date.is_none() {
                                    metadata.creation_date = Some(text.to_string());
                                }
                            }
                            _ => {}
                        }
                    }
                    Ok(Event::Eof) => break,
                    _ => {}
                }
                buf.clear();
            }
        }
        
        // Try to get page count from app.xml
        if let Ok(mut app_file) = archive.by_name("docProps/app.xml") {
            let mut xml_content = String::new();
            std::io::Read::read_to_string(&mut app_file, &mut xml_content)?;
            
            if let Some(pages_start) = xml_content.find("<Pages>") {
                if let Some(pages_end) = xml_content.find("</Pages>") {
                    let pages_str = &xml_content[pages_start + 7..pages_end];
                    if let Ok(pages) = pages_str.parse::<usize>() {
                        metadata.page_count = Some(pages);
                    }
                }
            }
        }
        
        Ok(metadata)
    }
    
    /// Parse plain text files (TXT, MD)
    async fn parse_text(&self, file_path: &Path) -> Result<RawDocument> {
        // Read file with encoding detection
        let mut file = tokio::fs::File::open(file_path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;
        
        // Detect encoding and convert to UTF-8
        let (content, _, had_errors) = encoding_rs::UTF_8.decode(&buffer);
        if had_errors {
            // Try with WINDOWS-1252 as fallback
            let (content, _, _) = encoding_rs::WINDOWS_1252.decode(&buffer);
            Ok(RawDocument {
                content: self.clean_text(&content),
                metadata: self.extract_text_metadata(&content),
            })
        } else {
            Ok(RawDocument {
                content: self.clean_text(&content),
                metadata: self.extract_text_metadata(&content),
            })
        }
    }
    
    /// Extract basic metadata from text content
    fn extract_text_metadata(&self, content: &str) -> super::DocumentMetadata {
        let mut metadata = super::DocumentMetadata::default();
        
        // For markdown files, try to extract title from first # heading
        if let Some(first_line) = content.lines().next() {
            if first_line.starts_with("# ") {
                metadata.title = Some(first_line[2..].trim().to_string());
            }
        }
        
        // Simple language detection based on common patterns
        if content.chars().filter(|c| c.is_ascii()).count() > content.len() * 9 / 10 {
            metadata.language = Some("en".to_string());
        }
        
        metadata
    }
    
    /// Clean extracted text by normalizing whitespace and removing artifacts
    fn clean_text(&self, text: &str) -> String {
        // Remove null bytes and other control characters
        let cleaned: String = text.chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .collect();
        
        // Normalize whitespace
        let lines: Vec<String> = cleaned
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();
        
        // Join lines with single newlines
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_clean_text() {
        let parser = DocumentParser::new();
        let dirty = "Hello\0World\r\n\n  \n\tExtra   spaces  \n\n";
        let clean = parser.clean_text(dirty);
        assert_eq!(clean, "Hello World\nExtra   spaces");
    }
}
