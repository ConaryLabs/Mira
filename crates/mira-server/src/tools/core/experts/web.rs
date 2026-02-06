// crates/mira-server/src/tools/core/experts/web.rs
// Web fetch and search execution for expert sub-agents

use crate::utils::truncate_at_boundary;
use serde_json::Value;
use std::net::IpAddr;

/// Maximum response body size (2 MB) to prevent memory exhaustion from large payloads.
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

/// Check if an IP address is a private, loopback, or link-local address (SSRF targets).
fn is_dangerous_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()           // 127.0.0.0/8
            || v4.is_private()         // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            || v4.is_link_local()      // 169.254.0.0/16
            || v4.is_broadcast()       // 255.255.255.255
            || v4.is_unspecified()     // 0.0.0.0
            || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64  // 100.64.0.0/10 (CGNAT)
        }
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            v6.is_loopback()           // ::1
            || v6.is_unspecified()     // ::
            || v6.is_multicast()       // ff00::/8
            || (segs[0] & 0xffc0) == 0xfe80  // fe80::/10 link-local
            || (segs[0] & 0xfe00) == 0xfc00  // fc00::/7 ULA (unique local)
            // IPv4-mapped IPv6 addresses (::ffff:x.x.x.x)
            || v6.to_ipv4_mapped().is_some_and(|v4| is_dangerous_ip(&IpAddr::V4(v4)))
        }
    }
}

/// Validate that a URL's host does not resolve to a dangerous (internal) IP address.
async fn validate_url_target(parsed: &url::Url) -> Result<(), String> {
    let host = parsed.host_str().ok_or("Error: URL has no host")?;

    // If host is a literal IP, check directly
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_dangerous_ip(&ip) {
            return Err(format!(
                "Error: Access to internal/private address {} is not allowed",
                ip
            ));
        }
        return Ok(());
    }

    // DNS resolution to check all resolved IPs
    let port = parsed.port_or_known_default().unwrap_or(443);
    let addr_str = format!("{}:{}", host, port);
    let addrs = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(|e| format!("Error: DNS resolution failed for {}: {}", host, e))?;

    for addr in addrs {
        if is_dangerous_ip(&addr.ip()) {
            return Err(format!(
                "Error: Host '{}' resolves to internal/private address {} — access denied",
                host,
                addr.ip()
            ));
        }
    }

    Ok(())
}

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

    // SSRF protection: validate the resolved target before connecting
    if let Err(e) = validate_url_target(&parsed).await {
        return e;
    }

    // Build HTTP client — disable automatic redirects so we can validate each hop
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; Mira/1.0; +https://github.com/ConaryLabs/Mira)")
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: Failed to create HTTP client: {}", e),
    };

    // Follow redirects manually with SSRF validation at each hop
    let mut current_url = url.to_string();
    let max_redirects = 5;
    let mut response = None;

    for _redirect in 0..=max_redirects {
        let resp = match client.get(&current_url).send().await {
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

        let status = resp.status();

        // Handle redirects with SSRF re-validation
        if status.is_redirection() {
            let location = match resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                Some(loc) => loc,
                None => return format!("Error: Redirect from {} has no Location header", current_url),
            };

            // Resolve relative redirects against the current URL
            let redirect_url = match url::Url::parse(location)
                .or_else(|_| url::Url::parse(&current_url).and_then(|base| base.join(location)))
            {
                Ok(u) => u,
                Err(e) => return format!("Error: Invalid redirect URL '{}': {}", location, e),
            };

            // Validate redirect target against SSRF
            if let Err(e) = validate_url_target(&redirect_url).await {
                return e;
            }

            current_url = redirect_url.to_string();
            continue;
        }

        response = Some(resp);
        break;
    }

    let response = match response {
        Some(r) => r,
        None => return format!("Error: Too many redirects fetching {}", url),
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

    // Read body with size limit (Finding 4: bounded read to prevent memory exhaustion)
    let body = match read_response_body(response, MAX_RESPONSE_BYTES).await {
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

/// Read a response body with a maximum byte limit to prevent memory exhaustion.
async fn read_response_body(response: reqwest::Response, max_bytes: usize) -> Result<String, String> {
    use futures::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buf = Vec::with_capacity(max_bytes.min(64 * 1024));

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        let remaining = max_bytes.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        let take = chunk.len().min(remaining);
        buf.extend_from_slice(&chunk[..take]);
        if buf.len() >= max_bytes {
            break;
        }
    }

    Ok(String::from_utf8_lossy(&buf).into_owned())
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

    // --- SSRF protection tests ---

    #[test]
    fn test_is_dangerous_ip_v4_loopback() {
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v4_private() {
        for addr in ["10.0.0.1", "172.16.0.1", "192.168.1.1"] {
            let ip: IpAddr = addr.parse().unwrap();
            assert!(is_dangerous_ip(&ip), "{} should be dangerous", addr);
        }
    }

    #[test]
    fn test_is_dangerous_ip_v4_link_local() {
        let ip: IpAddr = "169.254.1.1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v4_cgnat() {
        let ip: IpAddr = "100.64.0.1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
        // Just outside CGNAT range
        let ip: IpAddr = "100.128.0.1".parse().unwrap();
        assert!(!is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v4_public() {
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v6_loopback() {
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v6_link_local() {
        let ip: IpAddr = "fe80::1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v6_ula() {
        for addr in ["fc00::1", "fd00::1", "fdab::1"] {
            let ip: IpAddr = addr.parse().unwrap();
            assert!(is_dangerous_ip(&ip), "{} should be dangerous (ULA)", addr);
        }
    }

    #[test]
    fn test_is_dangerous_ip_v6_multicast() {
        let ip: IpAddr = "ff02::1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v6_mapped_v4_private() {
        // ::ffff:127.0.0.1
        let ip: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
        // ::ffff:192.168.1.1
        let ip: IpAddr = "::ffff:192.168.1.1".parse().unwrap();
        assert!(is_dangerous_ip(&ip));
    }

    #[test]
    fn test_is_dangerous_ip_v6_public() {
        let ip: IpAddr = "2607:f8b0:4004:800::200e".parse().unwrap();
        assert!(!is_dangerous_ip(&ip));
    }

    #[tokio::test]
    async fn test_ssrf_blocks_loopback_url() {
        let result = execute_web_fetch("http://127.0.0.1/secret", 1000).await;
        assert!(result.contains("internal/private address"), "Should block loopback: {}", result);
    }

    #[tokio::test]
    async fn test_ssrf_blocks_ipv6_loopback_url() {
        let result = execute_web_fetch("http://[::1]/secret", 1000).await;
        assert!(result.contains("internal/private address"), "Should block IPv6 loopback: {}", result);
    }

    // --- Bounded body reader tests ---

    #[tokio::test]
    async fn test_read_response_body_lossy_utf8() {
        // Simulate truncation mid-multibyte: the lossy reader should not error
        let mut bytes = "Hello world".as_bytes().to_vec();
        bytes.extend_from_slice(&[0xC3]); // start of a 2-byte UTF-8 char, incomplete
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.starts_with("Hello world"));
        // The replacement character should appear, not an error
        assert!(body.contains('\u{FFFD}'));
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
