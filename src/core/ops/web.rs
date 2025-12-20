//! Core web operations - shared by MCP and Chat
//!
//! Web fetch and search functionality.

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::time::Duration;

use super::super::{CoreError, CoreResult};

// Cached regexes for HTML processing
static RE_SCRIPT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("valid regex"));
static RE_STYLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("valid regex"));
static RE_BLOCK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)</?(p|div|br|h[1-6]|li|tr)[^>]*>").expect("valid regex"));
static RE_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").expect("valid regex"));
static RE_MULTI_NEWLINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").expect("valid regex"));
static RE_MULTI_SPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r" {2,}").expect("valid regex"));

// ============================================================================
// Input/Output Types
// ============================================================================

pub struct WebFetchInput {
    pub url: String,
    pub max_length: usize,
    pub timeout: Option<Duration>,
}

pub struct WebFetchOutput {
    pub content: String,
    pub content_type: String,
    pub from_cache: bool,
    pub truncated: bool,
}

pub struct WebSearchInput {
    pub query: String,
    pub limit: usize,
    pub google_api_key: Option<String>,
    pub google_cx: Option<String>,
}

pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: Option<String>,
}

// ============================================================================
// Operations
// ============================================================================

/// Fetch content from a URL
pub async fn web_fetch(client: &Client, input: WebFetchInput) -> CoreResult<WebFetchOutput> {
    let timeout = input.timeout.unwrap_or(Duration::from_secs(30));

    // Try direct fetch first
    let response = match client
        .get(&input.url)
        .timeout(timeout)
        .send()
        .await
    {
        Ok(r) if r.status().as_u16() == 403 => {
            // Try cache fallback
            match fetch_from_cache(client, &input.url, timeout).await {
                Ok(r) => (r, true),
                Err(_) => return Err(CoreError::WebFetch(
                    input.url,
                    "HTTP 403 Forbidden (cache also unavailable)".to_string()
                )),
            }
        }
        Ok(r) if !r.status().is_success() => {
            return Err(CoreError::WebFetch(
                input.url,
                format!("HTTP {}", r.status().as_u16())
            ));
        }
        Ok(r) => (r, false),
        Err(e) => {
            return Err(CoreError::WebFetch(input.url, e.to_string()));
        }
    };

    let (response, from_cache) = response;

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Only process text content
    if !content_type.contains("text") && !content_type.contains("json") {
        return Err(CoreError::WebFetch(
            input.url,
            format!("Non-text content: {}", content_type)
        ));
    }

    let body = response.text().await
        .map_err(|e| CoreError::WebFetch(input.url.clone(), e.to_string()))?;

    let is_html = content_type.contains("html");
    let text = if is_html { html_to_text(&body) } else { body };

    // Add cache notice if from cache
    let text = if from_cache {
        format!("[Retrieved from Google Cache]\n\n{}", text)
    } else {
        text
    };

    // Truncate if too long
    let (content, truncated) = if text.len() > input.max_length {
        (format!("{}...\n\n[Truncated, {} total bytes]", &text[..input.max_length], text.len()), true)
    } else {
        (text, false)
    };

    Ok(WebFetchOutput {
        content,
        content_type,
        from_cache,
        truncated,
    })
}

/// Search the web using Google or DuckDuckGo
pub async fn web_search(client: &Client, input: WebSearchInput) -> CoreResult<Vec<SearchResult>> {
    // Use Google if configured
    if let (Some(api_key), Some(cx)) = (&input.google_api_key, &input.google_cx) {
        return google_search(client, &input.query, input.limit, api_key, cx).await;
    }

    // Fall back to DuckDuckGo
    duckduckgo_search(client, &input.query, input.limit).await
}

/// Search using Google Custom Search JSON API
async fn google_search(
    client: &Client,
    query: &str,
    limit: usize,
    api_key: &str,
    cx: &str,
) -> CoreResult<Vec<SearchResult>> {
    let num = limit.min(10);
    let url = format!(
        "https://www.googleapis.com/customsearch/v1?key={}&cx={}&q={}&num={}",
        api_key,
        cx,
        urlencoding::encode(query),
        num
    );

    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| CoreError::WebFetch(url.clone(), e.to_string()))?;

    if !response.status().is_success() {
        return Err(CoreError::WebFetch(
            url,
            format!("Google API error: {}", response.status())
        ));
    }

    #[derive(serde::Deserialize)]
    struct GoogleResponse {
        items: Option<Vec<GoogleItem>>,
    }

    #[derive(serde::Deserialize)]
    struct GoogleItem {
        title: String,
        link: String,
        snippet: Option<String>,
    }

    let result: GoogleResponse = response.json().await
        .map_err(|e| CoreError::WebFetch(url, e.to_string()))?;

    Ok(result.items.unwrap_or_default().into_iter().map(|item| {
        SearchResult {
            title: item.title,
            url: item.link,
            snippet: item.snippet,
        }
    }).collect())
}

/// Search using DuckDuckGo HTML (fallback)
async fn duckduckgo_search(
    client: &Client,
    query: &str,
    limit: usize,
) -> CoreResult<Vec<SearchResult>> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| CoreError::WebFetch(url.clone(), e.to_string()))?;

    let html = response.text().await
        .map_err(|e| CoreError::WebFetch(url, e.to_string()))?;

    let mut results = Vec::new();

    for (i, chunk) in html.split("result__a").enumerate().skip(1) {
        if i > limit {
            break;
        }

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

                // Extract title
                if let Some(title_end) = href_rest.find("</a>") {
                    let title_chunk = &href_rest[href_end + 2..title_end];
                    let title = title_chunk
                        .replace("<b>", "")
                        .replace("</b>", "")
                        .trim()
                        .to_string();

                    if !title.is_empty() && !actual_url.is_empty() {
                        results.push(SearchResult {
                            title,
                            url: actual_url,
                            snippet: None,
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Fetch URL from Google Cache
async fn fetch_from_cache(client: &Client, url: &str, timeout: Duration) -> CoreResult<reqwest::Response> {
    let cache_url = format!(
        "https://webcache.googleusercontent.com/search?q=cache:{}",
        urlencoding::encode(url)
    );

    let response = client.get(&cache_url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| CoreError::WebFetch(cache_url.clone(), e.to_string()))?;

    if response.status().is_success() {
        Ok(response)
    } else {
        Err(CoreError::WebFetch(cache_url, format!("Cache returned {}", response.status())))
    }
}

/// Convert HTML to plain text
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
    fn test_html_entities() {
        let html = "&lt;code&gt; &amp; &quot;test&quot;";
        let text = html_to_text(html);
        assert_eq!(text, "<code> & \"test\"");
    }
}
