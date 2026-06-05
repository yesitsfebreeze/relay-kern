use serde::{Deserialize, Serialize};

/// Configuration for Claude-Code memory capture + recall.
///
/// OFF by default. Opt in via a `[capture]` section in `.kern/kern.toml`:
///
/// ```toml
/// [capture]
/// enabled = true
/// ```
///
/// `dir` and `digest_path` are intentionally **cwd-relative and independent
/// of `data_dir`**: the Claude-Code hooks (`kern-capture.mjs`,
/// `kern-recall.mjs`) resolve these paths from the session cwd and have no
/// knowledge of kern's `data_dir`. Do not derive them from `data_dir` — that
/// would break the hook contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
	/// Master switch for the capture_spool + digest tasks.
	pub enabled: bool,
	/// Spool directory (relative to cwd) the Stop hook writes deltas into.
	pub dir: String,
	/// How often the spool is drained, in seconds.
	pub poll_secs: u64,
	/// Output path (relative to cwd) for the recall digest.
	pub digest_path: String,
	/// How often the digest is regenerated, in seconds.
	pub digest_secs: u64,
	/// Max thoughts included in the digest.
	pub digest_k: usize,
	/// Trust floor for digest inclusion: posterior confidence mean
	/// (`conf_mean`, the Beta-Bernoulli expectation) a claim must clear to be
	/// re-injected into the SessionStart digest. The digest is a persistent
	/// re-injection surface — a poisoned/low-trust claim that lands here is
	/// replayed into every future session. Gating it here quarantines
	/// low-trust and repeatedly-contradicted claims (whose `conf_beta` has
	/// grown, dragging the mean down) out of that surface. Set to `0.0` to
	/// disable the gate. Default `0.35`.
	pub digest_min_trust: f32,
	/// Approximate token budget for the digest body. Claims are added best-first
	/// (heat × confidence) until this many tokens are used, trimmed greedily —
	/// context rot means attention degrades with length, so a tight budget beats
	/// a long dump. `digest_k` still caps item count. `0` disables the token cap.
	/// Default `1500`.
	pub digest_token_budget: usize,
	/// Retention window, in seconds, for archived deltas under `<dir>/done/`.
	/// The graph is the durable copy after ingest; the archive is only a
	/// transient audit trail, so it is swept each drain cycle and entries older
	/// than this are deleted to bound disk/inode growth. Default 7 days.
	pub done_retention_secs: u64,
}

impl Default for CaptureConfig {
	fn default() -> Self {
		Self {
			enabled: false,
			dir: ".kern/capture".into(),
			poll_secs: 5,
			digest_path: ".kern/digest.md".into(),
			digest_secs: 30,
			digest_k: 40,
			digest_min_trust: 0.35,
			digest_token_budget: 1500,
			done_retention_secs: 7 * 24 * 60 * 60,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn defaults_are_off_with_sane_tunables() {
		let c = CaptureConfig::default();
		assert!(!c.enabled);
		assert_eq!(c.dir, ".kern/capture");
		assert_eq!(c.poll_secs, 5);
		assert_eq!(c.digest_path, ".kern/digest.md");
		assert_eq!(c.digest_secs, 30);
		assert_eq!(c.digest_k, 40);
		assert_eq!(c.digest_min_trust, 0.35);
		assert_eq!(c.digest_token_budget, 1500);
		assert_eq!(c.done_retention_secs, 7 * 24 * 60 * 60);
	}
}
