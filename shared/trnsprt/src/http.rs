//! MCP Streamable HTTP transport (2025 spec).
//!
//! `POST /mcp` — client request → JSON response.
//! `GET  /mcp` — SSE stream for server-initiated notifications.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;

use crate::server::{dispatch, error_response};
use crate::McpServer;

/// Serve MCP Streamable HTTP on `addr` (e.g. `"127.0.0.1:3001"`).
pub async fn serve_http<S>(server: Arc<S>, addr: &str) -> std::io::Result<()>
where
	S: McpServer + Sync + 'static,
{
	let app = Router::new()
		.route("/mcp", post(handle_post::<S>))
		.route("/mcp", get(handle_get))
		.with_state(server);

	let listener = tokio::net::TcpListener::bind(addr).await?;
	axum::serve(listener, app).await?;
	Ok(())
}

async fn handle_post<S: McpServer + Sync + 'static>(
	State(server): State<Arc<S>>,
	body: String,
) -> impl IntoResponse {
	let frame: serde_json::Value = match serde_json::from_str(&body) {
		Ok(v) => v,
		Err(e) => {
			let resp = error_response(serde_json::Value::Null, -32700, &format!("parse error: {e}"));
			return (StatusCode::OK, axum::Json(resp));
		}
	};

	match dispatch(server.as_ref(), &frame) {
		Some(resp) => (StatusCode::OK, axum::Json(resp)),
		None => (StatusCode::ACCEPTED, axum::Json(serde_json::Value::Null)),
	}
}

async fn handle_get() -> impl IntoResponse {
	let stream = async_stream::stream! {
		loop {
			tokio::time::sleep(std::time::Duration::from_secs(25)).await;
			yield Ok::<Event, std::convert::Infallible>(Event::default().comment("keepalive"));
		}
	};
	Sse::new(stream)
}
