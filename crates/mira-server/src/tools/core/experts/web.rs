// crates/mira-server/src/tools/core/experts/web.rs
// Web fetch and search execution for expert sub-agents

use crate::utils::truncate_at_boundary;
use serde_json::Value;

/// Fetch a web page and extract text content
pub async fn execute_web_fetch(url: &str, max_chars: usize) -> String {
    if url.is_empty() {
        return "Error: URL is required".to_string();
    }

    // Validate URL
    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(e) => return format!("Error: Invalid URL '{}': {}", url, e),
    };

    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return format!(
            "Error: Only http:// and https:// URLs are supported, got {}",
            scheme
        );
    }

    // Build HTTP client with browser-like settings
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; Mira/1.0; +https://github.com/ConaryLabs/Mira)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: Failed to create HTTP client: {}", e),
    };

    // Fetch the page
    let response = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            if e.is_timeout() {
                return format!("Error: Request timed out fetching {}", url);
            }
            if e.is_connect() {
                return format!("Error: Could not connect to {}", url);
            }
            return format!("Error: Failed to fetch {}: {}", url, e);
        }
    };

    let status = response.status();
    if !status.is_success() {
        return format!("Error: HTTP {} when fetching {}", status.as_u16(), url);
    }

    // Check content type - only process text/html and text/plain
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let is_html = content_type.contains("text/html");
    let is_text = content_type.contains("text/plain") || content_type.contains("text/markdown");

    if !is_html && !is_text && !content_type.is_empty() {
        return format!(
            "Error: Unsupported content type '{}' for {}. Only HTML and text content is supported.",
            content_type, url
        );
    }

    // Read body
    let body = match response.text().await {
        Ok(b) => b,
        Err(e) => return format!("Error: Failed to read response body from {}: {}", url, e),
    };

    // Extract text content
    let text = if is_html || content_type.is_empty() {
        extract_text_from_html(&body)
    } else {
        body
    };

    // Truncate if needed
    let truncated = if text.len() > max_chars {
        format!(
            "{}\n\n... (truncated at {} chars, total {})",
            truncate_at_boundary(&text, max_chars),
            max_chars,
            text.len()
        )
    } else {
        text
    };

    format!("Content from {}:\n\n{}", url, truncated)
}

/// Extract readable text from HTML using scraper
pub(super) fn extract_text_from_html(html: &str) -> String {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);

    // Try to get main content area first
    let selectors = [
        "main",
        "article",
        "[role=main]",
        ".content",
        "#content",
        "body",
    ];
    for sel_str in &selectors {
        if let Ok(selector) = Selector::parse(sel_str)
            && let Some(element) = document.select(&selector).next()
        {
            let text = element.text().collect::<Vec<_>>().join(" ");
            let cleaned = clean_extracted_text(&text);
            if !cleaned.is_empty() && cleaned.len() > 100 {
                return cleaned;
            }
        }
    }

    // Fallback: get all text from body
    let text: String = document.root_element().text().collect::<Vec<_>>().join(" ");
    clean_extracted_text(&text)
}

/// Clean up extracted text - normalize whitespace and remove noise
pub(super) fn clean_extracted_text(text: &str) -> String {
    // Split into lines, trim each, then reassemble with paragraph breaks
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).collect();
    let mut result = String::with_capacity(text.len());
    let mut consecutive_empty = 0;

    for line in &lines {
        if line.is_empty() {
            consecutive_empty += 1;
            continue;
        }

        if !result.is_empty() {
            if consecutive_empty >= 1 {
                // Paragraph break (max 2 newlines)
                result.push_str("\n\n");
            } else {
                // Same paragraph - join with space
                result.push(' ');
            }
        }

        // Collapse internal whitespace within the line
        let mut last_was_space = false;
        for ch in line.chars() {
            if ch.is_whitespace() {
                if !last_was_space {
                    result.push(' ');
                    last_was_space = true;
                }
            } else {
                result.push(ch);
                last_was_space = false;
            }
        }

        consecutive_empty = 0;
    }

    result.trim().to_string()
}

/// Check if Brave Search API key is configured
pub fn has_brave_search() -> bool {
    std::env::var("BRAVE_API_KEY")
        .ok()
        .filter(|k| !k.trim().is_empty())
        .is_some()
}

/// Search the web using Brave Search API
pub async fn execute_web_search(query: &str, count: u32) -> String {
    if query.is_empty() {
        return "Error: Search query is required".to_string();
    }

    let api_key = match std::env::var("BRAVE_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => return "Error: BRAVE_API_KEY not configured. Set it in ~/.mira/.env to enable web search.".to_string(),
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: Failed to create HTTP client: {}", e),
    };

    let encoded_query = urlencoding::encode(query);
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}&extra_snippets=true&text_decorations=false",
        encoded_query, count
    );

    let response = match client
        .get(&url)
        .header("X-Subscription-Token", &api_key)
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            if e.is_timeout() {
                return "Error: Brave Search request timed out".to_string();
            }
            return format!("Error: Brave Search request failed: {}", e);
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return format!(
            "Error: Brave Search returned HTTP {}: {}",
            status.as_u16(),
            body
        );
    }

    let body: Value = match response.json().await {
        Ok(v) => v,
        Err(e) => return format!("Error: Failed to parse Brave Search response: {}", e),
    };

    // Extract web results
    let results = match body["web"]["results"].as_array() {
        Some(r) => r,
        None => return "No search results found.".to_string(),
    };

    if results.is_empty() {
        return "No search results found.".to_string();
    }

    let mut output = format!("Search results for \"{}\":\n\n", query);
    for (i, result) in results.iter().enumerate() {
        let title = result["title"].as_str().unwrap_or("(no title)");
        let url = result["url"].as_str().unwrap_or("");
        let description = result["description"].as_str().unwrap_or("(no description)");

        output.push_str(&format!(
            "{}. **{}**\n   {}\n   {}\n",
            i + 1,
            title,
            url,
            description
        ));

        // Include extra snippets if available (richer context for experts)
        if let Some(snippets) = result["extra_snippets"].as_array() {
            for snippet in snippets.iter().take(2) {
                if let Some(s) = snippet.as_str() {
                    output.push_str(&format!("   > {}\n", s));
                }
            }
        }

        output.push('\n');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_extracted_text() {
        let input = "  Hello   World  \n\n\n\n  Foo  ";
        let result = clean_extracted_text(input);
        assert_eq!(result, "Hello World\n\nFoo");
    }

    #[test]
    fn test_clean_extracted_text_preserves_double_newline() {
        let input = "Paragraph one.\n\nParagraph two.";
        let result = clean_extracted_text(input);
        assert_eq!(result, "Paragraph one.\n\nParagraph two.");
    }

    #[test]
    fn test_extract_text_from_html_basic() {
        let html = r#"<html><body><main><h1>Title</h1><p>Hello world</p></main></body></html>"#;
        let text = extract_text_from_html(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Hello world"));
    }

    #[test]
    fn test_extract_text_from_html_article() {
        let html = r#"<html><body><nav>Menu</nav><article><h1>Article Title</h1><p>Article content here with enough text to pass the 100 char minimum threshold for content extraction.</p></article></body></html>"#;
        let text = extract_text_from_html(html);
        assert!(text.contains("Article Title"));
        assert!(text.contains("Article content"));
    }

    #[tokio::test]
    async fn test_execute_web_fetch_invalid_url() {
        let result = execute_web_fetch("not-a-url", 1000).await;
        assert!(result.starts_with("Error:"));
    }

    #[tokio::test]
    async fn test_execute_web_fetch_empty_url() {
        let result = execute_web_fetch("", 1000).await;
        assert_eq!(result, "Error: URL is required");
    }

    #[tokio::test]
    async fn test_execute_web_fetch_bad_scheme() {
        let result = execute_web_fetch("ftp://example.com", 1000).await;
        assert!(result.contains("Only http:// and https://"));
    }

    #[tokio::test]
    async fn test_execute_web_search_empty_query() {
        let result = execute_web_search("", 5).await;
        assert_eq!(result, "Error: Search query is required");
    }

    #[tokio::test]
    async fn test_execute_web_search_no_api_key() {
        // Only run this test if BRAVE_API_KEY is not set (to avoid unsafe env manipulation)
        if has_brave_search() {
            // Skip test when key is present - we don't want to manipulate env unsafely
            return;
        }
        let result = execute_web_search("test query", 5).await;
        assert!(result.contains("BRAVE_API_KEY not configured"));
    }
}
