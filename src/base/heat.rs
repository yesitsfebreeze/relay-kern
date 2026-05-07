use std::time::SystemTime;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct HeatConfig {
	pub half_life_secs: u64,
	pub deposit_access: f32,
	pub deposit_traversal: f32,
}

impl HeatConfig {
	pub fn defaults() -> Self {
		Self {
			half_life_secs: 7 * 24 * 60 * 60,
			deposit_access: 1.0,
			deposit_traversal: 0.5,
		}
	}
}

impl Default for HeatConfig {
	fn default() -> Self {
		Self::defaults()
	}
}

pub fn decayed(heat: f32, since: Option<SystemTime>, now: SystemTime, half_life_secs: u64) -> f32 {
	if heat <= 0.0 {
		return 0.0;
	}
	let Some(since) = since else {
		return heat;
	};
	let dt = match now.duration_since(since) {
		Ok(d) => d.as_secs_f64(),
		Err(_) => return heat,
	};
	let t = (half_life_secs as f64).max(1.0);
	let lambda = std::f64::consts::LN_2 / t;
	(heat as f64 * (-lambda * dt).exp()) as f32
}

pub fn deposit(
	heat: f32,
	since: Option<SystemTime>,
	now: SystemTime,
	half_life_secs: u64,
	deposit: f32,
) -> f32 {
	decayed(heat, since, now, half_life_secs) + deposit
}
