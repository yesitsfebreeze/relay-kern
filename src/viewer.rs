//! Live graph data API. A small read-only HTTP server (separate from the MCP
//! surface) that serves the current graph as JSON. The UI is the standalone
//! Vite + Vue app in `viewer/` (hot-reloadable; talks to this endpoint), so the
//! UI is no longer baked into the binary. Bind address is `cfg.serve.viewer`
//! (default 127.0.0.1:7700, localhost-only; empty disables it).

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::base::graph::GraphGnn;
use crate::base::locks::read_recovered;
use crate::base::util::truncate;

type Graph = Arc<RwLock<GraphGnn>>;

/// Serve the graph data API at `addr` until the process exits.
pub async fn run(graph: Graph, addr: &str) -> std::io::Result<()> {
	let app = Router::new()
		.route("/", get(index))
		.route("/graph", get(graph_json))
		.with_state(graph);
	let listener = tokio::net::TcpListener::bind(addr).await?;
	tracing::info!(target: "kern.viewer", addr = %addr, "graph data API listening");
	axum::serve(listener, app).await
}

async fn index() -> &'static str {
	"kern graph data API. GET /graph for JSON. UI: run the Vue app in viewer/ (npm run dev)."
}

/// Snapshot the live graph as `{nodes, links, kerns}`. Nodes are entities
/// (id, truncated text, kind, kern, heat, confidence); links are reason edges.
/// Edges whose endpoints are not both present (e.g. into an unloaded kern) are
/// dropped so the client never sees a dangling link.
async fn graph_json(State(g): State<Graph>) -> Json<serde_json::Value> {
	let g = read_recovered(&g);
	let kerns = g.all();

	let mut node_ids: HashSet<String> = HashSet::new();
	let mut nodes = Vec::new();
	for kern in &kerns {
		for e in kern.entities.values() {
			node_ids.insert(e.id.clone());
			nodes.push(json!({
				"id": e.id,
				"label": truncate(&e.text(), 60),
				"kind": format!("{:?}", e.kind),
				"kern": kern.id,
				"heat": e.heat,
				"conf": e.conf_mean(),
			}));
		}
	}

	let mut links = Vec::new();
	for kern in &kerns {
		for r in kern.reasons.values() {
			if node_ids.contains(&r.from) && node_ids.contains(&r.to) {
				links.push(json!({
					"source": r.from,
					"target": r.to,
					"kind": format!("{:?}", r.kind),
				}));
			}
		}
	}

	// Sphere structure: the recursive kern tree (purpose, radii, parent/children,
	// member count). The viewer renders each kern as a sphere you can step into.
	let kern_meta: Vec<_> = kerns
		.iter()
		.map(|k| {
			json!({
				"id": k.id,
				"label": if k.purpose_text.trim().is_empty() { "(unnamed)".to_string() } else { truncate(&k.purpose_text, 60) },
				"named": !k.purpose_text.trim().is_empty(),
				"parent": k.parent,
				"children": k.children,
				"inner_radius": k.inner_radius,
				"outer_radius": k.outer_radius,
				"count": k.entities.len(),
			})
		})
		.collect();

	Json(json!({
		"nodes": nodes,
		"links": links,
		"kerns": kern_meta,
		"kern_count": kerns.len(),
	}))
}
