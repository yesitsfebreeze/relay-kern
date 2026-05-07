

use crate::gnn::tensor::Tensor;

pub trait Optimizer {
	fn step(&mut self, params: &mut [&mut Tensor], grads: &[&Tensor]);
	fn zero_grad(&self, grads: &mut [Tensor]) {
		for g in grads.iter_mut() {
			for v in &mut g.data {
				*v = 0.0;
			}
		}
	}
}

pub struct SGD {
	pub lr: f64,
	pub momentum: f64,
	velocity: Vec<Tensor>,
}

impl SGD {
	pub fn new(lr: f64) -> Self {
		Self {
			lr,
			momentum: 0.0,
			velocity: Vec::new(),
		}
	}

	pub fn with_momentum(lr: f64, momentum: f64) -> Self {
		Self {
			lr,
			momentum,
			velocity: Vec::new(),
		}
	}
}

impl Optimizer for SGD {
	fn step(&mut self, params: &mut [&mut Tensor], grads: &[&Tensor]) {
		if self.momentum > 0.0 && self.velocity.is_empty() {
			self.velocity = params
				.iter()
				.map(|p| Tensor::zeros(p.rows, p.cols))
				.collect();
		}

		for (i, (param, grad)) in params.iter_mut().zip(grads.iter()).enumerate() {
			if self.momentum > 0.0 {
				let v = &mut self.velocity[i];
				for (j, vj) in v.data.iter_mut().enumerate() {
					*vj = self.momentum * *vj + grad.data[j];
				}
				for (j, pj) in param.data.iter_mut().enumerate() {
					*pj -= self.lr * self.velocity[i].data[j];
				}
			} else {
				for (pj, gj) in param.data.iter_mut().zip(&grad.data) {
					*pj -= self.lr * gj;
				}
			}
		}
	}
}

pub struct Adam {
	pub lr: f64,
	pub beta1: f64,
	pub beta2: f64,
	pub epsilon: f64,
	step_count: usize,
	m: Vec<Tensor>,
	v: Vec<Tensor>,
}

impl Adam {
	pub fn new(lr: f64) -> Self {
		Self {
			lr,
			beta1: 0.9,
			beta2: 0.999,
			epsilon: 1e-8,
			step_count: 0,
			m: Vec::new(),
			v: Vec::new(),
		}
	}
}

impl Optimizer for Adam {
	fn step(&mut self, params: &mut [&mut Tensor], grads: &[&Tensor]) {
		if self.m.is_empty() {
			self.m = params
				.iter()
				.map(|p| Tensor::zeros(p.rows, p.cols))
				.collect();
			self.v = params
				.iter()
				.map(|p| Tensor::zeros(p.rows, p.cols))
				.collect();
		}
		self.step_count += 1;
		let t = self.step_count as f64;

		for (i, (param, grad)) in params.iter_mut().zip(grads.iter()).enumerate() {
			for j in 0..param.data.len() {
				let g = grad.data[j];
				self.m[i].data[j] = self.beta1 * self.m[i].data[j] + (1.0 - self.beta1) * g;
				self.v[i].data[j] = self.beta2 * self.v[i].data[j] + (1.0 - self.beta2) * g * g;

				let m_hat = self.m[i].data[j] / (1.0 - self.beta1.powf(t));
				let v_hat = self.v[i].data[j] / (1.0 - self.beta2.powf(t));

				param.data[j] -= self.lr * m_hat / (v_hat.sqrt() + self.epsilon);
			}
		}
	}
}
