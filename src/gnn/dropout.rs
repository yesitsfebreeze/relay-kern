

use crate::gnn::tensor::Tensor;

pub struct Dropout {
	pub p: f64,
	pub training: bool,
	last_mask: Option<Tensor>,
}

impl Dropout {
	pub fn new(p: f64) -> Self {
		Self {
			p,
			training: true,
			last_mask: None,
		}
	}

	pub fn forward(&mut self, input: &Tensor) -> Tensor {
		if !self.training || self.p == 0.0 {
			self.last_mask = None;
			return input.clone();
		}
		if self.p == 1.0 {
			self.last_mask = Some(Tensor::zeros(input.rows, input.cols));
			return Tensor::zeros(input.rows, input.cols);
		}
		use rand::RngExt;
		let mut rng = rand::rng();
		let scale = 1.0 / (1.0 - self.p);
		let mut mask = Tensor::zeros(input.rows, input.cols);
		let mut out = Tensor::zeros(input.rows, input.cols);
		for i in 0..input.data.len() {
			if rng.random::<f64>() >= self.p {
				mask.data[i] = scale;
				out.data[i] = input.data[i] * scale;
			}
		}
		self.last_mask = Some(mask);
		out
	}

	pub fn backward(&self, d_out: &Tensor) -> Tensor {
		match &self.last_mask {
			None => d_out.clone(),
			Some(mask) => d_out.mul(mask).expect("dropout backward mul"),
		}
	}

	pub fn set_training(&mut self, training: bool) {
		self.training = training;
	}
}
