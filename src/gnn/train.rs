

use crate::gnn::graph::Graph;
use crate::gnn::model::Model;
use crate::gnn::optim::Optimizer;
use crate::gnn::tensor::Tensor;
pub type LossFunc = fn(&Tensor, &Tensor) -> f64;

pub type GradFunc = fn(&Tensor, &Tensor) -> Tensor;

pub struct TrainConfig {
	pub epochs: usize,
	pub lr: f64,
	pub log_every: usize, // 0 = never
	pub clip_norm: f64,   // 0 = no clipping
}

impl Default for TrainConfig {
	fn default() -> Self {
		Self {
			epochs: 100,
			lr: 0.01,
			log_every: 10,
			clip_norm: 0.0,
		}
	}
}

#[derive(Debug, Clone)]
pub struct EpochResult {
	pub epoch: usize,
	pub loss: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn train(
	model: &mut Model,
	optim: &mut dyn Optimizer,
	loss_fn: LossFunc,
	grad_fn: GradFunc,
	config: &TrainConfig,
	graph: &Graph,
	features: &Tensor,
	labels: &Tensor,
) -> Vec<EpochResult> {
	let mut results = Vec::with_capacity(config.epochs);

	for epoch in 1..=config.epochs {
		model.zero_grads();

		let predicted = model.forward(graph, features);
		let loss = loss_fn(&predicted, labels);

		let d_out = grad_fn(&predicted, labels);
		model.backward(graph, &d_out);

		if config.clip_norm > 0.0 {
			clip_gradients(model, config.clip_norm);
		}

		{
			let grads: Vec<Tensor> = model.param_grads().iter().map(|t| (*t).clone()).collect();
			let grad_refs: Vec<&Tensor> = grads.iter().collect();
			let mut params = model.parameters_mut();
			optim.step(&mut params, &grad_refs);
		}

		results.push(EpochResult { epoch, loss });
	}
	results
}

pub fn clip_gradients(model: &mut Model, max_norm: f64) {
	let norm_sq: f64 = model
		.param_grads()
		.iter()
		.map(|g| g.data.iter().map(|v| v * v).sum::<f64>())
		.sum();
	let norm = norm_sq.sqrt();
	if norm == 0.0 || norm <= max_norm {
		return;
	}
	let scale = max_norm / norm;
	for g in model.param_grads_mut() {
		g.scale_inplace(scale);
	}
}
