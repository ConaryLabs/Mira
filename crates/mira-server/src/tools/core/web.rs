//! Unified web tools (google_search, web_fetch, research)

use crate::tools::core::ToolContext;

/// Google search - requires google_search client (web-only)
pub async fn google_search<C: ToolContext>(
    ctx: &C,
    query: String,
    num_results: Option<i64>,
) -> Result<String, String> {
    let google = ctx.google_search()
        .ok_or("Google search is only available in web chat interface".to_string())?;
    
    let num_results = num_results.unwrap_or(5) as u32;
    let results = google.search(&query, num_results).await
        .map_err(|e| e.to_string())?;
    
    if results.is_empty() {
        return Ok("No search results found.".to_string());
    }
    
    let mut response = String::from("Search results:\n\n");
    for (i, result) in results.iter().enumerate() {
        response.push_str(&format!("{}. {}\n", i + 1, result.title));
        response.push_str(&format!("   {}\n", result.url));
        response.push_str(&format!("   {}\n\n", result.snippet));
    }
    
    Ok(response)
}

/// Fetch web page content - requires web_fetcher (web-only)
pub async fn web_fetch<C: ToolContext>(
    ctx: &C,
    url: String,
) -> Result<String, String> {
    let fetcher = ctx.web_fetcher()
        .ok_or("Web fetch is only available in web chat interface".to_string())?;
    
    let page = fetcher.fetch(&url).await
        .map_err(|e| e.to_string())?;
    
    let content_preview = if page.content.len() > 3000 {
        format!("{}...\n\n[Content truncated to 3000 characters]", &page.content[..3000])
    } else {
        page.content
    };
    
    Ok(format!("Title: {}\n\n{}", page.title, content_preview))
}

/// Research with synthesis - requires both google_search and web_fetcher (web-only)
pub async fn research<C: ToolContext>(
    ctx: &C,
    _question: String,
    _depth: Option<String>,
) -> Result<String, String> {
    let _google = ctx.google_search()
        .ok_or("Research is only available in web chat interface".to_string())?;
    let _fetcher = ctx.web_fetcher()
        .ok_or("Research is only available in web chat interface".to_string())?;

    // This is a simplified implementation
    // Full implementation would be in web/chat/tools.rs
    Err("Research tool not yet implemented in unified core. Use web chat interface.".to_string())
}
