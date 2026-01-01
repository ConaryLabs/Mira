// src/web/embedded.rs
// Embedded static assets for single-binary distribution

use axum::{
    body::Body,
    http::{header, HeaderValue, Request, Response, StatusCode},
    response::IntoResponse,
};
use rust_embed::RustEmbed;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::Service;

/// Static assets (CSS, HTML, favicon)
#[derive(RustEmbed)]
#[folder = "../../assets/"]
pub struct Assets;

/// WASM package files
#[derive(RustEmbed)]
#[folder = "../../pkg/"]
pub struct Pkg;

/// Get MIME type from file extension
fn mime_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}

/// Serve an embedded file
fn serve_embedded<E: RustEmbed>(path: &str) -> Response<Body> {
    match E::get(path) {
        Some(content) => {
            let mime = mime_type(path);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, HeaderValue::from_static(mime))
                .header(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600"))
                .body(Body::from(content.data.into_owned()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap(),
    }
}

/// Service for embedded assets
#[derive(Clone)]
pub struct EmbeddedAssets;

impl<B> Service<Request<B>> for EmbeddedAssets
where
    B: Send + 'static,
{
    type Response = Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let path = req.uri().path().trim_start_matches('/');
        let response = serve_embedded::<Assets>(path);
        Box::pin(async move { Ok(response) })
    }
}

/// Service for embedded WASM package
#[derive(Clone)]
pub struct EmbeddedPkg;

impl<B> Service<Request<B>> for EmbeddedPkg
where
    B: Send + 'static,
{
    type Response = Response<Body>;
    type Error = std::convert::Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let path = req.uri().path().trim_start_matches('/');
        let response = serve_embedded::<Pkg>(path);
        Box::pin(async move { Ok(response) })
    }
}

/// Handler for serving the index.html
pub async fn index_html() -> impl IntoResponse {
    serve_embedded::<Assets>("index.html")
}
