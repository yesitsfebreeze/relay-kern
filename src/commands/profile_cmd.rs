use std::sync::Arc;
use std::time::Instant;

use crate::base::search::search_all_unlocked;
use crate::profile::{render_timeline, Profile};
use crate::retrieval::seed::Mode;

use super::{load_graph, Client, Endpoint};

const TIMELINE_WIDTH: usize = 40;

/// Small fixed conversation so distill timing is comparable across runs.
const DISTILL_SAMPLE: &str = "User: The deploy failed because the config pointed at the staging \
	bucket. Assistant: Fixed — the bucket name is now anchored to the environment, so production \
	reads prod-artifacts and staging keeps its own.";

fn ms(t: Instant) -> f64 {
	t.elapsed().as_secs_f64() * 1000.0
}

fn flat(name: &str, total_ms: f64) -> Profile {
	Profile {
		name: name.to_string(),
		checkpoints: Vec::new(),
		total_ms,
	}
}

fn renamed(mut p: Profile, name: &str) -> Profile {
	p.name = name.to_string();
	p
}

/// Time every hot path against the live graph and print a scaled timeline.
/// Read-only: nothing is persisted, so it is safe to run next to a daemon.
pub(super) async fn cmd_profile(cfg: &crate::config::Config, text: &str, no_llm: bool) {
	let mut profiles: Vec<Profile> = Vec::new();

	let t = Instant::now();
	let g = load_graph(cfg);
	profiles.push(flat("load graph", ms(t)));
	let kerns = g.kerns.len();
	let mut entities = 0usize;
	for k in g.all() {
		entities += k.entities.len();
	}

	let reason_url = cfg.reason_url().to_string();
	let llm_client = Client::new(
		Endpoint::new(&reason_url, &cfg.reason.model, cfg.reason_key()),
		Endpoint::new(cfg.answer_url(), &cfg.answer.model, cfg.answer_key()),
		Endpoint::new(&cfg.embed.url, &cfg.embed.model, &cfg.embed.key),
	);

	// Cold vs warm split: the first embed may pay an Ollama model (re)load,
	// the second is the steady-state cost every later stage actually sees.
	let t = Instant::now();
	let qvec = match llm_client.embed(text).await {
		Ok(v) => v,
		Err(e) => {
			eprintln!("embed: {e} (embed endpoint up at {}?)", cfg.embed.url);
			return;
		}
	};
	profiles.push(flat("embed (cold)", ms(t)));

	let t = Instant::now();
	let _ = llm_client.embed(text).await;
	profiles.push(flat("embed (warm)", ms(t)));

	let t = Instant::now();
	let hits = search_all_unlocked(&g, &qvec, 10);
	profiles.push(flat(&format!("vector search ({} hits)", hits.len()), ms(t)));

	for (mode, label) in [
		(Mode::Content, "query content (no llm)"),
		(Mode::Reason, "query reason (no llm)"),
		(Mode::Hybrid, "query hybrid (no llm)"),
	] {
		let (_, p) = crate::retrieval::answer::query_profiled(
			&g,
			&cfg.retrieval,
			&qvec,
			text,
			mode,
			None,
			None,
			None,
		);
		profiles.push(renamed(p, label));
	}

	if no_llm || reason_url.is_empty() {
		if !no_llm {
			eprintln!("no reason endpoint configured; skipping llm stages");
		}
	} else {
		let complete = llm_client.complete_func();
		let llm_fn: crate::retrieval::LlmFunc = Arc::new(llm_client.complete_func());
		let embed_fn: crate::retrieval::EmbedFunc = {
			let c = llm_client.clone();
			Arc::new(move |t: &str| {
				let c = c.clone();
				let t = t.to_string();
				match crate::llm::block_on_in_place(c.embed(&t)) {
					Some(r) => r.map_err(|e| e.to_string()),
					None => Err("no runtime".to_string()),
				}
			})
		};

		let (_, p) = crate::retrieval::answer::query_profiled(
			&g,
			&cfg.retrieval,
			&qvec,
			text,
			Mode::Hybrid,
			Some(&llm_fn),
			Some(&embed_fn),
			None,
		);
		profiles.push(renamed(p, "query hybrid (llm)"));

		let t = Instant::now();
		let claims = crate::ingest::distill::distill(DISTILL_SAMPLE, &complete);
		let n = claims.map(|c| c.len()).unwrap_or(0);
		profiles.push(flat(&format!("distill ({n} claims)"), ms(t)));
	}

	let t = Instant::now();
	let digest = crate::retrieval::digest::build_digest(
		&g,
		cfg.capture.digest_k,
		cfg.capture.digest_min_trust as f64,
		cfg.capture.digest_token_budget,
	);
	profiles.push(flat(&format!("digest build ({} bytes)", digest.len()), ms(t)));

	println!("kern profile — {kerns} kerns, {entities} entities, query: {text:?}");
	println!();
	print!("{}", render_timeline(&profiles, TIMELINE_WIDTH));
}
