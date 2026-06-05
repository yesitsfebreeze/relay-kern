//! `tracing-subscriber` layer that forwards every event into the shared
//! journal as a `Kind::Log` entry. Install in every binary that uses
//! `tracing::*` macros so logs end up alongside RPC + turn events.

use std::fmt;

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

use crate::{Entry, Kind};

pub struct JournalTracingLayer {
	pub source: &'static str,
}

impl JournalTracingLayer {
	pub fn new(source: &'static str) -> Self {
		Self { source }
}
}

impl<S: Subscriber> Layer<S> for JournalTracingLayer {
	fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
		let meta = event.metadata();
		let mut v = FieldRecorder::default();
		event.record(&mut v);
		let payload = serde_json::json!({
			"src": self.source,
			"target": meta.target(),
			"level": meta.level().to_string(),
			"msg": v.finish(),
		});
		crate::emit(Entry::new(Kind::Log, meta.target(), payload));
}
}

/// Generic `tracing` field visitor that captures the `message` field (the
/// most common case) and falls back to the first non-message field's
/// `name=value` rendering. Shared by [`JournalTracingLayer`] and any other
/// tracing layer that wants the same single-line summary semantics.
#[derive(Default)]
pub struct FieldRecorder {
	pub message: Option<String>,
	pub fallback: Option<String>,
}

impl FieldRecorder {
	/// Consume the recorder and return `message` if present, else `fallback`,
	/// else an empty string.
	pub fn finish(self) -> String {
		self.message.or(self.fallback).unwrap_or_default()
	}
}

impl Visit for FieldRecorder {
	fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
		if field.name() == "message" {
			self.message = Some(format!("{value:?}").trim_matches('"').to_string());
		} else if self.fallback.is_none() {
			self.fallback = Some(format!("{}={:?}", field.name(), value));
		}
}
	fn record_str(&mut self, field: &Field, value: &str) {
		if field.name() == "message" {
			self.message = Some(value.to_string());
		} else if self.fallback.is_none() {
			self.fallback = Some(format!("{}={}", field.name(), value));
		}
}
}
