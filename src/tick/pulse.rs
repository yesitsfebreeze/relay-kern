use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::base::constants::{PULSE_DECAY, PULSE_THRESHOLD, STIGMERGY_GC_INTERVAL};
use crate::base::graph::GraphGnn;
use crate::base::heat::{self, HeatConfig};

use super::queue::{task, Queue, TaskKind};

/// Last unix-seconds at which `maybe_enqueue_stigmergy_gc` actually fanned
/// out a sweep. Single-flighted via `compare_exchange` so concurrent pulses
/// can never double-enqueue.
static LAST_GC_AT_SECS: AtomicU64 = AtomicU64::new(0);

pub fn pulse(q: &Queue, g: &mut GraphGnn, kern_id: &str, strength: f64) {
	pulse_with_half_life(q, g, kern_id, strength, HeatConfig::defaults().half_life_secs);
	emit_stigmergy_snapshot(g, kern_id);
	// Below-threshold pulses are no-ops by contract; don't enqueue GC work
	// either. The next above-threshold pulse will trigger the sweep.
	if strength >= PULSE_THRESHOLD {
		maybe_enqueue_stigmergy_gc(q, g);
		maybe_enqueue_reembed(q, g);
	}
}

/// Pure decision: should `maybe_enqueue_stigmergy_gc` fan out a sweep now?
/// Returns true iff `interval` has fully elapsed since the last sweep.
/// `now_secs == 0` (callers couldn't read the clock) returns false.
/// `last_secs > now_secs` (clock skew) returns false — refuse to sweep on a
/// regressed clock to avoid amplifying time travel.
pub fn should_run_gc(now_secs: u64, last_secs: u64, interval: Duration) -> bool {
	if now_secs == 0 || last_secs > now_secs {
		return false;
	}
	now_secs - last_secs >= interval.as_secs()
}

/// Single-flight guard around `should_run_gc`: at most one pulse per
/// `STIGMERGY_GC_INTERVAL` enqueues `StigmergyGc` for every kern. Pending
/// per-kern dedup is owned by `Queue::enqueue` via the existing TaskKey
/// map, so a slow handler can't pile up duplicates either.
fn maybe_enqueue_stigmergy_gc(q: &Queue, g: &GraphGnn) {
	let now_secs = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_secs())
		.unwrap_or(0);
	let last = LAST_GC_AT_SECS.load(Ordering::Relaxed);
	if !should_run_gc(now_secs, last, STIGMERGY_GC_INTERVAL) {
		return;
	}
	if LAST_GC_AT_SECS
		.compare_exchange(last, now_secs, Ordering::AcqRel, Ordering::Relaxed)
		.is_err()
	{
		return;
	}
	for kern_id in g.kerns.keys() {
		q.enqueue(task(TaskKind::StigmergyGc, kern_id));
	}
}

/// Enqueue a `Reembed` sweep for every kern that has a dirty (edited) thought or
/// reason, so edits made directly in the graph get re-embedded even without an
/// explicit trigger (e.g. after a restart). Dedup is owned by `Queue::enqueue`.
fn maybe_enqueue_reembed(q: &Queue, g: &GraphGnn) {
	for (kern_id, k) in g.kerns.iter() {
		let dirty = k.entities.values().any(|e| e.dirty) || k.reasons.values().any(|r| r.dirty);
		if dirty {
			q.enqueue(task(TaskKind::Reembed, kern_id));
		}
	}
}

/// Emit one `kern::metrics` tracing event summarising heat distribution at the
/// pulsed kern. Called from the public `pulse` entrypoint only — never from
/// recursive `pulse_with_half_life` frames — so each pulse tree yields exactly
/// one snapshot.
fn emit_stigmergy_snapshot(g: &GraphGnn, kern_id: &str) {
	let Some(k) = g.kerns.get(kern_id) else {
		return;
	};
	let snap = crate::metrics::snapshot_heat(k.entities.values());
	if snap.n > 0 {
		tracing::info!(
			target: "kern::metrics",
			gini = snap.gini,
			max_heat = snap.max_heat,
			n = snap.n,
			"stigmergy_snapshot"
		);
	}
}

pub fn pulse_with_half_life(
	q: &Queue,
	g: &mut GraphGnn,
	kern_id: &str,
	strength: f64,
	half_life_secs: u64,
) {
	if strength < PULSE_THRESHOLD {
		return;
	}
	let (children, has_thoughts, entity_ids): (Vec<String>, bool, Vec<String>) = {
		let Some(k) = g.kerns.get(kern_id) else {
			return;
		};
		(
			k.children.clone(),
			!k.entities.is_empty(),
			k.entities.keys().cloned().collect(),
		)
	};

	if has_thoughts {
		q.enqueue(task(TaskKind::Cluster, kern_id));
	}

	let deposit = (HeatConfig::defaults().deposit_traversal as f64 * strength) as f32;
	if deposit > 0.0 {
		let now = SystemTime::now();
		if let Some(k) = g.kerns.get_mut(kern_id) {
			for tid in &entity_ids {
				if let Some(t) = k.entities.get_mut(tid) {
					t.heat = heat::deposit(t.heat, t.heat_updated_at, now, half_life_secs, deposit);
					t.heat_updated_at = Some(now);
				}
			}
		}
	}

	let reduced = strength * PULSE_DECAY;
	for child_id in &children {
		pulse_with_half_life(q, g, child_id, reduced, half_life_secs);
	}
}
