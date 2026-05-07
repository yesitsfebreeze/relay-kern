

use crate::gnn::backward::{act_deriv_mul, BackwardGraphLayer, GraphLayer};
use crate::gnn::dropout::Dropout;
use crate::gnn::graph::Graph;
use crate::gnn::layer::{Backward, Layer, LinearLayer};
use crate::gnn::norm::LayerNorm;
use crate::gnn::tensor::Tensor;

pub struct GCNLayer {
	pub linear: LinearLayer,
	pub norm: Option<LayerNorm>,
	pub drop: Option<Dropout>,
	pub act_fn: Option<fn(f64) -> f64>,
	last_norm_adj: Option<Tensor>,
	last_pre_act: Option<Tensor>,
}

impl GCNLayer {
	pub fn new(
		in_features: usize,
		out_features: usize,
		act_fn: Option<fn(f64) -> f64>,
		norm: bool,
		drop_rate: f64,
	) -> Self {
		let mut rng = rand::rng();
		Self::with_rng(in_features, out_features, act_fn, norm, drop_rate, &mut rng)
	}

	/// Construct a `GCNLayer` with deterministic weight init from a
	/// seeded RNG. Use this in tests asserting on training dynamics so
	/// the run is reproducible regardless of system entropy.
	pub fn with_rng<R: rand::Rng>(
		in_features: usize,
		out_features: usize,
		act_fn: Option<fn(f64) -> f64>,
		norm: bool,
		drop_rate: f64,
		rng: &mut R,
	) -> Self {
		Self {
			linear: LinearLayer::with_rng(in_features, out_features, rng),
			norm: if norm {
				Some(LayerNorm::new(out_features))
			} else {
				None
			},
			drop: if drop_rate > 0.0 {
				Some(Dropout::new(drop_rate))
			} else {
				None
			},
			act_fn,
			last_norm_adj: None,
			last_pre_act: None,
		}
	}
}

impl GraphLayer for GCNLayer {
	fn forward_graph(&mut self, g: &Graph, features: &Tensor) -> Tensor {
		let norm_adj = g.normalized_adjacency();
		let agg = norm_adj.matmul(features).expect("GCN adj*features");
		self.last_norm_adj = Some(norm_adj);

		let mut h = self.linear.forward(&agg);
		if let Some(ref mut n) = self.norm {
			h = n.forward(&h);
		}
		self.last_pre_act = Some(h.clone());
		if let Some(f) = self.act_fn {
			h = h.apply(f);
		}
		if let Some(ref mut d) = self.drop {
			h = d.forward(&h);
		}
		h
	}

	fn parameters(&self) -> Vec<&Tensor> {
		let mut p = self.linear.parameters();
		if let Some(ref n) = self.norm {
			p.extend(Layer::parameters(n));
		}
		p
	}

	fn parameters_mut(&mut self) -> Vec<&mut Tensor> {
		let mut p = self.linear.parameters_mut();
		if let Some(ref mut n) = self.norm {
			p.extend(Layer::parameters_mut(n));
		}
		p
	}

	fn set_training(&mut self, training: bool) {
		if let Some(ref mut d) = self.drop {
			d.set_training(training);
		}
	}
}

impl BackwardGraphLayer for GCNLayer {
	fn backward_graph(&mut self, _g: &Graph, d_out: &Tensor) -> Tensor {
		let mut grad = d_out.clone();
		if let Some(ref d) = self.drop {
			grad = d.backward(&grad);
		}
		if let Some(f) = self.act_fn {
			let pre_act = self.last_pre_act.as_ref().expect("backward before forward");
			grad = act_deriv_mul(f, &grad, pre_act);
		}
		if let Some(ref mut n) = self.norm {
			grad = n.backward(&grad);
		}
		let d_agg = self.linear.backward(&grad);
		let norm_adj = self
			.last_norm_adj
			.as_ref()
			.expect("backward before forward");
		norm_adj
			.transpose()
			.matmul(&d_agg)
			.expect("GCN backward adj.T*dAgg")
	}

	fn param_grads(&self) -> Vec<&Tensor> {
		let mut g = self.linear.param_grads();
		if let Some(ref n) = self.norm {
			g.extend(Backward::param_grads(n));
		}
		g
	}

	fn param_grads_mut(&mut self) -> Vec<&mut Tensor> {
		let mut g = self.linear.param_grads_mut();
		if let Some(ref mut n) = self.norm {
			g.extend(Backward::param_grads_mut(n));
		}
		g
	}

	fn zero_grads(&mut self) {
		self.linear.zero_grads();
		if let Some(ref mut n) = self.norm {
			Backward::zero_grads(n);
		}
	}
}
