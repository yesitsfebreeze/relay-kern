use futures_util::StreamExt as _;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
	#[error("HTTP error: {0}")]
	Http(#[from] reqwest::Error),
	#[error("API error ({status}): {body}")]
	Api { status: u16, body: String },
	#[error("empty embedding response")]
	EmptyEmbedding,
	#[error("empty completion response")]
	EmptyCompletion,
}

pub fn is_transient(err: &LlmError) -> bool {
	match err {
		LlmError::Http(e) => {
			if e.is_timeout() || e.is_connect() {
				return true;
			}
			if let Some(s) = e.status() {
				return s.as_u16() >= 500 || s.as_u16() == 429;
			}
			true
		}
		LlmError::Api { status, .. } => *status >= 500 || *status == 429,
		_ => false,
	}
}

/// Whether a failed batch embed warrants a second attempt as a single embed.
/// Only batch-specific failures qualify — transient network/5xx/429 errors, or
/// an empty batch response. A permanent client error (e.g. 400 bad model, 401
/// auth) fails identically as a single call, so it is propagated instead, which
/// lets [`embed_with_retry`](crate::ingest) short-circuit rather than pay a
/// second HTTP round-trip per chunk.
fn should_retry_single(err: &LlmError) -> bool {
	is_transient(err) || matches!(err, LlmError::EmptyEmbedding)
}

#[derive(Clone)]
pub struct Client {
	inner: Arc<Inner>,
}

struct Inner {
	reason_url: String,
	reason_model: String,
	reason_headers: HeaderMap,
	answer_url: String,
	answer_model: String,
	answer_headers: HeaderMap,
	embed_url: String,
	embed_model: String,
	embed_headers: HeaderMap,
	http: reqwest::Client,
}

impl Client {
	#[allow(clippy::too_many_arguments)]
	pub fn new(
		reason_url: &str,
		reason_model: &str,
		reason_key: &str,
		answer_url: &str,
		answer_model: &str,
		answer_key: &str,
		embed_url: &str,
		embed_model: &str,
		embed_key: &str,
	) -> Self {
		let embed_url = if embed_url.is_empty() {
			reason_url
		} else {
			embed_url
		};
		let embed_key = if embed_key.is_empty() {
			reason_key
		} else {
			embed_key
		};
		// Answer endpoint falls back to reason when unset — the single-Ollama
		// case where only the model differs. Empty model would 400 on /ask, so
		// fall back there too rather than send a blank model name.
		let answer_url = if answer_url.is_empty() {
			reason_url
		} else {
			answer_url
		};
		let answer_key = if answer_key.is_empty() {
			reason_key
		} else {
			answer_key
		};
		let answer_model = if answer_model.is_empty() {
			reason_model
		} else {
			answer_model
		};
		let normalize = |u: &str| {
			let u = u.trim_end_matches('/');
			u.strip_suffix("/v1").unwrap_or(u).to_string()
		};
		let http = reqwest::Client::builder()
			.timeout(Duration::from_secs(120))
			.build()
			.expect("failed to build HTTP client");
		Self {
			inner: Arc::new(Inner {
				reason_url: normalize(reason_url),
				reason_model: reason_model.to_string(),
				reason_headers: make_headers(reason_key),
				answer_url: normalize(answer_url),
				answer_model: answer_model.to_string(),
				answer_headers: make_headers(answer_key),
				embed_url: normalize(embed_url),
				embed_model: embed_model.to_string(),
				embed_headers: make_headers(embed_key),
				http,
			}),
		}
	}

	pub fn new_embed_only(embed_url: &str, embed_model: &str) -> Self {
		Self::new("", "", "", "", "", "", embed_url, embed_model, "")
	}

	pub async fn embed(&self, text: &str) -> Result<Vec<f64>, LlmError> {
		match self.embed_batch(&[text.to_string()]).await {
			Ok(mut vecs) => {
				if vecs.is_empty() {
					return Err(LlmError::EmptyEmbedding);
				}
				Ok(vecs.swap_remove(0))
			}
			Err(e) if should_retry_single(&e) => self.embed_single(text).await,
			Err(e) => Err(e),
		}
	}

	pub async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f64>>, LlmError> {
		let url = format!("{}/v1/embeddings", self.inner.embed_url);
		let body = EmbedBatchRequest {
			model: &self.inner.embed_model,
			input: texts,
		};
		let resp = self
			.inner
			.http
			.post(&url)
			.headers(self.inner.embed_headers.clone())
			.json(&body)
			.send()
			.await?;
		let status = resp.status().as_u16();
		if status >= 400 {
			let body = resp.text().await.unwrap_or_default();
			return Err(LlmError::Api { status, body });
		}
		let mut parsed: EmbedResponse = resp.json().await?;
		if parsed.data.is_empty() {
			return Err(LlmError::EmptyEmbedding);
		}
		parsed.data.sort_by_key(|d| d.index);
		Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
	}

	async fn embed_single(&self, text: &str) -> Result<Vec<f64>, LlmError> {
		let url = format!("{}/v1/embeddings", self.inner.embed_url);
		let body = EmbedSingleRequest {
			model: &self.inner.embed_model,
			input: text,
		};
		let resp = self
			.inner
			.http
			.post(&url)
			.headers(self.inner.embed_headers.clone())
			.json(&body)
			.send()
			.await?;
		let status = resp.status().as_u16();
		if status >= 400 {
			let body = resp.text().await.unwrap_or_default();
			return Err(LlmError::Api { status, body });
		}
		let parsed: EmbedResponse = resp.json().await?;
		parsed
			.data
			.into_iter()
			.next()
			.map(|d| d.embedding)
			.ok_or(LlmError::EmptyEmbedding)
	}

	pub async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
		let url = format!("{}/v1/chat/completions", self.inner.reason_url);
		let body = ChatRequest {
			model: &self.inner.reason_model,
			messages: vec![ChatMessage {
				role: "user",
				content: prompt,
			}],
		};
		let resp = self
			.inner
			.http
			.post(&url)
			.headers(self.inner.reason_headers.clone())
			.json(&body)
			.send()
			.await?;
		let status = resp.status().as_u16();
		if status >= 400 {
			let body = resp.text().await.unwrap_or_default();
			return Err(LlmError::Api { status, body });
		}
		let parsed: ChatResponse = resp.json().await?;
		parsed
			.choices
			.into_iter()
			.next()
			.map(|c| c.message.content)
			.ok_or(LlmError::EmptyCompletion)
	}

	/// Stream a chat completion as a sequence of content-delta strings.
	/// `messages` is the full multi-turn context (role/content pairs). Yields each
	/// non-empty token delta in order; ends when the server sends `[DONE]` or the
	/// body closes. Errors (HTTP, network) surface as a single `Err` item.
	pub fn complete_stream(
		&self,
		messages: Vec<(String, String)>,
	) -> impl futures_core::Stream<Item = Result<String, LlmError>> + Send {
		let client = self.clone();
		async_stream::stream! {
			// Ollama's NATIVE /api/chat, not the OpenAI-compat /v1: it is the only
			// endpoint that honors `options.num_ctx` and `keep_alive`, both of which
			// the answer path needs to stay GPU-resident (see ANSWER_NUM_CTX). The
			// trade is that the answer endpoint must be Ollama — by design: [answer]
			// is the local small-model glue path. Reason/embed stay on /v1.
			let url = format!("{}/api/chat", client.inner.answer_url);
			let msgs: Vec<serde_json::Value> = messages
				.iter()
				.map(|(r, c)| serde_json::json!({"role": r, "content": c}))
				.collect();
			// `think: false` disables the thinking phase. The answer model (qwen3.5)
			// thinks by default, emitting hidden reasoning tokens before the first
			// visible one — pure latency for a path that only glues already-retrieved
			// graph nodes into prose. Ignored by non-reasoning models.
			let body = serde_json::json!({
				"model": client.inner.answer_model,
				"messages": msgs,
				"stream": true,
				"think": false,
				"keep_alive": ANSWER_KEEP_ALIVE,
				"options": { "num_ctx": ANSWER_NUM_CTX },
			});
			// Override the client's 120s TOTAL timeout: a streamed generation can
			// take far longer to finish than 120s (big RAG prompt + CPU inference),
			// and a total-response timeout would abort it mid-stream and surface as
			// "error decoding response body". 600s is a generous ceiling for slow
			// local models; tokens still stream as they arrive.
			let resp = match client.inner.http.post(&url)
				.headers(client.inner.answer_headers.clone())
				.timeout(Duration::from_secs(600))
				.json(&body)
				.send()
				.await
			{
				Ok(r) => r,
				Err(e) => { yield Err(LlmError::from(e)); return; }
			};
			let status = resp.status().as_u16();
			if status >= 400 {
				let body = resp.text().await.unwrap_or_default();
				yield Err(LlmError::Api { status, body });
				return;
			}
			let mut stream = resp.bytes_stream();
			let mut buf: Vec<u8> = Vec::new();
			while let Some(chunk) = stream.next().await {
				let chunk = match chunk {
					Ok(b) => b,
					Err(e) => { yield Err(LlmError::from(e)); return; }
				};
				buf.extend_from_slice(&chunk);
				// Decode only COMPLETE lines, so a multibyte char split across chunks
				// is never lossily decoded mid-sequence. Each full SSE line is valid UTF-8.
				while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
					let raw: Vec<u8> = buf.drain(..=pos).collect();
					let line = String::from_utf8_lossy(&raw);
					match parse_chat_line(line.trim_end()) {
						Some(ChatDelta::Done) => return,
						Some(ChatDelta::Token(t)) if !t.is_empty() => yield Ok(t),
						_ => {}
					}
				}
			}
		}
	}

	/// Touch the answer model so Ollama keeps it GPU-resident. Mirrors the embed
	/// keep-alive: `/ask` is user-facing and a cold reload of qwen3.5:4b costs
	/// multiple seconds before the first token. A 1-token `/api/chat` with the
	/// same `num_ctx` and `keep_alive` as the real stream holds the exact runner
	/// instance, so the next `/ask` reuses it instead of reloading. Errors are
	/// the caller's to ignore — a missed warm just means one slow request.
	pub async fn warm_answer(&self) -> Result<(), LlmError> {
		let url = format!("{}/api/chat", self.inner.answer_url);
		let body = serde_json::json!({
			"model": self.inner.answer_model,
			"messages": [{"role": "user", "content": "warm"}],
			"stream": false,
			"think": false,
			"keep_alive": ANSWER_KEEP_ALIVE,
			"options": { "num_ctx": ANSWER_NUM_CTX, "num_predict": 1 },
		});
		let resp = self
			.inner
			.http
			.post(&url)
			.headers(self.inner.answer_headers.clone())
			.json(&body)
			.send()
			.await?;
		let status = resp.status().as_u16();
		if status >= 400 {
			let body = resp.text().await.unwrap_or_default();
			return Err(LlmError::Api { status, body });
		}
		Ok(())
	}

	pub fn complete_func(&self) -> impl Fn(&str) -> String + Send + Sync + 'static {
		let client = self.clone();
		move |prompt: &str| {
			let client = client.clone();
			let prompt = prompt.to_string();
			match tokio::runtime::Handle::try_current() {
				Ok(handle) => {
					let result = tokio::task::block_in_place(|| handle.block_on(client.complete(&prompt)));
					result.unwrap_or_default()
				}
				Err(_) => String::new(),
			}
		}
	}
}

fn make_headers(key: &str) -> HeaderMap {
	let mut h = HeaderMap::new();
	h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
	if !key.is_empty() {
		if let Ok(v) = HeaderValue::from_str(&format!("Bearer {key}")) {
			h.insert(AUTHORIZATION, v);
		}
	}
	h
}

#[derive(Serialize)]
struct EmbedBatchRequest<'a> {
	model: &'a str,
	input: &'a [String],
}

#[derive(Serialize)]
struct EmbedSingleRequest<'a> {
	model: &'a str,
	input: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
	data: Vec<EmbedDatum>,
}

#[derive(Deserialize)]
struct EmbedDatum {
	embedding: Vec<f64>,
	#[serde(default)]
	index: usize,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
	model: &'a str,
	messages: Vec<ChatMessage<'a>>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
	role: &'a str,
	content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
	choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
	message: ChatChoiceMessage,
}

#[derive(Deserialize)]
struct ChatChoiceMessage {
	content: String,
}

/// Context window for the answer model's `/api/chat` load. The `/ask` path glues
/// already-retrieved graph nodes into a short paragraph — it never needs a large
/// window, and Ollama's default 32k allocates a KV cache big enough to spill a 4b
/// model off an 8 GB GPU onto CPU (~2x slower, and it evicts the embedder). 8192
/// keeps qwen3.5:4b fully GPU-resident alongside the embedder.
const ANSWER_NUM_CTX: u64 = 8192;

/// Keep the answer model resident between requests. `/v1` ignores `keep_alive`;
/// `/api/chat` honors it. Paired with the ~4-min warm ping this holds the model
/// so a user `/ask` never pays a cold reload.
const ANSWER_KEEP_ALIVE: &str = "10m";

#[derive(Debug, PartialEq)]
enum ChatDelta {
	Token(String),
	Done,
}

/// Parse one line of an Ollama `/api/chat` streaming response (NDJSON). Each line
/// is a JSON object `{"message":{"content":"…"},"done":bool}`. `done:true` →
/// `Done`; otherwise the message content → `Token`. Blank lines / parse failures
/// → `None`.
fn parse_chat_line(line: &str) -> Option<ChatDelta> {
	if line.is_empty() {
		return None;
	}
	let v: Value = serde_json::from_str(line).ok()?;
	if v.get("done").and_then(Value::as_bool).unwrap_or(false) {
		return Some(ChatDelta::Done);
	}
	let content = v
		.get("message")
		.and_then(|m| m.get("content"))
		.and_then(Value::as_str)?;
	Some(ChatDelta::Token(content.to_string()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn chat_line_yields_token_then_done() {
		assert_eq!(
			parse_chat_line(r#"{"message":{"role":"assistant","content":"He"},"done":false}"#),
			Some(ChatDelta::Token("He".to_string()))
		);
		assert_eq!(
			parse_chat_line(r#"{"message":{"content":""},"done":true,"done_reason":"stop"}"#),
			Some(ChatDelta::Done)
		);
		assert_eq!(parse_chat_line(""), None);
		assert_eq!(parse_chat_line("not json"), None);
		assert_eq!(parse_chat_line(r#"{"message":{},"done":false}"#), None);
	}

	#[test]
	fn permanent_client_errors_do_not_retry_single() {
		// 400 bad model, 401 auth: a single embed fails identically, so the
		// batch error must propagate (no wasted second round-trip).
		assert!(!should_retry_single(&LlmError::Api {
			status: 400,
			body: String::new()
		}));
		assert!(!should_retry_single(&LlmError::Api {
			status: 401,
			body: String::new()
		}));
		assert!(!should_retry_single(&LlmError::EmptyCompletion));
	}

	#[test]
	fn transient_and_empty_batch_retry_single() {
		assert!(should_retry_single(&LlmError::Api {
			status: 429,
			body: String::new()
		}));
		assert!(should_retry_single(&LlmError::Api {
			status: 503,
			body: String::new()
		}));
		assert!(should_retry_single(&LlmError::EmptyEmbedding));
	}
}
