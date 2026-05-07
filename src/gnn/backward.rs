

use crate::gnn::graph::Graph;
use crate::gnn::tensor::Tensor;

pub fn act_deriv_mul(act_fn: fn(f64) -> f64, d_out: &Tensor, pre_act: &Tensor) -> Tensor {
	const EPS: f64 = 1e-5;
	let mut out = Tensor::zeros(d_out.rows, d_out.cols);
	for (i, &x) in pre_act.data.iter().enumerate() {
		let deriv = (act_fn(x + EPS) - act_fn(x - EPS)) / (2.0 * EPS);
		out.data[i] = d_out.data[i] * deriv;
	}
	out
}

pub fn l2_normalize_rows(t: &Tensor) -> Tensor {
	let mut out = t.clone();
	for i in 0..t.rows {
		let mut sum_sq = 0.0;
		for j in 0..t.cols {
			let v = t.at(i, j);
			sum_sq += v * v;
		}
		if sum_sq == 0.0 {
			continue;
		}
		let inv_norm = 1.0 / sum_sq.sqrt();
		for j in 0..t.cols {
			out.set(i, j, t.at(i, j) * inv_norm);
		}
	}
	out
}

pub fn l2_norm_backward(pre_norm: &Tensor, d_out: &Tensor) -> Tensor {
	let mut out = Tensor::zeros(pre_norm.rows, pre_norm.cols);
	for i in 0..pre_norm.rows {
		let mut sum_sq = 0.0;
		for j in 0..pre_norm.cols {
			let v = pre_norm.at(i, j);
			sum_sq += v * v;
		}
		if sum_sq == 0.0 {
			continue;
		}
		let norm = sum_sq.sqrt();
		let inv_norm = 1.0 / norm;
		let mut dot_val = 0.0;
		for j in 0..pre_norm.cols {
			dot_val += d_out.at(i, j) * pre_norm.at(i, j);
		}
		for j in 0..pre_norm.cols {
			let x_hat = pre_norm.at(i, j) * inv_norm;
			out.set(i, j, (d_out.at(i, j) - x_hat * dot_val) * inv_norm);
		}
	}
	out
}

pub trait GraphLayer {
	fn forward_graph(&mut self, g: &Graph, features: &Tensor) -> Tensor;
	fn parameters(&self) -> Vec<&Tensor>;
	fn parameters_mut(&mut self) -> Vec<&mut Tensor>;
	fn set_training(&mut self, training: bool);
}

pub trait BackwardGraphLayer: GraphLayer {
	fn backward_graph(&mut self, g: &Graph, d_out: &Tensor) -> Tensor;
	fn param_grads(&self) -> Vec<&Tensor>;
	fn param_grads_mut(&mut self) -> Vec<&mut Tensor>;
	fn zero_grads(&mut self);
}
