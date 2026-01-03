// src/web/search.rs
// Google Custom Search and web fetching for DeepSeek chat

use anyhow::{anyhow, Result};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use tracing::{debug, info, warn};

// ═══════════════════════════════════════
// GOOGLE CUSTOM SEARCH
// ═══════════════════════════════════════

/// Google Custom Search API response
#[derive(Debug, Deserialize)]
struct GoogleSearchResponse {
    items: Option<Vec<GoogleSearchItem>>,
}

#[derive(Debug, Deserialize)]
struct GoogleSearchItem {
    title: String,
    link: String,
    snippet: Option<String>,
}

/// Search result returned to the caller
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Google Custom Search client
pub struct GoogleSearchClient {
    api_key: String,
    cx: String,
    client: reqwest::Client,
}

impl GoogleSearchClient {
    /// Create a new Google Search client
    pub fn new(api_key: String, cx: String) -> Self {
        Self {
            api_key,
            cx,
            client: reqwest::Client::new(),
        }
    }

    /// Create from environment variables
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("GOOGLE_API_KEY").ok()?;
        let cx = std::env::var("GOOGLE_SEARCH_CX").ok()?;
        Some(Self::new(api_key, cx))
    }

    /// Search Google
    pub async fn search(&self, query: &str, num_results: u32) -> Result<Vec<SearchResult>> {
        let start_time = Instant::now();
        let num = num_results.min(10); // API limit

        let url = format!(
            "https://www.googleapis.com/customsearch/v1?key={}&cx={}&q={}&num={}",
            self.api_key,
            self.cx,
            urlencoding::encode(query),
            num
        );

        debug!(query = %query, num = num, "Executing Google search");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Google search request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            warn!(status = %status, body = %body, "Google search failed");
            return Err(anyhow!("Google search failed with status {}: {}", status, body));
        }

        let data: GoogleSearchResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse Google search response: {}", e))?;

        let results: Vec<SearchResult> = data
            .items
            .unwrap_or_default()
            .into_iter()
            .map(|item| SearchResult {
                title: item.title,
                url: item.link,
                snippet: item.snippet.unwrap_or_default(),
            })
            .collect();

        let duration_ms = start_time.elapsed().as_millis();
        info!(
            query = %query,
            results = results.len(),
            duration_ms = duration_ms,
            "Google search complete"
        );

        Ok(results)
    }
}

// ═══════════════════════════════════════
// WEB FETCHER
// ═══════════════════════════════════════

/// Fetched web page content
#[derive(Debug, Clone, Serialize)]
pub struct WebPage {
    pub url: String,
    pub title: String,
    pub content: String,
    pub word_count: usize,
}

/// Web page fetcher with HTML parsing
pub struct WebFetcher {
    client: reqwest::Client,
}

impl WebFetcher {
    /// Create a new web fetcher
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraBot/1.0)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }

    /// Fetch and parse a web page
    pub async fn fetch(&self, url: &str) -> Result<WebPage> {
        let start_time = Instant::now();
        debug!(url = %url, "Fetching web page");

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch {}: {}", url, e))?;

        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!("HTTP {} fetching {}", status, url));
        }

        let html = response
            .text()
            .await
            .map_err(|e| anyhow!("Failed to read response body: {}", e))?;

        let (title, content) = parse_html(&html);
        let word_count = content.split_whitespace().count();

        let duration_ms = start_time.elapsed().as_millis();
        info!(
            url = %url,
            title = %title,
            word_count = word_count,
            duration_ms = duration_ms,
            "Web page fetched"
        );

        Ok(WebPage {
            url: url.to_string(),
            title,
            content,
            word_count,
        })
    }
}

impl Default for WebFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse HTML and extract title and main content
fn parse_html(html: &str) -> (String, String) {
    let document = Html::parse_document(html);

    // Extract title
    let title_selector = Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_default()
        .trim()
        .to_string();

    // Remove script, style, nav, footer, header, aside elements
    let mut content = String::new();

    // Try to find main content areas first
    let main_selectors = ["main", "article", "#content", ".content", "#main", ".main"];
    let mut found_main = false;

    for selector_str in main_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(main_el) = document.select(&selector).next() {
                content = extract_text_content(main_el);
                if content.split_whitespace().count() > 100 {
                    found_main = true;
                    break;
                }
            }
        }
    }

    // Fall back to body if no main content found
    if !found_main {
        if let Ok(body_selector) = Selector::parse("body") {
            if let Some(body) = document.select(&body_selector).next() {
                content = extract_text_content(body);
            }
        }
    }

    // Clean up whitespace
    let content = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    // Truncate if too long (keep ~10k chars)
    let content = if content.len() > 10000 {
        content[..10000].to_string() + "\n...[truncated]"
    } else {
        content
    };

    (title, content)
}

/// Extract text content from an element, skipping script/style/nav
fn extract_text_content(element: scraper::ElementRef) -> String {
    let mut text = String::new();

    for node in element.children() {
        if let Some(el) = scraper::ElementRef::wrap(node) {
            let tag = el.value().name();
            // Skip non-content elements
            if matches!(
                tag,
                "script" | "style" | "nav" | "footer" | "header" | "aside" | "noscript" | "iframe"
            ) {
                continue;
            }
            // Recursively extract
            text.push_str(&extract_text_content(el));
            // Add newline after block elements
            if matches!(tag, "p" | "div" | "br" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                text.push('\n');
            }
        } else if let Some(text_node) = node.value().as_text() {
            text.push_str(text_node);
        }
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_html() {
        let html = r#"
            <html>
                <head><title>Test Page</title></head>
                <body>
                    <nav>Navigation</nav>
                    <main>
                        <h1>Hello World</h1>
                        <p>This is content.</p>
                    </main>
                    <footer>Footer</footer>
                </body>
            </html>
        "#;

        let (title, content) = parse_html(html);
        assert_eq!(title, "Test Page");
        assert!(content.contains("Hello World"));
        assert!(content.contains("This is content"));
        assert!(!content.contains("Navigation"));
        assert!(!content.contains("Footer"));
    }
}
