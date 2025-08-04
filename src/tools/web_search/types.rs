// src/tools/web_search/types.rs

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Tavily API request structure (best for AI agents in 2025)
#[derive(Debug, Serialize, Deserialize)]
pub struct TavilySearchRequest {
    pub api_key: String,
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_depth: Option<String>,  // "basic" or "advanced"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_answer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_raw_content: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_domains: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TavilySearchResponse {
    pub answer: Option<String>,
    pub query: String,
    pub response_time: f64,
    pub results: Vec<TavilyResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TavilyResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: f32,
    pub raw_content: Option<String>,
    pub published_date: Option<String>,
}

/// Brave Search API structures
#[derive(Debug, Serialize, Deserialize)]
pub struct BraveSearchRequest {
    pub q: String,  // query
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safesearch: Option<String>,  // "off", "moderate", "strict"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<String>,  // "24h", "week", "month", "year"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveSearchResponse {
    pub r#type: String,
    pub query: BraveQuery,
    pub web: Option<BraveWebResults>,
    pub news: Option<BraveNewsResults>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveQuery {
    pub original: String,
    pub show_strict_warning: bool,
    pub altered: Option<String>,
    pub safesearch: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveWebResults {
    pub r#type: String,
    pub results: Vec<BraveWebResult>,
    pub family_friendly: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveWebResult {
    pub title: String,
    pub url: String,
    pub description: String,
    pub age: Option<String>,
    pub page_age: Option<String>,
    pub meta_url: Option<BraveMetaUrl>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveMetaUrl {
    pub scheme: String,
    pub netloc: String,
    pub hostname: String,
    pub favicon: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveNewsResults {
    pub r#type: String,
    pub results: Vec<BraveNewsResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveNewsResult {
    pub title: String,
    pub url: String,
    pub description: String,
    pub age: String,
    pub page_age: Option<String>,
    pub meta_url: Option<BraveMetaUrl>,
    pub thumbnail: Option<BraveThumbnail>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BraveThumbnail {
    pub src: String,
    pub height: Option<i32>,
    pub width: Option<i32>,
}

/// SerpAPI structures (Google results)
#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiRequest {
    pub q: String,
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hl: Option<String>,  // language
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gl: Option<String>,  // country
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe: Option<String>,  // safe search
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiResponse {
    pub search_metadata: SerpApiSearchMetadata,
    pub search_parameters: SerpApiSearchParameters,
    pub search_information: Option<SerpApiSearchInfo>,
    pub organic_results: Option<Vec<SerpApiOrganicResult>>,
    pub answer_box: Option<SerpApiAnswerBox>,
    pub knowledge_graph: Option<SerpApiKnowledgeGraph>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiSearchMetadata {
    pub id: String,
    pub status: String,
    pub json_endpoint: String,
    pub created_at: String,
    pub processed_at: String,
    pub total_time_taken: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiSearchParameters {
    pub q: String,
    pub location_requested: Option<String>,
    pub location_used: Option<String>,
    pub google_domain: String,
    pub hl: String,
    pub gl: String,
    pub device: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiSearchInfo {
    pub total_results: Option<i64>,
    pub time_taken_displayed: Option<f64>,
    pub query_displayed: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiOrganicResult {
    pub position: i32,
    pub title: String,
    pub link: String,
    pub displayed_link: String,
    pub snippet: String,
    pub snippet_highlighted_words: Option<Vec<String>>,
    pub date: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiAnswerBox {
    pub r#type: String,
    pub title: Option<String>,
    pub answer: Option<String>,
    pub snippet: Option<String>,
    pub snippet_highlighted_words: Option<Vec<String>>,
    pub link: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiKnowledgeGraph {
    pub title: String,
    pub r#type: Option<String>,
    pub description: Option<String>,
    pub source: Option<SerpApiSource>,
    pub facts: Option<Vec<SerpApiFact>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiSource {
    pub name: String,
    pub link: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SerpApiFact {
    pub label: String,
    pub value: String,
}

/// Cached search result for performance
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CachedSearchResult {
    pub query: String,
    pub result: crate::tools::web_search::WebSearchResult,
    pub cached_at: DateTime<Utc>,
    pub cache_ttl_seconds: i64,
}

impl CachedSearchResult {
    pub fn is_expired(&self) -> bool {
        let now = Utc::now();
        let age = now.signed_duration_since(self.cached_at);
        age.num_seconds() > self.cache_ttl_seconds
    }
}
