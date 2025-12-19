//! Web tools: search and fetch

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Cached regexes for HTML processing (compiled once at startup)
static RE_SCRIPT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("valid regex"));
static RE_STYLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("valid regex"));
static RE_BLOCK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)</?(p|div|br|h[1-6]|li|tr)[^>]*>").expect("valid regex"));
static RE_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").expect("valid regex"));
static RE_MULTI_NEWLINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").expect("valid regex"));
static RE_MULTI_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r" {2,}").expect("valid regex"));

/// Google Custom Search API response
#[derive(Debug, Deserialize)]
struct GoogleSearchResponse {
    items: Option<Vec<GoogleSearchItem>>,
    error: Option<GoogleSearchError>,
}

#[derive(Debug, Deserialize)]
struct GoogleSearchItem {
    title: String,
    link: String,
    snippet: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoogleSearchError {
    code: i32,
    message: String,
}

/// Configuration for web search
#[derive(Debug, Clone, Default)]
pub struct WebSearchConfig {
    /// Google API key (for Custom Search JSON API)
    pub google_api_key: Option<String>,
    /// Google Custom Search Engine ID (cx)
    pub google_cx: Option<String>,
}

/// Convert HTML to plain text (basic implementation)
pub fn html_to_text(html: &str) -> String {
    // Remove script and style tags with their content
    let text = RE_SCRIPT.replace_all(html, "");
    let text = RE_STYLE.replace_all(&text, "");

    // Replace common block elements with newlines
    let text = RE_BLOCK.replace_all(&text, "\n");

    // Remove all remaining HTML tags
    let text = RE_TAG.replace_all(&text, "");

    // Decode common HTML entities
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    // Collapse multiple newlines and spaces
    let text = RE_MULTI_NEWLINE.replace_all(&text, "\n\n");
    let text = RE_MULTI_SPACE.replace_all(&text, " ");

    text.trim().to_string()
}

/// Web tool implementations
pub struct WebTools {
    config: WebSearchConfig,
}

impl WebTools {
    /// Create WebTools with optional Google Search configuration
    pub fn new(config: WebSearchConfig) -> Self {
        Self { config }
    }

    /// Create WebTools with default (no Google Search) configuration
    pub fn new_default() -> Self {
        Self {
            config: WebSearchConfig::default(),
        }
    }

    /// Check if Google Custom Search is configured
    pub fn has_google_search(&self) -> bool {
        self.config.google_api_key.is_some() && self.config.google_cx.is_some()
    }

    pub async fn web_search(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;

        // Use Google Custom Search if configured, otherwise fall back to DuckDuckGo
        if self.has_google_search() {
            self.google_search(query, limit).await
        } else {
            self.duckduckgo_search(query, limit).await
        }
    }

    /// Search using Google Custom Search JSON API
    async fn google_search(&self, query: &str, limit: usize) -> Result<String> {
        let api_key = self.config.google_api_key.as_ref().unwrap();
        let cx = self.config.google_cx.as_ref().unwrap();

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .build()?;

        // Google Custom Search API - max 10 results per request
        let num = limit.min(10);
        let url = format!(
            "https://www.googleapis.com/customsearch/v1?key={}&cx={}&q={}&num={}",
            api_key,
            cx,
            urlencoding::encode(query),
            num
        );

        let response = client.get(&url).send().await?;
        let status = response.status();

        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Ok(format!("Google Search API error {}: {}", status, text));
        }

        let search_result: GoogleSearchResponse = response.json().await?;

        // Check for API errors in response body
        if let Some(error) = search_result.error {
            return Ok(format!("Google Search API error {}: {}", error.code, error.message));
        }

        let items = search_result.items.unwrap_or_default();
        if items.is_empty() {
            return Ok(format!("No results found for: {}", query));
        }

        let results: Vec<String> = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let snippet = item.snippet.as_deref().unwrap_or("").replace('\n', " ");
                format!("{}. {} - {}\n   {}", i + 1, item.title, item.link, snippet)
            })
            .collect();

        Ok(results.join("\n\n"))
    }

    /// Search using DuckDuckGo HTML (fallback, may get rate limited)
    async fn duckduckgo_search(&self, query: &str, limit: usize) -> Result<String> {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .build()?;

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = client.get(&url).send().await?;
        let html = response.text().await?;

        // Parse results from HTML
        let mut results = Vec::new();
        for (i, chunk) in html.split("result__a").enumerate().skip(1) {
            if i > limit {
                break;
            }
            // Extract href and title
            if let Some(href_start) = chunk.find("href=\"") {
                let href_rest = &chunk[href_start + 6..];
                if let Some(href_end) = href_rest.find('"') {
                    let href = &href_rest[..href_end];
                    // Decode DuckDuckGo redirect URL
                    let actual_url = if href.contains("uddg=") {
                        href.split("uddg=")
                            .nth(1)
                            .and_then(|s| s.split('&').next())
                            .map(|s| urlencoding::decode(s).unwrap_or_default().to_string())
                            .unwrap_or_else(|| href.to_string())
                    } else {
                        href.to_string()
                    };

                    // Extract title (text before </a>)
                    if let Some(title_end) = href_rest.find("</a>") {
                        let title_chunk = &href_rest[href_end + 2..title_end];
                        let title = title_chunk
                            .replace("<b>", "")
                            .replace("</b>", "")
                            .trim()
                            .to_string();
                        if !title.is_empty() && !actual_url.is_empty() {
                            results
                                .push(format!("{}. {} - {}", results.len() + 1, title, actual_url));
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok(format!("No results found for: {} (DuckDuckGo may be rate limiting)", query))
        } else {
            Ok(results.join("\n"))
        }
    }

    pub async fn web_fetch(&self, args: &Value) -> Result<String> {
        let url = args["url"].as_str().unwrap_or("");
        let max_length = args["max_length"].as_u64().unwrap_or(10000) as usize;

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return Ok(format!("Error fetching {}: {}", url, e)),
        };

        let status = response.status();
        if !status.is_success() {
            return Ok(format!("HTTP {}: {}", status.as_u16(), url));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Only process text content
        if !content_type.contains("text") && !content_type.contains("json") {
            return Ok(format!("Non-text content: {} ({})", content_type, url));
        }

        let body = response.text().await?;
        let is_html = content_type.contains("html");

        // Convert HTML to plain text (basic)
        let text = if is_html { html_to_text(&body) } else { body };

        // Truncate if too long
        if text.len() > max_length {
            Ok(format!(
                "{}...\n\n[Truncated, {} total bytes]",
                &text[..max_length],
                text.len()
            ))
        } else {
            Ok(text)
        }
    }
}

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
