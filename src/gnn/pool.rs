

use crate::gnn::tensor::Tensor;

pub type PoolFunc = fn(&Tensor) -> Tensor;

pub fn sum_pool(features: &Tensor) -> Tensor {
	let mut out = Tensor::zeros(1, features.cols);
	for i in 0..features.rows {
		for j in 0..features.cols {
			out.data[j] += features.at(i, j);
		}
	}
	out
}

pub fn mean_pool(features: &Tensor) -> Tensor {
	let mut out = sum_pool(features);
	let n = features.rows as f64;
	for v in &mut out.data {
		*v /= n;
	}
	out
}

pub fn max_pool(features: &Tensor) -> Tensor {
	let mut out = Tensor::zeros(1, features.cols);
	for j in 0..features.cols {
		out.data[j] = f64::NEG_INFINITY;
	}
	for i in 0..features.rows {
		for j in 0..features.cols {
			let v = features.at(i, j);
			if v > out.data[j] {
				out.data[j] = v;
			}
		}
	}
	out
}

pub fn sum_mean_pool(features: &Tensor) -> Tensor {
	let s = sum_pool(features);
	let m = mean_pool(features);
	let mut data = vec![0.0; 2 * features.cols];
	data[..features.cols].copy_from_slice(&s.data);
	data[features.cols..].copy_from_slice(&m.data);
	Tensor {
		data,
		rows: 1,
		cols: 2 * features.cols,
	}
}

pub struct ReadoutLayer {
	pub pool: PoolFunc,
	pub linear: Option<crate::gnn::layer::LinearLayer>,
}

impl ReadoutLayer {
	pub fn new(pool: PoolFunc, in_features: usize, out_features: usize) -> Self {
		let linear = if out_features > 0 {
			Some(crate::gnn::layer::LinearLayer::new(in_features, out_features))
		} else {
			None
		};
		Self { pool, linear }
	}

	pub fn forward(&mut self, input: &Tensor) -> Tensor {
		let mut pooled = (self.pool)(input);
		if let Some(ref mut l) = self.linear {
			use crate::gnn::layer::Layer;
			pooled = l.forward(&pooled);
		}
		pooled
	}
}
