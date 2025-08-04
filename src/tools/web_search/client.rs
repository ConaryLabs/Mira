// src/tools/web_search/client.rs

use super::types::*;
use super::{WebSearchArgs, WebSearchResult, WebSearchError, WebSearchConfig, SearchProvider, SearchSource, RawSearchResult, SearchDepth};
use reqwest::Client;
use std::time::Duration;
use std::collections::HashMap;
use async_trait::async_trait;

/// Trait for web search providers
#[async_trait]
pub trait SearchClient: Send + Sync {
    async fn search(&self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError>;
    fn provider_name(&self) -> &str;
}

/// Main web search client that routes to different providers
pub struct WebSearchClient {
    config: WebSearchConfig,
    http_client: Client,
    cache: HashMap<String, CachedSearchResult>,
}

impl WebSearchClient {
    pub fn new(config: WebSearchConfig) -> Result<Self, WebSearchError> {
        let http_client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_seconds))
            .user_agent("Mira-AI/1.0")
            .build()
            .map_err(|e| WebSearchError::NetworkError(e))?;

        Ok(Self {
            config,
            http_client,
            cache: HashMap::new(),
        })
    }

    /// Execute a web search using the configured provider
    pub async fn search(&mut self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError> {
        // Check cache first
        let cache_key = format!("{}:{}", self.config.provider.name(), &args.query);
        if let Some(cached) = self.cache.get(&cache_key) {
            if !cached.is_expired() {
                eprintln!("🎯 Cache hit for query: {}", args.query);
                return Ok(cached.result.clone());
            }
        }

        eprintln!("🔍 Executing web search: {} via {}", args.query, self.config.provider.name());
        
        // Route to appropriate provider
        let result = match &self.config.provider {
            SearchProvider::Tavily => {
                let client = TavilyClient::new(
                    self.config.api_key.clone().ok_or(WebSearchError::InvalidApiKey)?,
                    self.http_client.clone(),
                );
                client.search(args).await?
            },
            SearchProvider::Brave => {
                let client = BraveClient::new(
                    self.config.api_key.clone().ok_or(WebSearchError::InvalidApiKey)?,
                    self.http_client.clone(),
                );
                client.search(args).await?
            },
            SearchProvider::SerpApi => {
                let client = SerpApiClient::new(
                    self.config.api_key.clone().ok_or(WebSearchError::InvalidApiKey)?,
                    self.http_client.clone(),
                );
                client.search(args).await?
            },
            SearchProvider::Bing => {
                return Err(WebSearchError::ApiError("Bing provider not yet implemented".to_string()));
            },
            SearchProvider::DuckDuckGo => {
                let client = DuckDuckGoClient::new(self.http_client.clone());
                client.search(args).await?
            },
        };

        // Cache the result
        let cached = CachedSearchResult {
            query: args.query.clone(),
            result: result.clone(),
            cached_at: chrono::Utc::now(),
            cache_ttl_seconds: 300, // 5 minute cache
        };
        self.cache.insert(cache_key, cached);

        Ok(result)
    }
}

impl SearchProvider {
    pub fn name(&self) -> &str {
        match self {
            SearchProvider::Tavily => "Tavily",
            SearchProvider::SerpApi => "SerpApi",
            SearchProvider::Bing => "Bing",
            SearchProvider::Brave => "Brave",
            SearchProvider::DuckDuckGo => "DuckDuckGo",
        }
    }
}

/// Tavily search client - optimized for AI agents
pub struct TavilyClient {
    api_key: String,
    http_client: Client,
}

impl TavilyClient {
    pub fn new(api_key: String, http_client: Client) -> Self {
        Self { api_key, http_client }
    }
}

#[async_trait]
impl SearchClient for TavilyClient {
    async fn search(&self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError> {
        let url = "https://api.tavily.com/search";
        
        let request = TavilySearchRequest {
            api_key: self.api_key.clone(),
            query: args.query.clone(),
            search_depth: Some(match args.search_depth {
                SearchDepth::Basic => "basic".to_string(),
                SearchDepth::Advanced => "advanced".to_string(),
            }),
            max_results: Some(args.num_results),
            include_answer: Some(true),
            include_raw_content: Some(args.search_depth == SearchDepth::Advanced),
            include_domains: None,
            exclude_domains: None,
        };

        let response = self.http_client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|e| WebSearchError::NetworkError(e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            
            if status.as_u16() == 429 {
                return Err(WebSearchError::RateLimitExceeded);
            } else if status.as_u16() == 401 {
                return Err(WebSearchError::InvalidApiKey);
            }
            
            return Err(WebSearchError::ApiError(format!("Tavily API error {}: {}", status, error_text)));
        }

        let tavily_response: TavilySearchResponse = response.json().await
            .map_err(|e| WebSearchError::ApiError(format!("Failed to parse Tavily response: {}", e)))?;

        // Convert Tavily response to our format
        let sources: Vec<SearchSource> = tavily_response.results.iter().map(|r| SearchSource {
            title: r.title.clone(),
            url: r.url.clone(),
            snippet: r.content.clone(),
            published_date: r.published_date.clone(),
            domain: extract_domain(&r.url),
            relevance_score: Some(r.score),
        }).collect();

        let raw_results = if args.search_depth == SearchDepth::Advanced {
            Some(tavily_response.results.iter().map(|r| RawSearchResult {
                title: r.title.clone(),
                url: r.url.clone(),
                snippet: r.content.clone(),
                body: r.raw_content.clone(),
                published_date: r.published_date.clone(),
                author: None,
                score: Some(r.score),
            }).collect())
        } else {
            None
        };

        Ok(WebSearchResult {
            summary: tavily_response.answer.unwrap_or_else(|| {
                format!("Found {} results for '{}'", sources.len(), args.query)
            }),
            sources,
            total_results: Some(tavily_response.results.len() as i32),
            provider: "Tavily".to_string(),
            raw_results,
        })
    }

    fn provider_name(&self) -> &str {
        "Tavily"
    }
}

/// Brave search client
pub struct BraveClient {
    api_key: String,
    http_client: Client,
}

impl BraveClient {
    pub fn new(api_key: String, http_client: Client) -> Self {
        Self { api_key, http_client }
    }
}

#[async_trait]
impl SearchClient for BraveClient {
    async fn search(&self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError> {
        let url = "https://api.search.brave.com/res/v1/web/search";
        
        let mut params = HashMap::new();
        params.insert("q", args.query.clone());
        params.insert("count", args.num_results.to_string());
        params.insert("safesearch", "moderate".to_string());

        let response = self.http_client
            .get(url)
            .header("X-Subscription-Token", &self.api_key)
            .query(&params)
            .send()
            .await
            .map_err(|e| WebSearchError::NetworkError(e))?;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 429 {
                return Err(WebSearchError::RateLimitExceeded);
            } else if status.as_u16() == 401 {
                return Err(WebSearchError::InvalidApiKey);
            }
            return Err(WebSearchError::ApiError(format!("Brave API error: {}", status)));
        }

        let brave_response: BraveSearchResponse = response.json().await
            .map_err(|e| WebSearchError::ApiError(format!("Failed to parse Brave response: {}", e)))?;

        let web_results = brave_response.web.ok_or(WebSearchError::NoResults)?;
        
        let sources: Vec<SearchSource> = web_results.results.iter().map(|r| SearchSource {
            title: r.title.clone(),
            url: r.url.clone(),
            snippet: r.description.clone(),
            published_date: r.page_age.clone(),
            domain: r.meta_url.as_ref().map(|m| m.hostname.clone()),
            relevance_score: None,
        }).collect();

        Ok(WebSearchResult {
            summary: format!("Found {} results for '{}'", sources.len(), args.query),
            sources,
            total_results: Some(web_results.results.len() as i32),
            provider: "Brave".to_string(),
            raw_results: None,
        })
    }

    fn provider_name(&self) -> &str {
        "Brave"
    }
}

/// SerpAPI client (Google search results)
pub struct SerpApiClient {
    api_key: String,
    http_client: Client,
}

impl SerpApiClient {
    pub fn new(api_key: String, http_client: Client) -> Self {
        Self { api_key, http_client }
    }
}

#[async_trait]
impl SearchClient for SerpApiClient {
    async fn search(&self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError> {
        let url = "https://serpapi.com/search";
        
        let mut params = HashMap::new();
        params.insert("q", args.query.clone());
        params.insert("api_key", self.api_key.clone());
        params.insert("num", args.num_results.to_string());
        params.insert("engine", "google".to_string());

        let response = self.http_client
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(|e| WebSearchError::NetworkError(e))?;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 429 {
                return Err(WebSearchError::RateLimitExceeded);
            } else if status.as_u16() == 401 {
                return Err(WebSearchError::InvalidApiKey);
            }
            return Err(WebSearchError::ApiError(format!("SerpAPI error: {}", status)));
        }

        let serp_response: SerpApiResponse = response.json().await
            .map_err(|e| WebSearchError::ApiError(format!("Failed to parse SerpAPI response: {}", e)))?;

        let organic_results = serp_response.organic_results.ok_or(WebSearchError::NoResults)?;
        
        let sources: Vec<SearchSource> = organic_results.iter().map(|r| SearchSource {
            title: r.title.clone(),
            url: r.link.clone(),
            snippet: r.snippet.clone(),
            published_date: r.date.clone(),
            domain: Some(r.displayed_link.clone()),
            relevance_score: Some(1.0 / (r.position as f32)),
        }).collect();

        // Include answer box if available
        let summary = if let Some(answer_box) = serp_response.answer_box {
            answer_box.answer.or(answer_box.snippet).unwrap_or_else(|| {
                format!("Found {} results for '{}'", sources.len(), args.query)
            })
        } else {
            format!("Found {} results for '{}'", sources.len(), args.query)
        };

        Ok(WebSearchResult {
            summary,
            sources,
            total_results: serp_response.search_information.and_then(|i| i.total_results.map(|t| t as i32)),
            provider: "Google (via SerpAPI)".to_string(),
            raw_results: None,
        })
    }

    fn provider_name(&self) -> &str {
        "SerpAPI"
    }
}

/// DuckDuckGo client (free, no API key required)
pub struct DuckDuckGoClient {
    http_client: Client,
}

impl DuckDuckGoClient {
    pub fn new(http_client: Client) -> Self {
        Self { http_client }
    }
}

#[async_trait]
impl SearchClient for DuckDuckGoClient {
    async fn search(&self, args: &WebSearchArgs) -> Result<WebSearchResult, WebSearchError> {
        // DuckDuckGo instant answer API (limited but free)
        let url = "https://api.duckduckgo.com/";
        
        let mut params = HashMap::new();
        params.insert("q", args.query.clone());
        params.insert("format", "json".to_string());
        params.insert("no_html", "1".to_string());
        params.insert("skip_disambig", "1".to_string());

        let response = self.http_client
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(|e| WebSearchError::NetworkError(e))?;

        if !response.status().is_success() {
            return Err(WebSearchError::ApiError("DuckDuckGo API error".to_string()));
        }

        // Parse the limited DuckDuckGo response
        let ddg_response: serde_json::Value = response.json().await
            .map_err(|e| WebSearchError::ApiError(format!("Failed to parse DuckDuckGo response: {}", e)))?;

        // DuckDuckGo instant answer API is very limited
        // For production, consider using their HTML search and parsing
        let mut sources = Vec::new();
        
        // Check for abstract/answer
        if let Some(abstract_text) = ddg_response["AbstractText"].as_str() {
            if !abstract_text.is_empty() {
                sources.push(SearchSource {
                    title: ddg_response["Heading"].as_str().unwrap_or("DuckDuckGo Result").to_string(),
                    url: ddg_response["AbstractURL"].as_str().unwrap_or("").to_string(),
                    snippet: abstract_text.to_string(),
                    published_date: None,
                    domain: ddg_response["AbstractSource"].as_str().map(|s| s.to_string()),
                    relevance_score: Some(1.0),
                });
            }
        }

        // Check for related topics
        if let Some(related) = ddg_response["RelatedTopics"].as_array() {
            for topic in related.iter().take(args.num_results as usize) {
                if let Some(text) = topic["Text"].as_str() {
                    sources.push(SearchSource {
                        title: topic["Text"].as_str().unwrap_or("Related").to_string(),
                        url: topic["FirstURL"].as_str().unwrap_or("").to_string(),
                        snippet: text.to_string(),
                        published_date: None,
                        domain: None,
                        relevance_score: Some(0.8),
                    });
                }
            }
        }

        if sources.is_empty() {
            return Err(WebSearchError::NoResults);
        }

        let num_sources = sources.len() as i32;

        Ok(WebSearchResult {
            summary: format!("Found {} results for '{}'", num_sources, args.query),
            sources,
            total_results: Some(num_sources),
            provider: "DuckDuckGo".to_string(),
            raw_results: None,
        })
    }

    fn provider_name(&self) -> &str {
        "DuckDuckGo"
    }
}

/// Helper function to extract domain from URL
fn extract_domain(url: &str) -> Option<String> {
    url.split("://")
        .nth(1)
        .and_then(|s| s.split('/').next())
        .map(|s| s.to_string())
}
