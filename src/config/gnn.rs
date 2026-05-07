use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct GnnConfig {
	pub self_weight: f64,
	pub min_weight: f64,
	pub min_thoughts: usize,
	pub train_epochs: usize,
	pub train_learning_rate: f64,
}

impl Default for GnnConfig {
	fn default() -> Self {
		Self {
			self_weight: 0.6,
			min_weight: 0.01,
			min_thoughts: 2,
			train_epochs: 24,
			train_learning_rate: 0.01,
		}
	}
}

impl From<GnnConfig> for crate::gnn::propagate::GnnConfig {
	fn from(c: GnnConfig) -> Self {
		crate::gnn::propagate::GnnConfig {
			self_weight: c.self_weight,
			min_weight: c.min_weight,
			min_thoughts: c.min_thoughts,
			train_epochs: c.train_epochs,
			train_learning_rate: c.train_learning_rate,
		}
	}
}
