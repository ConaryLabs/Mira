//! Web tools: search and fetch

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

/// Convert HTML to plain text (basic implementation)
pub fn html_to_text(html: &str) -> String {
    // Remove script and style tags with their content
    let script_re = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let style_re = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let text = script_re.replace_all(html, "");
    let text = style_re.replace_all(&text, "");

    // Replace common block elements with newlines
    let block_re = Regex::new(r"(?i)</?(p|div|br|h[1-6]|li|tr)[^>]*>").unwrap();
    let text = block_re.replace_all(&text, "\n");

    // Remove all remaining HTML tags
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    let text = tag_re.replace_all(&text, "");

    // Decode common HTML entities
    let text = text
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    // Collapse multiple newlines and spaces
    let multi_newline = Regex::new(r"\n{3,}").unwrap();
    let multi_space = Regex::new(r" {2,}").unwrap();
    let text = multi_newline.replace_all(&text, "\n\n");
    let text = multi_space.replace_all(&text, " ");

    text.trim().to_string()
}

/// Web tool implementations
pub struct WebTools;

impl WebTools {
    pub async fn web_search(&self, args: &Value) -> Result<String> {
        let query = args["query"].as_str().unwrap_or("");
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;

        // Use DuckDuckGo HTML search (no API key required)
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
            Ok(format!("No results found for: {}", query))
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
        if status.as_u16() == 403 {
            // A lot of sites throw 403s at non-browser clients. If the Playwright sidecar is
            // running, fall back to it.
            if let Ok(text) = self.web_fetch_browser_inner(url, max_length, None, None, None).await
            {
                return Ok(text);
            }
        }

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

    pub async fn web_fetch_browser(&self, args: &Value) -> Result<String> {
        let url = args["url"].as_str().unwrap_or("");
        let max_length = args["max_length"].as_u64().unwrap_or(20000) as usize;
        let selector = args.get("selector").and_then(|v| v.as_str()).map(|s| s.to_string());
        let wait_until = args.get("wait_until").and_then(|v| v.as_str()).map(|s| s.to_string());
        let timeout_ms = args.get("timeout_ms").and_then(|v| v.as_u64());

        self.web_fetch_browser_inner(url, max_length, selector, wait_until, timeout_ms)
            .await
            .or_else(|e| Ok(format!("Error (browser fetch): {}", e)))
    }

    async fn web_fetch_browser_inner(
        &self,
        url: &str,
        max_length: usize,
        selector: Option<String>,
        wait_until: Option<String>,
        timeout_ms: Option<u64>,
    ) -> Result<String> {
        let fetchd = std::env::var("MIRA_FETCHD_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:7337".to_string());

        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (compatible; MiraChat/1.0)")
            .timeout(std::time::Duration::from_secs(75))
            .build()?;

        let payload = serde_json::json!({
            "url": url,
            "max_chars": max_length,
            "selector": selector,
            "wait_until": wait_until,
            "timeout_ms": timeout_ms,
        });

        let resp = client
            .post(format!("{}/fetch", fetchd.trim_end_matches('/')))
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Ok(format!("HTTP {} from fetchd: {}\n{}", status.as_u16(), url, body));
        }

        let v: Value = resp.json().await?;

        let title = v.get("title").and_then(|x| x.as_str()).unwrap_or("");
        let final_url = v.get("final_url").and_then(|x| x.as_str()).unwrap_or(url);
        let status = v.get("status").and_then(|x| x.as_u64()).unwrap_or(0);
        let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");

        let mut out = String::new();
        if !title.is_empty() {
            out.push_str(&format!("Title: {}\n", title));
        }
        if final_url != url {
            out.push_str(&format!("Final URL: {}\n", final_url));
        }
        if status != 0 {
            out.push_str(&format!("Status: {}\n\n", status));
        } else {
            out.push_str("\n");
        }
        out.push_str(text);

        Ok(out)
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
