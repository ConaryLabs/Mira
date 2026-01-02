// crates/mira-app/src/api.rs
// HTTP API functions for communicating with the Mira server

use serde::{Deserialize, Serialize};
use mira_types::{MemoryFact, CodeSearchResult, Goal, Task, ProjectContext};

pub async fn fetch_health() -> Result<String, String> {
    let window = web_sys::window().ok_or("No window")?;
    let location = window.location();
    let host = location.host().map_err(|_| "No host")?;
    let protocol = location.protocol().map_err(|_| "No protocol")?;

    let url = format!("{}//{}/api/health", protocol, host);

    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Fetch error: {:?}", e))?;

    resp.text()
        .await
        .map_err(|e| format!("Text error: {:?}", e))
}

pub async fn fetch_memories() -> Result<Vec<MemoryFact>, String> {
    let url = get_api_url("/api/memories");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<MemoryFact>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub async fn recall_memories(query: &str) -> Result<Vec<MemoryFact>, String> {
    let url = get_api_url("/api/recall");

    #[derive(Serialize)]
    struct RecallReq {
        query: String,
        limit: Option<u32>,
    }

    let resp = gloo_net::http::Request::post(&url)
        .json(&RecallReq { query: query.to_string(), limit: Some(20) })
        .map_err(|e| format!("{:?}", e))?
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<MemoryFact>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub async fn search_code(query: &str) -> Result<Vec<CodeSearchResult>, String> {
    let url = get_api_url("/api/search/code");

    #[derive(Serialize)]
    struct SearchReq {
        query: String,
        limit: Option<u32>,
    }

    let resp = gloo_net::http::Request::post(&url)
        .json(&SearchReq { query: query.to_string(), limit: Some(20) })
        .map_err(|e| format!("{:?}", e))?
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<CodeSearchResult>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub async fn fetch_goals() -> Result<Vec<Goal>, String> {
    let url = get_api_url("/api/goals");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<Goal>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub async fn fetch_tasks() -> Result<Vec<Task>, String> {
    let url = get_api_url("/api/tasks");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<Task>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub async fn send_chat_message(message: &str) -> Result<(), String> {
    let url = get_api_url("/api/chat");

    #[derive(Serialize)]
    struct ChatReq {
        message: String,
        history: Vec<serde_json::Value>,
    }

    let resp = gloo_net::http::Request::post(&url)
        .json(&ChatReq {
            message: message.to_string(),
            history: vec![],
        })
        .map_err(|e| format!("{:?}", e))?
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    if !resp.ok() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Chat error: {}", text));
    }

    Ok(())
}

// ═══════════════════════════════════════
// PROJECT API
// ═══════════════════════════════════════

pub async fn fetch_projects() -> Result<Vec<ProjectContext>, String> {
    let url = get_api_url("/api/projects");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Vec<ProjectContext>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub async fn fetch_current_project() -> Result<Option<ProjectContext>, String> {
    let url = get_api_url("/api/project");
    let resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: Option<ProjectContext>,
        error: Option<String>,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;

    // If there's an error like "No active project", return None instead of error
    if data.error.is_some() {
        return Ok(None);
    }

    Ok(data.data)
}

pub async fn set_project(path: &str, name: Option<&str>) -> Result<ProjectContext, String> {
    let url = get_api_url("/api/project/set");

    #[derive(Serialize)]
    struct SetProjectReq<'a> {
        path: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<&'a str>,
    }

    let resp = gloo_net::http::Request::post(&url)
        .json(&SetProjectReq { path, name })
        .map_err(|e| format!("{:?}", e))?
        .send()
        .await
        .map_err(|e| format!("{:?}", e))?;

    #[derive(Deserialize)]
    struct ApiResponse {
        data: ProjectContext,
    }

    let data: ApiResponse = resp.json().await.map_err(|e| format!("{:?}", e))?;
    Ok(data.data)
}

pub fn get_api_url(path: &str) -> String {
    let window = web_sys::window().expect("No window");
    let location = window.location();
    let host = location.host().expect("No host");
    let protocol = location.protocol().expect("No protocol");
    format!("{}//{}{}", protocol, host, path)
}
