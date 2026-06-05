

use crate::gnn::activation::Activation;
use crate::gnn::graph::Graph;
use crate::gnn::tensor::Tensor;

/// Multiply an incoming gradient by the activation's analytic derivative,
/// evaluated at the pre-activation values. Exact (no finite-difference bias at
/// kinks) and half the activation evaluations of a central difference.
pub fn act_deriv_mul(act: Activation, d_out: &Tensor, pre_act: &Tensor) -> Tensor {
	let mut out = Tensor::zeros(d_out.rows, d_out.cols);
	for (i, &x) in pre_act.data.iter().enumerate() {
		out.data[i] = d_out.data[i] * act.deriv(x);
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn relu_backward_is_exact_no_kink_bias() {
		// Pre-activations straddling zero, incoming grad all 1.0. With the old
		// central-difference, x=+/-1e-6 leaked ~0.5; the analytic derivative
		// gates exactly: 0 where x<=0, pass-through where x>0.
		let pre = Tensor {
			data: vec![-2.0, -1e-6, 0.0, 1e-6, 3.0],
			rows: 1,
			cols: 5,
		};
		let d_out = Tensor {
			data: vec![1.0; 5],
			rows: 1,
			cols: 5,
		};
		let g = act_deriv_mul(Activation::Relu, &d_out, &pre);
		assert_eq!(g.data, vec![0.0, 0.0, 0.0, 1.0, 1.0]);
	}

	#[test]
	fn backward_scales_incoming_gradient_by_deriv() {
		let pre = Tensor {
			data: vec![1.0, -1.0],
			rows: 1,
			cols: 2,
		};
		let d_out = Tensor {
			data: vec![0.5, 0.5],
			rows: 1,
			cols: 2,
		};
		let g = act_deriv_mul(Activation::LeakyRelu(0.2), &d_out, &pre);
		assert_eq!(g.data, vec![0.5, 0.1]); // 0.5*1, 0.5*0.2
	}
}
