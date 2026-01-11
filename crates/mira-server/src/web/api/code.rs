// crates/mira-server/src/web/api/code.rs
// Code symbols, semantic search, and indexing API handlers

use axum::{extract::State, response::IntoResponse, Json};
use mira_types::{ApiResponse, CodeSearchRequest, CodeSearchResponse, CodeSearchResult, IndexRequest, IndexStats};

use crate::web::state::AppState;

pub async fn get_symbols(
    State(_state): State<AppState>,
    Json(req): Json<serde_json::Value>,
) -> Json<ApiResponse<Vec<mira_types::Symbol>>> {
    let file_path = req.get("file_path").and_then(|v| v.as_str()).unwrap_or("");

    if file_path.is_empty() {
        return Json(ApiResponse::err("file_path required"));
    }

    match crate::indexer::extract_symbols(std::path::Path::new(file_path)) {
        Ok(symbols) => {
            let typed_symbols: Vec<mira_types::Symbol> = symbols
                .into_iter()
                .map(|s| mira_types::Symbol {
                    name: s.name,
                    qualified_name: s.qualified_name,
                    symbol_type: s.symbol_type,
                    language: s.language,
                    start_line: s.start_line,
                    end_line: s.end_line,
                    signature: s.signature,
                    visibility: s.visibility,
                    documentation: s.documentation,
                    is_test: s.is_test,
                    is_async: s.is_async,
                })
                .collect();
            Json(ApiResponse::ok(typed_symbols))
        }
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}

pub async fn semantic_search(
    State(state): State<AppState>,
    Json(req): Json<CodeSearchRequest>,
) -> impl IntoResponse {
    let project_id = state.project_id().await;

    // Try semantic search if embeddings available
    if let Some(ref embeddings) = state.embeddings {
        if let Ok(query_embedding) = embeddings.embed(&req.query).await {
            let conn = state.db.conn();
            let limit = req.limit.unwrap_or(10);

            let embedding_bytes: Vec<u8> = query_embedding
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();

            let result: Result<Vec<CodeSearchResult>, _> = (|| {
                let mut stmt = conn.prepare(
                    "SELECT file_path, chunk_content, vec_distance_cosine(embedding, ?1) as distance, start_line
                     FROM vec_code
                     WHERE project_id = ?2 OR ?2 IS NULL
                     ORDER BY distance
                     LIMIT ?3",
                )?;

                let rows = stmt.query_map(
                    rusqlite::params![embedding_bytes, project_id, limit],
                    |row| {
                        let distance: f64 = row.get(2)?;
                        let start_line: i64 = row.get::<_, Option<i64>>(3)?.unwrap_or(0);
                        Ok(CodeSearchResult {
                            file_path: row.get(0)?,
                            line_number: start_line as u32,
                            content: row.get(1)?,
                            symbol_name: None,
                            symbol_type: None,
                            score: (1.0 - distance) as f32,
                        })
                    },
                )?;

                rows.collect::<Result<Vec<_>, _>>()
            })();

            if let Ok(results) = result {
                return Json(ApiResponse::ok(CodeSearchResponse { results }));
            }
        }
    }

    Json(ApiResponse::err("Semantic search requires embeddings API key"))
}

pub async fn trigger_index(
    State(state): State<AppState>,
    Json(req): Json<IndexRequest>,
) -> impl IntoResponse {
    let project = state.get_project().await;

    let path = req
        .path
        .or_else(|| project.as_ref().map(|p| p.path.clone()))
        .unwrap_or_else(|| ".".to_string());

    let path = std::path::PathBuf::from(&path);

    let project_id = state.project_id().await;

    match crate::indexer::index_project(&path, state.db.clone(), state.embeddings.clone(), project_id)
        .await
    {
        Ok(stats) => Json(ApiResponse::ok(IndexStats {
            files: stats.files,
            symbols: stats.symbols,
            chunks: stats.chunks,
            errors: stats.errors,
        })),
        Err(e) => Json(ApiResponse::err(e.to_string())),
    }
}
