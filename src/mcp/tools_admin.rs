use serde::Deserialize;

use crate::base::locks::{read_recovered, write_recovered};

use super::{tool_error, tool_result_json, Server};

impl Server {
	pub(crate) fn tool_health(&self) -> serde_json::Value {
		tool_result_json(&self.health_stats())
	}

	pub(crate) fn tool_anchor(&self, args: &serde_json::Value) -> serde_json::Value {
		#[derive(Deserialize, Default)]
		struct AnchorArgs {
			#[serde(default)]
			action: String,
			#[serde(default)]
			name: String,
			#[serde(default)]
			text: String,
		}

		let p: AnchorArgs = serde_json::from_value(args.clone()).unwrap_or_default();
		let action = if p.action.is_empty() {
			"list"
		} else {
			p.action.as_str()
		};

		match action {
			"list" => {
				let g = read_recovered(&self.graph);
				let anchors: Vec<serde_json::Value> = crate::base::accept::root_anchor_ids(&g)
					.iter()
					.filter_map(|cid| g.loaded(cid))
					.map(|c| {
						serde_json::json!({
							"name": c.anchor_text,
							"thoughts": c.entities.len(),
							"reasons": c.reasons.len(),
						})
					})
					.collect();
				tool_result_json(&serde_json::json!({ "anchors": anchors }))
			}
			"add" => {
				if p.name.is_empty() || p.text.is_empty() {
					return tool_error("add requires name and text");
				}
				let vec = match &self.llm {
					Some(llm) => match crate::llm::block_on_in_place(llm.embed(&p.text)) {
						Some(Ok(v)) => v,
						Some(Err(e)) => return tool_error(&format!("embed failed: {e}")),
						None => return tool_error("no tokio runtime"),
					},
					None => return tool_error("no embed client configured"),
				};
				let mut g = write_recovered(&self.graph);
				crate::base::accept::add_anchor(&mut g, &p.name, vec);
				drop(g);
				(self.save_fn)();
				tool_result_json(&serde_json::json!({ "added": p.name }))
			}
			"remove" | "rm" => {
				if p.name.is_empty() {
					return tool_error("remove requires name");
				}
				let mut g = write_recovered(&self.graph);
				let removed = crate::base::accept::remove_anchor(&mut g, &p.name);
				drop(g);
				if removed {
					(self.save_fn)();
					tool_result_json(&serde_json::json!({ "removed": p.name }))
				} else {
					tool_error(&format!("anchor not found: {}", p.name))
				}
			}
			_ => tool_error("action must be add, list, or remove"),
		}
	}

	pub(crate) fn tool_descriptor(&self, args: &serde_json::Value) -> serde_json::Value {
		#[derive(Deserialize)]
		struct DescArgs {
			action: String,
			name: String,
			#[serde(default)]
			description: String,
		}

		let p: DescArgs = match serde_json::from_value(args.clone()) {
			Ok(v) => v,
			Err(e) => return tool_error(&format!("invalid arguments: {e}")),
		};

		match p.action.as_str() {
			"add" => {
				if p.description.is_empty() {
					return tool_error("description required for add");
				}
				let mut g = write_recovered(&self.graph);
				g.root.descriptors.insert(p.name.clone(), p.description);
				drop(g);
				(self.save_fn)();
				tool_result_json(&serde_json::json!({"added": p.name}))
			}
			"rm" => {
				let mut g = write_recovered(&self.graph);
				g.root.descriptors.remove(&p.name);
				drop(g);
				(self.save_fn)();
				tool_result_json(&serde_json::json!({"removed": p.name}))
			}
			_ => tool_error("action must be add or rm"),
		}
	}

	pub(crate) fn tool_pulse(&self, args: &serde_json::Value) -> serde_json::Value {
		#[derive(Deserialize, Default)]
		struct PulseArgs {
			#[serde(default)]
			strength: f64,
		}

		let p: PulseArgs = serde_json::from_value(args.clone()).unwrap_or_default();
		let strength = if p.strength <= 0.0 { 1.0 } else { p.strength };

		let q = match &self.task_q {
			Some(q) => q,
			None => return tool_result_json(&serde_json::json!({"enqueued": 0})),
		};

		let mut g = write_recovered(&self.graph);
		let root_id = g.root.id.clone();
		crate::tick::pulse::pulse(q, &mut g, &root_id, strength);
		drop(g);

		tool_result_json(&serde_json::json!({"status": "pulsed", "strength": strength}))
	}
}

#[cfg(test)]
mod descriptor_tests {
	use std::sync::{
		atomic::{AtomicUsize, Ordering},
		Arc, RwLock,
	};

	use crate::base::graph::GraphGnn;
	use crate::base::locks::read_recovered;
	use crate::config::Config;
	use crate::llm;
	use crate::mcp::Server;

	fn make_server() -> (Server, Arc<AtomicUsize>) {
		let graph = Arc::new(RwLock::new(GraphGnn::new()));
		let counter = Arc::new(AtomicUsize::new(0));
		let c2 = counter.clone();
		let embedder = llm::Client::new_embed_only("http://127.0.0.1:1", "test");
		let worker = Arc::new(crate::ingest::Worker::new(graph.clone(), embedder, None, None));
		let server = Server {
			graph,
			worker,
			llm: None,
			save_fn: Arc::new(move || {
				c2.fetch_add(1, Ordering::SeqCst);
			}),
			task_q: None,
			cfg: Arc::new(Config::default()),
			cache: crate::retrieval::cache::QueryCache::default_shared(),
		};
		(server, counter)
	}

	fn text(v: &serde_json::Value) -> String {
		v["content"][0]["text"].as_str().unwrap_or("").to_string()
	}

	fn is_error(v: &serde_json::Value) -> bool {
		v.get("isError").and_then(|x| x.as_bool()).unwrap_or(false)
	}

	#[tokio::test]
	async fn add_inserts_descriptor_and_calls_save() {
		let (srv, counter) = make_server();
		let out = srv.tool_descriptor(
			&serde_json::json!({"action": "add", "name": "code", "description": "source code snippets"}),
		);
		assert!(!is_error(&out));
		let body: serde_json::Value = serde_json::from_str(&text(&out)).unwrap();
		assert_eq!(body["added"], "code");
		assert_eq!(counter.load(Ordering::SeqCst), 1);
		let g = read_recovered(&srv.graph);
		assert_eq!(g.root.descriptors.get("code").map(String::as_str), Some("source code snippets"));
	}

	#[tokio::test]
	async fn add_empty_description_returns_error_no_save() {
		let (srv, counter) = make_server();
		let out = srv
			.tool_descriptor(&serde_json::json!({"action": "add", "name": "code", "description": ""}));
		assert!(is_error(&out));
		assert!(text(&out).contains("description required"));
		assert_eq!(counter.load(Ordering::SeqCst), 0);
	}

	#[tokio::test]
	async fn add_missing_required_field_returns_deser_error() {
		let (srv, _) = make_server();
		// `name` is required; omitting it triggers serde error path
		let out = srv.tool_descriptor(&serde_json::json!({"action": "add"}));
		assert!(is_error(&out));
		assert!(text(&out).contains("invalid arguments"));
	}

	#[tokio::test]
	async fn rm_removes_existing_descriptor_and_calls_save_twice() {
		let (srv, counter) = make_server();
		srv.tool_descriptor(
			&serde_json::json!({"action": "add", "name": "notes", "description": "markdown notes"}),
		);
		let out = srv.tool_descriptor(&serde_json::json!({"action": "rm", "name": "notes"}));
		assert!(!is_error(&out));
		let body: serde_json::Value = serde_json::from_str(&text(&out)).unwrap();
		assert_eq!(body["removed"], "notes");
		assert_eq!(counter.load(Ordering::SeqCst), 2);
		let g = read_recovered(&srv.graph);
		assert!(!g.root.descriptors.contains_key("notes"));
	}

	#[tokio::test]
	async fn rm_nonexistent_is_noop_but_still_calls_save() {
		let (srv, counter) = make_server();
		let out = srv.tool_descriptor(&serde_json::json!({"action": "rm", "name": "ghost"}));
		assert!(!is_error(&out));
		let body: serde_json::Value = serde_json::from_str(&text(&out)).unwrap();
		assert_eq!(body["removed"], "ghost");
		assert_eq!(counter.load(Ordering::SeqCst), 1);
	}

	#[tokio::test]
	async fn unknown_action_returns_error() {
		let (srv, _) = make_server();
		let out = srv.tool_descriptor(&serde_json::json!({"action": "list", "name": "x"}));
		assert!(is_error(&out));
		assert!(text(&out).contains("action must be add or rm"));
	}
}
