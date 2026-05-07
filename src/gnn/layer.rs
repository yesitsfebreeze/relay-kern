

use crate::gnn::tensor::Tensor;

pub trait Layer {
	fn forward(&mut self, input: &Tensor) -> Tensor;
	fn parameters(&self) -> Vec<&Tensor>;
	fn parameters_mut(&mut self) -> Vec<&mut Tensor>;
}

pub trait Backward {
	fn backward(&mut self, d_out: &Tensor) -> Tensor;
	fn param_grads(&self) -> Vec<&Tensor>;
	fn param_grads_mut(&mut self) -> Vec<&mut Tensor>;
	fn zero_grads(&mut self);
}

pub struct LinearLayer {
	pub weight: Tensor, // (in_features, out_features)
	pub bias: Tensor,   // (1, out_features)
	last_input: Option<Tensor>,
	d_weight: Tensor,
	d_bias: Tensor,
}

impl LinearLayer {
	pub fn new(in_features: usize, out_features: usize) -> Self {
		let mut rng = rand::rng();
		Self::with_rng(in_features, out_features, &mut rng)
	}

	/// Construct a `LinearLayer` with deterministic weight init from a
	/// seeded RNG. Bias is zero-initialized (no RNG draw). Use this
	/// constructor in tests so loss-decrease assertions are reproducible.
	pub fn with_rng<R: rand::Rng>(
		in_features: usize,
		out_features: usize,
		rng: &mut R,
	) -> Self {
		let scale = (2.0 / (in_features + out_features) as f64).sqrt();
		let weight = Tensor::rand_with(in_features, out_features, scale, rng);
		let bias = Tensor::zeros(1, out_features);
		let d_weight = Tensor::zeros(in_features, out_features);
		let d_bias = Tensor::zeros(1, out_features);
		Self {
			weight,
			bias,
			last_input: None,
			d_weight,
			d_bias,
		}
	}
}

impl Layer for LinearLayer {
	fn forward(&mut self, input: &Tensor) -> Tensor {
		self.last_input = Some(input.clone());
		let out = input.matmul(&self.weight).expect("linear forward matmul");
		out.add_row_vec(&self.bias).expect("linear forward bias")
	}

	fn parameters(&self) -> Vec<&Tensor> {
		vec![&self.weight, &self.bias]
	}

	fn parameters_mut(&mut self) -> Vec<&mut Tensor> {
		vec![&mut self.weight, &mut self.bias]
	}
}

impl Backward for LinearLayer {
	fn backward(&mut self, d_out: &Tensor) -> Tensor {
		let input = self.last_input.as_ref().expect("backward before forward");
		let dw = input.transpose().matmul(d_out).expect("backward dW");
		self.d_weight.add_inplace(&dw).expect("accumulate dW");
		for i in 0..d_out.rows {
			for j in 0..d_out.cols {
				self.d_bias.data[j] += d_out.at(i, j);
			}
		}
		d_out
			.matmul(&self.weight.transpose())
			.expect("backward dInput")
	}

	fn param_grads(&self) -> Vec<&Tensor> {
		vec![&self.d_weight, &self.d_bias]
	}

	fn param_grads_mut(&mut self) -> Vec<&mut Tensor> {
		vec![&mut self.d_weight, &mut self.d_bias]
	}

	fn zero_grads(&mut self) {
		self.d_weight = Tensor::zeros(self.weight.rows, self.weight.cols);
		self.d_bias = Tensor::zeros(1, self.bias.cols);
	}
}
