

use crate::gnn::layer::{Backward, Layer};
use crate::gnn::tensor::Tensor;

pub struct LayerNorm {
	pub gamma: Tensor, // 1×D
	pub beta: Tensor,  // 1×D
	pub epsilon: f64,
	pub dim: usize,
	pub last_x_hat: Option<Tensor>,
	last_inv_std: Vec<f64>,
	d_gamma: Tensor,
	d_beta: Tensor,
}

impl LayerNorm {
	pub fn new(dim: usize) -> Self {
		Self {
			gamma: Tensor::ones(1, dim),
			beta: Tensor::zeros(1, dim),
			epsilon: 1e-5,
			dim,
			last_x_hat: None,
			last_inv_std: Vec::new(),
			d_gamma: Tensor::zeros(1, dim),
			d_beta: Tensor::zeros(1, dim),
		}
	}
}

impl Layer for LayerNorm {
	fn forward(&mut self, input: &Tensor) -> Tensor {
		let (n, d) = (input.rows, input.cols);
		let mut out = Tensor::zeros(n, d);
		let mut x_hat = Tensor::zeros(n, d);
		let mut inv_stds = vec![0.0; n];

		for (i, inv_std_slot) in inv_stds.iter_mut().enumerate().take(n) {
			let mut mean = 0.0;
			for j in 0..d {
				mean += input.at(i, j);
			}
			mean /= d as f64;

			let mut var = 0.0;
			for j in 0..d {
				let diff = input.at(i, j) - mean;
				var += diff * diff;
			}
			var /= d as f64;

			let inv_std = 1.0 / (var + self.epsilon).sqrt();
			*inv_std_slot = inv_std;

			for j in 0..d {
				let x = (input.at(i, j) - mean) * inv_std;
				x_hat.set(i, j, x);
				out.set(i, j, x * self.gamma.at(0, j) + self.beta.at(0, j));
			}
		}
		self.last_x_hat = Some(x_hat);
		self.last_inv_std = inv_stds;
		out
	}

	fn parameters(&self) -> Vec<&Tensor> {
		vec![&self.gamma, &self.beta]
	}

	fn parameters_mut(&mut self) -> Vec<&mut Tensor> {
		vec![&mut self.gamma, &mut self.beta]
	}
}

impl Backward for LayerNorm {
	fn backward(&mut self, d_out: &Tensor) -> Tensor {
		let x_hat = self.last_x_hat.as_ref().expect("backward before forward");
		let (n, d) = (d_out.rows, d_out.cols);
		let mut d_input = Tensor::zeros(n, d);

		for i in 0..n {
			let mut d_x_hat = vec![0.0; d];
			for (j, slot) in d_x_hat.iter_mut().enumerate().take(d) {
				*slot = d_out.at(i, j) * self.gamma.at(0, j);
				self.d_gamma.data[j] += d_out.at(i, j) * x_hat.at(i, j);
				self.d_beta.data[j] += d_out.at(i, j);
			}

			let mut sum_dx = 0.0;
			let mut sum_dx_xh = 0.0;
			for (j, &dxh) in d_x_hat.iter().enumerate().take(d) {
				sum_dx += dxh;
				sum_dx_xh += dxh * x_hat.at(i, j);
			}

			let scale = self.last_inv_std[i] / d as f64;
			for (j, &dxh) in d_x_hat.iter().enumerate().take(d) {
				d_input.set(
					i,
					j,
					scale * (d as f64 * dxh - sum_dx - x_hat.at(i, j) * sum_dx_xh),
				);
			}
		}
		d_input
	}

	fn param_grads(&self) -> Vec<&Tensor> {
		vec![&self.d_gamma, &self.d_beta]
	}

	fn param_grads_mut(&mut self) -> Vec<&mut Tensor> {
		vec![&mut self.d_gamma, &mut self.d_beta]
	}

	fn zero_grads(&mut self) {
		self.d_gamma = Tensor::zeros(1, self.dim);
		self.d_beta = Tensor::zeros(1, self.dim);
	}
}
