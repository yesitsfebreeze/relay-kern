

use crate::gnn::activation::Activation;
use crate::gnn::backward::{
	act_deriv_mul, l2_norm_backward, l2_normalize_rows, BackwardGraphLayer, GraphLayer,
};
use crate::gnn::dropout::Dropout;
use crate::gnn::graph::Graph;
use crate::gnn::layer::{Backward, Layer, LinearLayer};
use crate::gnn::message::AggregateFunc;
use crate::gnn::norm::LayerNorm;
use crate::gnn::tensor::Tensor;

pub struct SAGELayer {
	pub linear: LinearLayer,
	pub agg_func: AggregateFunc,
	pub norm: Option<LayerNorm>,
	pub drop: Option<Dropout>,
	pub act: Option<Activation>,
	pub l2_norm: bool,
	pub in_features: usize,
	last_concats: Option<Tensor>,
	last_nbr_idxs: Vec<Vec<usize>>,
	last_pre_act: Option<Tensor>,
	last_l2_in: Option<Tensor>,
}

impl SAGELayer {
	pub fn new(
		in_features: usize,
		out_features: usize,
		agg: AggregateFunc,
		act: Option<Activation>,
		l2_norm: bool,
		layer_norm: bool,
		drop_rate: f64,
	) -> Self {
		Self {
			linear: LinearLayer::new(2 * in_features, out_features),
			agg_func: agg,
			norm: if layer_norm {
				Some(LayerNorm::new(out_features))
			} else {
				None
			},
			drop: if drop_rate > 0.0 {
				Some(Dropout::new(drop_rate))
			} else {
				None
			},
			act,
			l2_norm,
			in_features,
			last_concats: None,
			last_nbr_idxs: Vec::new(),
			last_pre_act: None,
			last_l2_in: None,
		}
	}
}

impl GraphLayer for SAGELayer {
	fn forward_graph(&mut self, g: &Graph, features: &Tensor) -> Tensor {
		let n = g.num_nodes();
		let inf = self.in_features;
		let mut concats = Tensor::zeros(n, 2 * inf);
		let mut nbr_idxs = vec![Vec::new(); n];
		let zero_msg = Tensor::zeros(1, inf);

		for (i, node) in g.nodes.iter().enumerate() {
			let self_feat = features.row(i);
			let neighbors = g.in_neighbors(&node.id);
			let idxs: Vec<usize> = neighbors
				.iter()
				.filter_map(|nbr| g.node_index(nbr))
				.collect();
			let messages: Vec<Tensor> = idxs.iter().map(|&idx| features.row(idx)).collect();
			nbr_idxs[i] = idxs;

			let agg = (self.agg_func)(&messages).unwrap_or_else(|| zero_msg.clone());

			concats.data[i * (2 * inf)..i * (2 * inf) + inf].copy_from_slice(&self_feat.data);
			concats.data[i * (2 * inf) + inf..(i + 1) * (2 * inf)].copy_from_slice(&agg.data);
		}
		self.last_concats = Some(concats.clone());
		self.last_nbr_idxs = nbr_idxs;

		let mut output = self.linear.forward(&concats);
		if let Some(ref mut nm) = self.norm {
			output = nm.forward(&output);
		}
		self.last_pre_act = Some(output.clone());
		if let Some(a) = self.act {
			output = output.apply(|x| a.forward(x));
		}
		if self.l2_norm {
			self.last_l2_in = Some(output.clone());
			output = l2_normalize_rows(&output);
		} else {
			self.last_l2_in = None;
		}
		if let Some(ref mut d) = self.drop {
			output = d.forward(&output);
		}
		output
	}

	fn parameters(&self) -> Vec<&Tensor> {
		let mut p = self.linear.parameters();
		if let Some(ref nm) = self.norm {
			p.extend(Layer::parameters(nm));
		}
		p
	}

	fn parameters_mut(&mut self) -> Vec<&mut Tensor> {
		let mut p = self.linear.parameters_mut();
		if let Some(ref mut nm) = self.norm {
			p.extend(Layer::parameters_mut(nm));
		}
		p
	}

	fn set_training(&mut self, training: bool) {
		if let Some(ref mut d) = self.drop {
			d.set_training(training);
		}
	}
}

impl BackwardGraphLayer for SAGELayer {
	fn backward_graph(&mut self, g: &Graph, d_out: &Tensor) -> Tensor {
		let mut grad = d_out.clone();
		if let Some(ref d) = self.drop {
			grad = d.backward(&grad);
		}
		if self.l2_norm {
			if let Some(ref l2_in) = self.last_l2_in {
				grad = l2_norm_backward(l2_in, &grad);
			}
		}
		if let Some(a) = self.act {
			let pre_act = self.last_pre_act.as_ref().unwrap();
			grad = act_deriv_mul(a, &grad, pre_act);
		}
		if let Some(ref mut nm) = self.norm {
			grad = nm.backward(&grad);
		}
		let d_concat = self.linear.backward(&grad);

		let inf = self.in_features;
		let n = g.num_nodes();
		let mut d_features = Tensor::zeros(n, inf);

		for i in 0..n {
			for d in 0..inf {
				d_features.data[i * inf + d] += d_concat.at(i, d);
			}
			let nbrs = &self.last_nbr_idxs[i];
			if nbrs.is_empty() {
				continue;
			}
			let scale = 1.0 / nbrs.len() as f64;
			for &j in nbrs {
				for d in 0..inf {
					d_features.data[j * inf + d] += d_concat.at(i, inf + d) * scale;
				}
			}
		}
		d_features
	}

	fn param_grads(&self) -> Vec<&Tensor> {
		let mut g = self.linear.param_grads();
		if let Some(ref nm) = self.norm {
			g.extend(Backward::param_grads(nm));
		}
		g
	}

	fn param_grads_mut(&mut self) -> Vec<&mut Tensor> {
		let mut g = self.linear.param_grads_mut();
		if let Some(ref mut nm) = self.norm {
			g.extend(Backward::param_grads_mut(nm));
		}
		g
	}

	fn zero_grads(&mut self) {
		self.linear.zero_grads();
		if let Some(ref mut nm) = self.norm {
			Backward::zero_grads(nm);
		}
	}
}
