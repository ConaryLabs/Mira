//! Web tools: search and fetch
//!
//! Thin wrapper delegating to core::ops::web for shared implementation.

use anyhow::Result;
use serde::Deserialize;
use serde_json::Value;

use crate::core::ops::web as core_web;

/// Configuration for web search
#[derive(Debug, Clone, Default)]
pub struct WebSearchConfig {
    /// Google API key (for Custom Search JSON API)
    pub google_api_key: Option<String>,
    /// Google Custom Search Engine ID (cx)
    pub google_cx: Option<String>,
}

/// Web tool implementations
pub struct WebTools {
    config: WebSearchConfig,
    client: reqwest::Client,
}

impl WebTools {
    /// Create WebTools with optional Google Search configuration
    pub fn new(config: WebSearchConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .build()
            .unwrap_or_default();

        Self { config, client }
    }

    /// Create WebTools with default (no Google Search) configuration
    pub fn new_default() -> Self {
        Self::new(WebSearchConfig::default())
    }

    /// Check if Google Custom Search is configured
    pub fn has_google_search(&self) -> bool {
        self.config.google_api_key.is_some() && self.config.google_cx.is_some()
    }

    pub async fn web_search(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;

        let input = core_web::WebSearchInput {
            query: query.to_string(),
            limit,
            google_api_key: self.config.google_api_key.clone(),
            google_cx: self.config.google_cx.clone(),
        };

        match core_web::web_search(&self.client, input).await {
            Ok(results) => {
                if results.is_empty() {
                    Ok(format!("No results found for: {}", query))
                } else {
                    let formatted: Vec<String> = results.iter().enumerate().map(|(i, r)| {
                        let snippet = r.snippet.as_deref().unwrap_or("").replace('\n', " ");
                        format!("{}. {} - {}\n   {}", i + 1, r.title, r.url, snippet)
                    }).collect();
                    Ok(formatted.join("\n\n"))
                }
            }
            Err(e) => Ok(format!("Search error: {}", e)),
        }
    }

    pub async fn web_fetch(&self, args: &Value) -> Result<String> {
        let url = args["url"].as_str().unwrap_or("");
        let max_length = args["max_length"].as_u64().unwrap_or(10000) as usize;

        let input = core_web::WebFetchInput {
            url: url.to_string(),
            max_length,
            timeout: None,
        };

        match core_web::web_fetch(&self.client, input).await {
            Ok(output) => Ok(output.content),
            Err(e) => Ok(format!("Error fetching {}: {}", url, e)),
        }
    }
}

// Re-export html_to_text for tests
pub use core_web::html_to_text;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_text() {
        let html = r#"
            <html>
            <head><script>alert('hi')</script></head>
            <body>
                <h1>Title</h1>
                <p>Hello <b>world</b>!</p>
                <div>Another &amp; line</div>
            </body>
            </html>
        "#;

        let text = html_to_text(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world!"));
        assert!(text.contains("Another & line"));
        assert!(!text.contains("<script>"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_html_to_text_entities() {
        let html = "&lt;code&gt; &amp; &quot;test&quot;";
        let text = html_to_text(html);
        assert_eq!(text, "<code> & \"test\"");
    }
}
