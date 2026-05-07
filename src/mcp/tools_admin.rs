use serde::Deserialize;

use crate::base::locks::{read_recovered, write_recovered};

use super::{tool_error, tool_result_json, Server};

impl Server {
	pub(crate) fn tool_health(&self) -> serde_json::Value {
		tool_result_json(&self.health_stats())
	}

	pub(crate) fn tool_purpose(&self, args: &serde_json::Value) -> serde_json::Value {
		#[derive(Deserialize, Default)]
		struct PurposeArgs {
			#[serde(default)]
			text: String,
		}

		let p: PurposeArgs = serde_json::from_value(args.clone()).unwrap_or_default();

		if p.text.is_empty() {
			let g = read_recovered(&self.graph);
			let purpose = if g.root.purpose_text.is_empty() {
				"(unset)".to_string()
			} else {
				g.root.purpose_text.clone()
			};
			return tool_result_json(&serde_json::json!({"purpose": purpose}));
		}

		let vec = match &self.llm {
			Some(llm) => {
				let Some(handle) = tokio::runtime::Handle::try_current().ok() else {
					return tool_error("no tokio runtime");
				};
				match tokio::task::block_in_place(|| handle.block_on(llm.embed(&p.text))) {
					Ok(v) => v,
					Err(e) => return tool_error(&format!("embed failed: {e}")),
				}
			}
			None => return tool_error("no embed client configured"),
		};

		let mut g = write_recovered(&self.graph);
		g.root.purpose_text = p.text.clone();
		g.root.purpose_vec = vec;
		g.root.inner_radius = crate::base::constants::KERN_INNER_RADIUS;
		g.root.outer_radius = crate::base::constants::KERN_OUTER_RADIUS;
		drop(g);
		(self.save_fn)();

		tool_result_json(&serde_json::json!({"purpose": p.text}))
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
