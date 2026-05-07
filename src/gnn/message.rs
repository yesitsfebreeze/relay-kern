

use crate::gnn::tensor::Tensor;

pub type AggregateFunc = fn(&[Tensor]) -> Option<Tensor>;

pub fn sum_aggregate(messages: &[Tensor]) -> Option<Tensor> {
	if messages.is_empty() {
		return None;
	}
	let mut result = messages[0].clone();
	for m in &messages[1..] {
		for (a, b) in result.data.iter_mut().zip(&m.data) {
			*a += *b;
		}
	}
	Some(result)
}

pub fn mean_aggregate(messages: &[Tensor]) -> Option<Tensor> {
	let mut result = sum_aggregate(messages)?;
	let n = messages.len() as f64;
	for v in &mut result.data {
		*v /= n;
	}
	Some(result)
}

pub fn max_aggregate(messages: &[Tensor]) -> Option<Tensor> {
	if messages.is_empty() {
		return None;
	}
	let mut result = messages[0].clone();
	for m in &messages[1..] {
		for (a, b) in result.data.iter_mut().zip(&m.data) {
			*a = a.max(*b);
		}
	}
	Some(result)
}

pub struct MessagePassingLayer {
	pub linear: crate::gnn::layer::LinearLayer,
	pub agg_func: AggregateFunc,
	pub act_fn: Option<fn(f64) -> f64>,
	pub in_features: usize,
}

impl MessagePassingLayer {
	pub fn new(
		in_features: usize,
		out_features: usize,
		agg: AggregateFunc,
		act_fn: Option<fn(f64) -> f64>,
	) -> Self {
		Self {
			linear: crate::gnn::layer::LinearLayer::new(2 * in_features, out_features),
			agg_func: agg,
			act_fn,
			in_features,
		}
	}

	pub fn forward_graph(&mut self, g: &crate::gnn::graph::Graph, features: &Tensor) -> Tensor {
		let n = g.num_nodes();
		let out_cols = self.linear.weight.cols;
		let mut output = Tensor::zeros(n, out_cols);
		let zero_msg = Tensor::zeros(1, self.in_features);

		for (i, node) in g.nodes.iter().enumerate() {
			let self_feat = features.row(i);
			let neighbors = g.in_neighbors(&node.id);
			let messages: Vec<Tensor> = neighbors
				.iter()
				.map(|nbr| {
					let idx = g.node_index(nbr).unwrap();
					features.row(idx)
				})
				.collect();

			let agg = (self.agg_func)(&messages).unwrap_or_else(|| zero_msg.clone());

			let inf = self.in_features;
			let mut concat_data = vec![0.0; 2 * inf];
			concat_data[..inf].copy_from_slice(&self_feat.data);
			concat_data[inf..].copy_from_slice(&agg.data);
			let concat = Tensor {
				data: concat_data,
				rows: 1,
				cols: 2 * inf,
			};

			use crate::gnn::layer::Layer;
			let mut out = self.linear.forward(&concat);
			if let Some(f) = self.act_fn {
				out = out.apply(f);
			}
			output.set_row(i, &out);
		}
		output
	}
}
