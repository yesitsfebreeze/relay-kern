//! Live graph data API + zero-config local aggregator.
//!
//! Each kern daemon is per-cwd. To let one Vite app show *every* running kern
//! on the machine with no configuration, the viewer has two layers:
//!
//! 1. **Local server** — every daemon binds an ephemeral loopback port and
//!    serves its own graph at `GET /graph`. It writes that address into a
//!    shared registry directory (`<temp>/kern-viewers/<pid>.json`) and
//!    heartbeats it. A browser can't read UDP broadcasts, so the registry is a
//!    file the aggregator (a process, not the browser) reads.
//! 2. **Aggregator** — every daemon races to bind the well-known address
//!    `cfg.serve.viewer` (default 127.0.0.1:7700). Exactly one wins and becomes
//!    the hub; the rest retry periodically so the hub fails over if it dies.
//!    The hub serves `GET /graph` by fanning out to every live peer in the
//!    registry, namespacing their ids, and merging into one `{nodes,links,kerns}`.
//!
//! The browser always fetches `127.0.0.1:7700/graph` and gets the union — zero
//! config whether one daemon runs or ten.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt as _;
use std::convert::Infallible;
use serde_json::{json, Value};

use crate::base::graph::GraphGnn;
use trnsprt::McpServer as _;
use crate::base::locks::{read_recovered, write_recovered};
use crate::base::util::truncate;
use crate::config::RetrievalConfig;
use crate::tick::queue::{task, TaskKind};

type Graph = Arc<RwLock<GraphGnn>>;

#[derive(Clone)]
struct LocalState {
	graph: Graph,
	retrieval: RetrievalConfig,
	queue: std::sync::Arc<crate::tick::queue::Queue>,
	mcp: Arc<crate::mcp::Server>,
}

/// Heartbeat cadence and the staleness window for treating a registry entry as
/// dead. A peer is live if its file was refreshed within `STALE` *and* its
/// `/graph` answers; otherwise the aggregator skips it.
const HEARTBEAT: Duration = Duration::from_secs(5);
const STALE: Duration = Duration::from_secs(20);
/// Per-peer fan-out timeout: a wedged daemon must not stall the whole view.
const FANOUT_TIMEOUT: Duration = Duration::from_secs(3);
/// How often a non-hub daemon retries binding the aggregator address.
const FAILOVER_RETRY: Duration = Duration::from_secs(4);
/// Upper bound on a single search request's `k`, so an over-large request can't
/// drive the HNSW `ef` budget into a multi-second scan while holding the read lock.
const MAX_SEARCH_K: usize = 200;

fn now_secs() -> u64 {
	SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn registry_dir() -> PathBuf {
	std::env::temp_dir().join("kern-viewers")
}

fn registry_file() -> PathBuf {
	registry_dir().join(format!("{}.json", std::process::id()))
}

/// Run the viewer: start this daemon's local graph server, register it, and
/// contend for the aggregator role. Never returns under normal operation.
pub async fn run(graph: Graph, llm: crate::llm::Client, retrieval: RetrievalConfig, queue: std::sync::Arc<crate::tick::queue::Queue>, mcp: Arc<crate::mcp::Server>, agg_addr: &str) -> std::io::Result<()> {
	// 1. Local graph server on an ephemeral loopback port (this daemon's own data).
	let local = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
	let local_addr = local.local_addr()?.to_string();
	let local_state = LocalState { graph: graph.clone(), retrieval: retrieval.clone(), queue: queue.clone(), mcp };
	let local_app = Router::new()
		.route("/graph", get(graph_json))
		.route("/ask_retrieve", post(ask_retrieve))
		.route("/edit", post(edit))
		.route("/tool", post(local_tool))
		.with_state(local_state);
	tokio::spawn(async move {
		if let Err(e) = axum::serve(local, local_app).await {
			tracing::warn!(target: "kern.viewer", error = %e, "local graph server exited");
		}
	});
	tracing::info!(target: "kern.viewer", addr = %local_addr, "local graph server listening");

	// 2. Register self + heartbeat so the hub can discover this daemon.
	spawn_registry(local_addr.clone());

	// 3. Contend for the aggregator address; retry so the hub can fail over.
	let client = reqwest::Client::builder()
		.timeout(FANOUT_TIMEOUT)
		.build()
		.unwrap_or_default();
	let agg_addr = agg_addr.to_string();
	loop {
		match tokio::net::TcpListener::bind(&agg_addr).await {
			Ok(listener) => {
				tracing::info!(target: "kern.viewer", addr = %agg_addr, "aggregator hub listening");
				let hub = HubState { client: client.clone(), llm: llm.clone() };
				let app = Router::new()
					.route("/", get(index))
					.route("/graph", get(aggregate))
					.route("/ask", post(ask))
					.route("/edit", post(hub_edit))
					.route("/tool", post(hub_tool))
					.with_state(hub);
				if let Err(e) = axum::serve(listener, app).await {
					tracing::warn!(target: "kern.viewer", error = %e, "aggregator hub exited; will retry");
				}
			}
			// Another daemon holds the hub. Wait, then retry to take over if it dies.
			Err(_) => tokio::time::sleep(FAILOVER_RETRY).await,
		}
	}
}

/// Write `<temp>/kern-viewers/<pid>.json` once, then refresh its timestamp on a
/// timer. Best-effort: registry failures degrade to "this daemon is invisible
/// to the hub", never crash the daemon.
fn spawn_registry(local_addr: String) {
	tokio::spawn(async move {
		let _ = std::fs::create_dir_all(registry_dir());
		let file = registry_file();
		loop {
			let body = json!({ "graph": local_addr, "ts": now_secs() }).to_string();
			let _ = std::fs::write(&file, &body);
			tokio::time::sleep(HEARTBEAT).await;
		}
	});
}

/// Read the registry directory and return the loopback `/graph` addresses of
/// every peer heartbeated within `STALE`. Stale files are swept.
fn live_peers() -> Vec<String> {
	let dir = registry_dir();
	let entries = match std::fs::read_dir(&dir) {
		Ok(e) => e,
		Err(_) => return Vec::new(),
	};
	let now = now_secs();
	let mut peers = Vec::new();
	for entry in entries.flatten() {
		let path = entry.path();
		let Ok(text) = std::fs::read_to_string(&path) else { continue };
		let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
		let ts = v.get("ts").and_then(Value::as_u64).unwrap_or(0);
		if now.saturating_sub(ts) > STALE.as_secs() {
			let _ = std::fs::remove_file(&path); // sweep dead daemons
			continue;
		}
		if let Some(addr) = v.get("graph").and_then(Value::as_str) {
			peers.push(addr.to_string());
		}
	}
	peers
}

async fn index() -> &'static str {
	"kern viewer aggregator. GET /graph for the merged graph across all running daemons."
}

/// Hub endpoint: fan out to every live peer, namespace ids per peer to avoid
/// cross-project collisions, and merge into one `{nodes,links,kerns}`.
async fn aggregate(State(st): State<HubState>) -> Json<Value> {
	let client = &st.client;
	let peers = live_peers();
	let mut nodes = Vec::new();
	let mut links = Vec::new();
	let mut kerns = Vec::new();

	for addr in &peers {
		let url = format!("http://{addr}/graph");
		let resp = match client.get(&url).send().await {
			Ok(r) => r,
			Err(_) => continue, // unreachable peer (race with shutdown) — skip
		};
		let Ok(v) = resp.json::<Value>().await else { continue };
		// Namespace by peer address so identical ids in different daemons (e.g.
		// the same Fact text hashing alike across projects) never merge or
		// shadow. Links stay valid because endpoints share the peer's tag.
		let tag = format!("{addr}|");
		merge_peer(&tag, &v, &mut nodes, &mut links, &mut kerns);
	}

	Json(json!({
		"nodes": nodes,
		"links": links,
		"kerns": kerns,
		"kern_count": kerns.len(),
		"daemons": peers.len(),
	}))
}

/// Re-key one peer's payload under `tag` and append to the merged arrays.
fn merge_peer(tag: &str, v: &Value, nodes: &mut Vec<Value>, links: &mut Vec<Value>, kerns: &mut Vec<Value>) {
	let pre = |id: &Value| -> Value {
		id.as_str().map(|s| Value::String(format!("{tag}{s}"))).unwrap_or(Value::Null)
	};
	for n in v.get("nodes").and_then(Value::as_array).into_iter().flatten() {
		let mut n = n.clone();
		if let Some(o) = n.as_object_mut() {
			if let Some(id) = o.get("id") { let p = pre(id); o.insert("id".into(), p); }
			if let Some(k) = o.get("kern") { let p = pre(k); o.insert("kern".into(), p); }
		}
		nodes.push(n);
	}
	for l in v.get("links").and_then(Value::as_array).into_iter().flatten() {
		let mut l = l.clone();
		if let Some(o) = l.as_object_mut() {
			if let Some(id) = o.get("id") { let p = pre(id); o.insert("id".into(), p); }
			if let Some(s) = o.get("source") { let p = pre(s); o.insert("source".into(), p); }
			if let Some(t) = o.get("target") { let p = pre(t); o.insert("target".into(), p); }
		}
		links.push(l);
	}
	for k in v.get("kerns").and_then(Value::as_array).into_iter().flatten() {
		let mut k = k.clone();
		if let Some(o) = k.as_object_mut() {
			if let Some(id) = o.get("id") { let p = pre(id); o.insert("id".into(), p); }
			match o.get("parent") {
				Some(p) if p.is_string() => { let np = pre(p); o.insert("parent".into(), np); }
				_ => {}
			}
			if let Some(ch) = o.get("children").and_then(Value::as_array) {
				let mapped: Vec<Value> = ch.iter().map(&pre).collect();
				o.insert("children".into(), Value::Array(mapped));
			}
		}
		kerns.push(k);
	}
}

/// Tag one peer's search payload (`{hits, reasons}`) and append every hit to
/// `out`, prefixing `id`/`kern` so they match the namespaced ids `/graph`
/// already shipped to the browser. Both arrays are pooled into one list.
fn merge_search_hits(tag: &str, v: &Value, out: &mut Vec<Value>) {
	let pre = |id: &Value| -> Value {
		id.as_str().map(|s| Value::String(format!("{tag}{s}"))).unwrap_or(Value::Null)
	};
	for arr in ["hits", "reasons"] {
		for h in v.get(arr).and_then(Value::as_array).into_iter().flatten() {
			let mut h = h.clone();
			if let Some(o) = h.as_object_mut() {
				if let Some(id) = o.get("id") { let p = pre(id); o.insert("id".into(), p); }
				if let Some(k) = o.get("kern") { let p = pre(k); o.insert("kern".into(), p); }
			}
			out.push(h);
		}
	}
}

/// Merge every peer's tagged payload, sort by `score` descending, truncate to k.
fn rank_peers(peers: &[(String, Value)], k: usize) -> Vec<Value> {
	let mut out = Vec::new();
	for (tag, v) in peers {
		merge_search_hits(tag, v, &mut out);
	}
	out.sort_by(|a, b| {
		let sa = a.get("score").and_then(Value::as_f64).unwrap_or(f64::NEG_INFINITY);
		let sb = b.get("score").and_then(Value::as_f64).unwrap_or(f64::NEG_INFINITY);
		sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
	});
	out.truncate(k);
	out
}

fn default_k() -> usize { 10 }

#[derive(Clone)]
struct HubState {
	client: reqwest::Client,
	llm: crate::llm::Client,
}


#[derive(serde::Deserialize)]
struct AskRetrieveBody {
	vec: Vec<f64>,
	question: String,
	#[serde(default = "default_k")]
	k: usize,
}

/// Peer endpoint for the oracle: retrieve (no generation) over THIS daemon's
/// graph and return scored source thoughts + a pre-formatted provenance string.
/// The hub merges these across daemons and does the single generation.
async fn ask_retrieve(State(st): State<LocalState>, Json(body): Json<AskRetrieveBody>) -> Json<Value> {
	use crate::retrieval::answer;
	use crate::retrieval::seed::Mode;
	let k = body.k.min(MAX_SEARCH_K);
	let g = read_recovered(&st.graph);
	let result = answer::query(
		&g,
		&st.retrieval,
		&body.vec,
		&body.question,
		Mode::Hybrid,
		None,
		None,
		None::<crate::retrieval::score::QueryOptions>,
	);
	let sources: Vec<Value> = result.entities.iter().take(k).map(|se| {
		json!({
			"id": se.entity.id,
			"label": truncate(&se.entity.text(), 80),
			"text": truncate(&se.entity.text(), 300),
			"kind": format!("{:?}", se.entity.kind),
			"kern": g.kern_of_entity(&se.entity.id).map(str::to_owned).unwrap_or_default(),
			"heat": se.entity.heat,
			"conf": se.entity.conf_mean(),
			"score": se.score,
		})
	}).collect();
	let chain_text = answer::format_chains(&g, &result.path_chains);
	let mut reasons: Vec<Value> = Vec::new();
	let mut seen = std::collections::HashSet::new();
	for chain in &result.path_chains {
		for (j, node_id) in chain.nodes.iter().enumerate() {
			if j % 2 == 0 { continue; } // even = entity, odd = reason
			if !seen.insert(node_id.clone()) { continue; }
			if let Some((r, _)) = crate::base::search::find_reason(&g, node_id) {
				reasons.push(json!({
					"id": r.id,
					"from": r.from,
					"to": r.to,
					"text": if !r.text.is_empty() {
						truncate(&r.text, 160).to_string()
					} else {
						r.kind.fallback_label().unwrap_or("").to_string()
					},
					"kind": format!("{:?}", r.kind),
				}));
			}
		}
	}
	Json(json!({ "sources": sources, "chain_text": chain_text, "reasons": reasons }))
}

#[derive(serde::Deserialize)]
struct ChatTurn {
	role: String,
	content: String,
}

#[derive(serde::Deserialize)]
struct AskBody {
	question: String,
	#[serde(default)]
	history: Vec<ChatTurn>,
	#[serde(default = "default_ask_k")]
	k: usize,
}

fn default_ask_k() -> usize { 8 }

static AGENT_SYSTEM_PROMPT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

fn build_agent_system_prompt() -> &'static str {
	AGENT_SYSTEM_PROMPT.get_or_init(|| {
		let defs = crate::mcp::tools::tool_definitions();
		let tools: Vec<String> = defs.iter().map(|d| {
			let name = d.get("name").and_then(Value::as_str).unwrap_or("");
			let desc = d.get("description").and_then(Value::as_str).unwrap_or("");
			format!("- {name}: {desc}")
		}).collect();
		format!(
			"You are kern's assistant. Answer questions using the provided memory context.\n\
			 You can call tools when the user asks to modify the knowledge graph (add/remove memories, \
			 manage anchors, create links). Do not call tools for questions — answer those directly.\n\n\
			 Available tools:\n{}\n\n\
			 To call a tool output exactly: <tool_call>{{\"name\":\"TOOL\",\"args\":{{...}}}}</tool_call>\n\
			 A tool call must be on its own, not embedded mid-sentence.",
			tools.join("\n")
		)
	})
}

struct ToolCall {
	name: String,
	args: Value,
}

/// Extract visible text and any <tool_call>…</tool_call> blocks from an LLM response.
fn extract_tool_calls(text: &str) -> (String, Vec<ToolCall>) {
	let mut visible = String::new();
	let mut calls = Vec::new();
	let mut rest = text;
	while let Some(open) = rest.find("<tool_call>") {
		visible.push_str(&rest[..open]);
		rest = &rest[open + "<tool_call>".len()..];
		if let Some(close) = rest.find("</tool_call>") {
			let json_str = rest[..close].trim();
			rest = &rest[close + "</tool_call>".len()..];
			if let Ok(v) = serde_json::from_str::<Value>(json_str) {
				if let Some(name) = v.get("name").and_then(Value::as_str) {
					let args = v.get("args").cloned().unwrap_or(Value::Object(Default::default()));
					calls.push(ToolCall { name: name.to_string(), args });
				}
			}
		}
	}
	visible.push_str(rest);
	(visible.trim().to_string(), calls)
}

/// Execute one tool on the first available peer. Returns (ok, result_text).
async fn exec_tool(client: &reqwest::Client, peers: &[String], name: &str, args: &Value) -> (bool, String) {
	let body = json!({ "name": name, "args": args });
	for addr in peers {
		let url = format!("http://{addr}/tool");
		if let Ok(r) = client.post(&url).timeout(FANOUT_TIMEOUT).json(&body).send().await {
			if let Ok(v) = r.json::<Value>().await {
				let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
				let result = v.get("result").and_then(Value::as_str).unwrap_or("done").to_string();
				return (ok, result);
			}
		}
	}
	(false, "no daemon available".to_string())
}

/// Hub oracle endpoint: embed the question once, fan retrieval out to peers,
/// merge sources by score, emit a `sources` SSE event, then run an agentic
/// tool loop. The LLM can call kern tools (ingest, anchor, forget, etc.) by
/// emitting <tool_call>…</tool_call> blocks; each is executed and the result
/// fed back before the final answer is streamed as `token` events.
async fn ask(State(st): State<HubState>, Json(body): Json<AskBody>) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
	let stream = async_stream::stream! {
		let q = body.question.trim().to_string();
		if q.is_empty() {
			yield Ok(Event::default().event("done").data("{}"));
			return;
		}
		let k = body.k.min(MAX_SEARCH_K);
		let vec = match st.llm.embed(&q).await {
			Ok(v) => v,
			Err(e) => {
				yield Ok(Event::default().event("error").data(json!({ "message": e.to_string() }).to_string()));
				return;
			}
		};
		let peers = live_peers();
		let reqbody = json!({ "vec": vec, "question": q, "k": k });
		let mut tagged = Vec::new();
		let mut chains: Vec<String> = Vec::new();
		let mut reason_items: Vec<Value> = Vec::new();
		for addr in &peers {
			let url = format!("http://{addr}/ask_retrieve");
			let resp = match st.client.post(&url).json(&reqbody).send().await {
				Ok(r) => r,
				Err(_) => continue,
			};
			if let Ok(v) = resp.json::<Value>().await {
				if let Some(ct) = v.get("chain_text").and_then(Value::as_str) {
					if !ct.trim().is_empty() { chains.push(ct.to_string()); }
				}
				if let Some(rs) = v.get("reasons").and_then(Value::as_array) {
					for r in rs {
						let mut r = r.clone();
						let rid = r.get("id").and_then(Value::as_str).map(|s| s.to_string());
						if let (Some(o), Some(rid)) = (r.as_object_mut(), rid) {
							o.insert("id".into(), json!(format!("{addr}|{rid}")));
							if let Some(f) = o.get("from").and_then(Value::as_str).map(|s| s.to_string()) {
								o.insert("from".into(), json!(format!("{addr}|{f}")));
							}
							if let Some(t) = o.get("to").and_then(Value::as_str).map(|s| s.to_string()) {
								o.insert("to".into(), json!(format!("{addr}|{t}")));
							}
						}
						reason_items.push(r);
					}
				}
				tagged.push((format!("{addr}|"), json!({ "hits": v.get("sources").cloned().unwrap_or(json!([])) })));
			}
		}
		let mut merged = rank_peers(&tagged, k);
		for (n, s) in merged.iter_mut().enumerate() {
			if let Some(o) = s.as_object_mut() { o.insert("n".into(), json!(n + 1)); }
		}
		yield Ok(Event::default().event("sources").data(json!({ "entities": merged, "chains": chains, "reasons": reason_items }).to_string()));

		// Build initial message list with system prompt + history + context
		let user_prompt = build_ask_prompt(&merged, &chains, &q);
		let mut messages: Vec<(String, String)> = vec![("system".to_string(), build_agent_system_prompt().to_owned())];
		for t in body.history.iter().rev().take(6).rev() {
			let role = match t.role.as_str() {
				"assistant" | "system" => t.role.clone(),
				_ => "user".to_string(),
			};
			messages.push((role, t.content.clone()));
		}
		messages.push(("user".to_string(), user_prompt));

		const MAX_ITERS: usize = 8;
		let mut tool_idx = 0usize;

		for _iter in 0..MAX_ITERS {
			// Collect full response so we can detect tool calls before emitting anything.
			let mut response = String::new();
			let mut gen = Box::pin(st.llm.answer(crate::llm::AnswerParams {
				messages: messages.clone(),
				stream: false,
				num_predict: None,
			}));
			let mut had_err = false;
			while let Some(item) = gen.next().await {
				match item {
					Ok(tok) => response.push_str(&tok),
					Err(e) => {
						yield Ok(Event::default().event("error").data(json!({ "message": e.to_string() }).to_string()));
						had_err = true;
						break;
					}
				}
			}
			if had_err { return; }

			let (visible, tool_calls) = extract_tool_calls(&response);

			// Emit visible text (text outside any tool call blocks)
			if !visible.is_empty() {
				yield Ok(Event::default().event("token").data(json!({ "t": visible }).to_string()));
			}

			if tool_calls.is_empty() {
				break;
			}

			// Execute each tool call and emit events
			let mut result_ctx = String::new();
			for tc in &tool_calls {
				let idx = tool_idx;
				tool_idx += 1;
				yield Ok(Event::default().event("tool_call").data(
					json!({ "name": tc.name, "args": tc.args, "idx": idx }).to_string()
				));
				let (ok, result_text) = exec_tool(&st.client, &peers, &tc.name, &tc.args).await;
				yield Ok(Event::default().event("tool_result").data(
					json!({ "name": tc.name, "result": result_text, "ok": ok, "idx": idx }).to_string()
				));
				result_ctx.push_str(&format!("Tool `{}` result: {}\n", tc.name, result_text));
			}

			// Feed tool results back and continue
			messages.push(("assistant".to_string(), response));
			messages.push(("user".to_string(), format!("{result_ctx}Continue.")));
		}

		yield Ok(Event::default().event("done").data("{}"));
	};
	Sse::new(stream)
}

/// Hub endpoint: broadcast a tool call to all live peers, return first success.
async fn hub_tool(State(st): State<HubState>, Json(body): Json<Value>) -> Json<Value> {
	let peers = live_peers();
	if peers.is_empty() {
		return Json(json!({ "ok": false, "error": "no daemons available" }));
	}
	let mut last_ok: Option<Value> = None;
	let mut errors: Vec<String> = Vec::new();
	for addr in &peers {
		let url = format!("http://{addr}/tool");
		match st.client.post(&url).timeout(FANOUT_TIMEOUT).json(&body).send().await {
			Ok(r) => match r.json::<Value>().await {
				Ok(v) => {
					if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
						last_ok = Some(v);
					} else {
						errors.push(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string());
					}
				}
				Err(e) => errors.push(e.to_string()),
			},
			Err(e) => errors.push(e.to_string()),
		}
	}
	last_ok.map(Json).unwrap_or_else(|| Json(json!({ "ok": false, "error": errors.join("; ") })))
}

/// Build the generation prompt from merged source texts + per-daemon chain
/// strings. Numbers each fact so the model can cite them as `[n]`, which the
/// browser links back to the source tiles.
fn build_ask_prompt(sources: &[Value], chains: &[String], question: &str) -> String {
	let mut p = String::from("Context from knowledge graph:\n\n");
	// Cap the provenance chains in the PROMPT — format_chains can emit kilobytes
	// (full entity texts repeated across chains), which balloons prompt-eval
	// latency on local CPU models. The full chains still reach the UI via the
	// `sources` event; the model only needs a compact structural hint.
	let joined: String = chains
		.iter()
		.map(|c| c.trim())
		.filter(|c| !c.is_empty())
		.collect::<Vec<_>>()
		.join("\n");
	if !joined.is_empty() {
		let cap = joined.char_indices().nth(800).map(|(i, _)| i).unwrap_or(joined.len());
		p.push_str(&joined[..cap]);
		p.push('\n');
	}
	p.push_str("Relevant facts:\n");
	for (i, s) in sources.iter().enumerate() {
		let text = s.get("text").and_then(Value::as_str).unwrap_or("");
		p.push_str(&format!("{}. {}\n", i + 1, text));
	}
	p.push_str(&format!(
		"\nQuestion: {question}\n\
		 Answer concisely using only the context above. Cite the facts you use \
		 inline as [n] where n is the fact number. Do not restate the context. Be direct."
	));
	p
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn merge_peer_namespaces_ids_and_keeps_links_valid() {
		let payload = json!({
			"nodes": [{ "id": "e1", "kern": "k1", "label": "x" }],
			"links": [{ "source": "e1", "target": "e2", "kind": "Supports" }],
			"kerns": [
				{ "id": "k0", "parent": null, "children": ["k1"] },
				{ "id": "k1", "parent": "k0", "children": [] },
			],
		});
		let (mut n, mut l, mut k) = (Vec::new(), Vec::new(), Vec::new());
		merge_peer("127.0.0.1:7701|", &payload, &mut n, &mut l, &mut k);

		assert_eq!(n[0]["id"], "127.0.0.1:7701|e1");
		assert_eq!(n[0]["kern"], "127.0.0.1:7701|k1");
		// Endpoints carry the same tag, so the edge still resolves post-merge.
		assert_eq!(l[0]["source"], "127.0.0.1:7701|e1");
		assert_eq!(l[0]["target"], "127.0.0.1:7701|e2");
		// Root stays parentless; child parent/children references are re-keyed.
		assert!(k[0]["parent"].is_null());
		assert_eq!(k[0]["children"][0], "127.0.0.1:7701|k1");
		assert_eq!(k[1]["parent"], "127.0.0.1:7701|k0");
	}

	#[test]
	fn merge_peer_tolerates_missing_arrays() {
		let (mut n, mut l, mut k) = (Vec::new(), Vec::new(), Vec::new());
		merge_peer("t|", &json!({}), &mut n, &mut l, &mut k);
		assert!(n.is_empty() && l.is_empty() && k.is_empty());
	}

	#[test]
	fn ask_prompt_numbers_facts_and_requests_citations() {
		let sources = vec![
			json!({ "text": "confidence join uses max" }),
			json!({ "text": "max is monotone" }),
		];
		let chains = vec!["Chain 1:\n  [Entity] conf\n".to_string()];
		let p = build_ask_prompt(&sources, &chains, "how sure are we?");
		assert!(p.contains("1. confidence join uses max"));
		assert!(p.contains("2. max is monotone"));
		assert!(p.contains("Chain 1:"));
		assert!(p.contains("how sure are we?"));
		assert!(p.contains("[n]"));
	}

	#[test]
	fn rank_peers_namespaces_pools_sorts_and_truncates() {
		// Two peers. Each returns entity hits + reason hits with scores.
		let peer_a = json!({
			"hits":    [{ "id": "e1", "kern": "k1", "label": "a", "score": 0.40 }],
			"reasons": [{ "id": "e9", "kern": "k1", "label": "ra", "score": 0.95 }],
		});
		let peer_b = json!({
			"hits":    [{ "id": "e2", "kern": "k2", "label": "b", "score": 0.70 }],
			"reasons": [],
		});
		let tagged = vec![
			("A|".to_string(), peer_a),
			("B|".to_string(), peer_b),
		];
		let out = rank_peers(&tagged, 2);

		// Truncated to k=2, sorted by score desc across BOTH peers and BOTH arrays.
		assert_eq!(out.len(), 2);
		assert_eq!(out[0]["score"], 0.95);
		assert_eq!(out[1]["score"], 0.70);
		// ids + kern are namespaced by peer tag so they match what /graph shipped.
		assert_eq!(out[0]["id"], "A|e9");
		assert_eq!(out[0]["kern"], "A|k1");
		assert_eq!(out[1]["id"], "B|e2");
	}
}

#[derive(serde::Deserialize)]
struct EditBody {
	id: String,
	text: String,
	#[serde(default)]
	kind: String,
}

/// Peer endpoint: execute a kern MCP tool locally and return the result.
async fn local_tool(State(st): State<LocalState>, Json(body): Json<ToolBody>) -> Json<Value> {
	match st.mcp.call_tool(&body.name, &body.args) {
		Ok(r) => {
			let text = r.content.iter()
				.filter_map(|c: &Value| c.get("text").and_then(Value::as_str))
				.collect::<Vec<_>>()
				.join("\n");
			Json(json!({ "ok": !r.is_error, "result": text }))
		}
		Err(e) => Json(json!({ "ok": false, "error": format!("{e:?}") })),
	}
}

#[derive(serde::Deserialize)]
struct ToolBody {
	name: String,
	#[serde(default)]
	args: Value,
}

/// Peer endpoint: edit an entity or reason by id, mark dirty, enqueue reembed + persist.
async fn edit(State(st): State<LocalState>, Json(body): Json<EditBody>) -> Json<Value> {
	let is_reason = body.kind == "reason";
	let kern_id = {
		let g = read_recovered(&st.graph);
		if is_reason {
			g.kern_of_reason(&body.id).map(|s| s.to_string())
		} else {
			g.kern_of_entity(&body.id).map(|s| s.to_string())
		}
	};
	let Some(kern_id) = kern_id else {
		return Json(json!({ "ok": false, "error": "not found" }));
	};
	{
		let mut g = write_recovered(&st.graph);
		if let Some(k) = g.get_mut(&kern_id) {
			if is_reason {
				if let Some(r) = k.reasons.get_mut(&body.id) {
					r.set_text(body.text.clone());
				}
			} else if let Some(e) = k.entities.get_mut(&body.id) {
				e.set_text(body.text.clone());
			}
		}
	}
	st.queue.enqueue(task(TaskKind::Reembed, &kern_id));
	st.queue.enqueue(task(TaskKind::Persist, &kern_id));
	Json(json!({ "ok": true }))
}

/// Hub endpoint: forward an edit to the peer that owns the namespaced id.
async fn hub_edit(State(st): State<HubState>, Json(mut body): Json<Value>) -> Json<Value> {
	let id = body.get("id").and_then(Value::as_str).unwrap_or("").to_string();
	let Some((addr, real)) = id.split_once('|') else {
		return Json(json!({ "ok": false, "error": "bad id" }));
	};
	if let Some(o) = body.as_object_mut() {
		o.insert("id".into(), json!(real));
	}
	let url = format!("http://{addr}/edit");
	match st.client.post(&url).json(&body).send().await {
		Ok(r) => match r.json::<Value>().await {
			Ok(v) => Json(v),
			Err(_) => Json(json!({ "ok": false, "error": "peer decode" })),
		},
		Err(_) => Json(json!({ "ok": false, "error": "peer unreachable" })),
	}
}

/// Snapshot the live graph as `{nodes, links, kerns}`. Nodes are entities
/// (id, truncated text, kind, kern, heat, confidence); links are reason edges.
/// Edges whose endpoints are not both present (e.g. into an unloaded kern) are
/// dropped so the client never sees a dangling link.
async fn graph_json(State(st): State<LocalState>) -> Json<serde_json::Value> {
	let g = st.graph;
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
					"id": r.id,
					"source": r.from,
					"target": r.to,
					"kind": format!("{:?}", r.kind),
					"text": truncate(&r.text, 80),
					"score": r.score,
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
				"label": if k.anchor_text.trim().is_empty() { "(unnamed)".to_string() } else { truncate(&k.anchor_text, 60) },
				"named": !k.anchor_text.trim().is_empty(),
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
