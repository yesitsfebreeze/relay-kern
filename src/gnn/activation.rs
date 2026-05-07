

use crate::gnn::tensor::Tensor;

#[inline]
pub fn relu(x: f64) -> f64 {
	x.max(0.0)
}

#[inline]
pub fn relu_deriv(x: f64) -> f64 {
	if x > 0.0 {
		1.0
	} else {
		0.0
	}
}

#[inline]
pub fn sigmoid(x: f64) -> f64 {
	1.0 / (1.0 + (-x).exp())
}

#[inline]
pub fn sigmoid_deriv(x: f64) -> f64 {
	let s = sigmoid(x);
	s * (1.0 - s)
}

#[inline]
pub fn tanh_act(x: f64) -> f64 {
	x.tanh()
}

#[inline]
pub fn tanh_deriv(x: f64) -> f64 {
	let t = x.tanh();
	1.0 - t * t
}

#[inline]
pub fn leaky_relu(alpha: f64, x: f64) -> f64 {
	if x > 0.0 {
		x
	} else {
		alpha * x
	}
}

#[inline]
pub fn leaky_relu_deriv(alpha: f64, x: f64) -> f64 {
	if x > 0.0 {
		1.0
	} else {
		alpha
	}
}

pub fn softmax(t: &Tensor) -> Tensor {
	let mut out = Tensor::zeros(t.rows, t.cols);
	for i in 0..t.rows {
		let mut max_val = f64::NEG_INFINITY;
		for j in 0..t.cols {
			let v = t.at(i, j);
			if v > max_val {
				max_val = v;
			}
		}
		let mut sum = 0.0;
		for j in 0..t.cols {
			let e = (t.at(i, j) - max_val).exp();
			out.set(i, j, e);
			sum += e;
		}
		for j in 0..t.cols {
			out.set(i, j, out.at(i, j) / sum);
		}
	}
	out
}

pub fn log_softmax(t: &Tensor) -> Tensor {
	let mut out = Tensor::zeros(t.rows, t.cols);
	for i in 0..t.rows {
		let mut max_val = f64::NEG_INFINITY;
		for j in 0..t.cols {
			let v = t.at(i, j);
			if v > max_val {
				max_val = v;
			}
		}
		let mut log_sum = 0.0;
		for j in 0..t.cols {
			log_sum += (t.at(i, j) - max_val).exp();
		}
		let log_sum = max_val + log_sum.ln();
		for j in 0..t.cols {
			out.set(i, j, t.at(i, j) - log_sum);
		}
	}
	out
}
