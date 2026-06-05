use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 5;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
	// Turn-level (agnt-originating).
	User,
	Assistant,
	Final,
	TurnStart,
	TurnEnd,
	Usage,
	ToolCall,
	RecipeInvoke,
	PluginCall,
	Error,
	Ask,
	Answer,
	Goal,
	GoalSnapshot,
	Milestone,
	// Adjust mode: rewrite of a prior message and conversation forks
	// taken from a chosen point in history. Carry payload data inline
	// so consumers can replay journal slices without joining `payload`.
	Edit { target_ts_ms: u64, new_text: String },
	Fork { from_ts_ms: u64, new_fork_id: String },
	// Cross-process plumbing. Emitted by the tarpc client/server middleware
	// and ad-hoc by any binary that wants to surface activity in the
	// shared journal.
	RpcSend,
	RpcRecv,
	RpcError,
	Log,
	// Slice G — rolling plan. `PlanStep` is the canonical step record; the
	// orchestrator emits these as it builds the plan. `PlanProposal` is a
	// sub-agent suggestion awaiting orchestrator review. Payloads carry the
	// `PlanStepPayload` / `PlanProposalPayload` structs from `events.rs`.
	PlanStep,
	PlanProposal,
	// Slice I — recents MRU. Emitted whenever a user/agent action touches
	// an entity (`Open|Drill|Mention|AgentRead|AgentWrite|FsWrite`). The
	// a client replays these on cold start to seed its in-memory MRU
	// ring. Payload schema lives in `events::EntityTouchedPayload`.
	EntityTouched,
	// Slice K — agnt fork lifecycle. `ForkOpen` marks fork creation (with
	// optional parent fork id), `ForkResume` marks rehydration after pause,
	// `ForkClose` marks termination. The kern `SessionMirror` tails these
	// to ingest each fork as a `Document` entity with
	// `source = Source::Session { session_id = fork_id }` so sessions are
	// searchable via the `:session` facet. Distinct
	// from the existing `Fork` variant (which records a branch-from-history
	// point during `Edit`/adjust-mode rewrites and carries a `from_ts_ms`).
	ForkOpen { fork_id: String, parent: Option<String> },
	ForkResume { fork_id: String },
	ForkClose { fork_id: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entry {
	pub v: u32,
	pub ts_ms: u64,
	pub kind: Kind,
	pub key: String,
	pub payload: serde_json::Value,
}

impl Entry {
	pub fn new(kind: Kind, key: impl Into<String>, payload: serde_json::Value) -> Self {
		Self {
			v: SCHEMA_VERSION,
			ts_ms: now_ms(),
			kind,
			key: key.into(),
			payload,
		}
}
}

pub trait Sink: Send + Sync {
	fn emit(&self, entry: Entry);
}

pub struct NullSink;

impl Sink for NullSink {
	fn emit(&self, _entry: Entry) {

}
}

pub fn now_ms() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_millis() as u64)
		.unwrap_or(0)
}
