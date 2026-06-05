//! Live graph viewer. A small read-only HTTP server (separate from the MCP
//! surface) that serves the current graph as JSON plus a self-contained
//! force-directed web UI. Connect a browser to the configured address
//! (default 127.0.0.1:7700) to watch the knowledge graph live; the page polls
//! `/graph` on an interval. Localhost-only by default — it exposes graph text.

use std::collections::HashSet;
use std::sync::{Arc, RwLock};

use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::base::graph::GraphGnn;
use crate::base::locks::read_recovered;
use crate::base::util::truncate;

type Graph = Arc<RwLock<GraphGnn>>;

/// Serve the viewer at `addr` until the process exits.
pub async fn run(graph: Graph, addr: &str) -> std::io::Result<()> {
	let app = Router::new()
		.route("/", get(index))
		.route("/graph", get(graph_json))
		.with_state(graph);
	let listener = tokio::net::TcpListener::bind(addr).await?;
	tracing::info!(target: "kern.viewer", addr = %addr, "graph viewer listening");
	axum::serve(listener, app).await
}

async fn index() -> Html<&'static str> {
	Html(VIEWER_HTML)
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

	Json(json!({
		"nodes": nodes,
		"links": links,
		"kerns": kerns.len(),
	}))
}

const VIEWER_HTML: &str = r#"<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <title>kern graph</title>
  <style>
    html,body{margin:0;height:100%;background:#0b0d10;color:#cdd3da;font:13px system-ui,sans-serif}
    #graph{width:100vw;height:100vh}
    #hud{position:fixed;top:10px;left:12px;z-index:10;background:#11151aee;padding:8px 12px;border-radius:8px;border:1px solid #222a33}
    #hud b{color:#7fd1ae}
    #err{color:#e06c75}
  </style>
  <script src="//unpkg.com/force-graph"></script>
</head>
<body>
  <div id="hud"><b>kern</b> graph &middot; <span id="stats">loading…</span> <span id="err"></span></div>
  <div id="graph"></div>
  <script>
    const el = document.getElementById('graph');
    const G = ForceGraph()(el)
      .backgroundColor('#0b0d10')
      .nodeLabel(n => `${n.kind} · heat ${(+n.heat).toFixed(2)} · conf ${(+n.conf).toFixed(2)}\n${n.label}`)
      .nodeAutoColorBy('kern')
      .nodeRelSize(4)
      .nodeVal(n => 1 + (+n.heat || 0) * 3)
      .linkColor(() => 'rgba(120,140,160,0.25)')
      .linkDirectionalArrowLength(2.5)
      .linkDirectionalArrowRelPos(1);
    async function load() {
      try {
        const r = await fetch('/graph');
        const d = await r.json();
        G.graphData({ nodes: d.nodes, links: d.links });
        document.getElementById('stats').textContent =
          `${d.nodes.length} thoughts · ${d.links.length} reasons · ${d.kerns} kerns`;
        document.getElementById('err').textContent = '';
      } catch (e) {
        document.getElementById('err').textContent = ' — ' + e;
      }
    }
    load();
    setInterval(load, 5000);
  </script>
</body>
</html>"#;
