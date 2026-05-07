

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TensorError {
	#[error("shape mismatch: expected ({er},{ec}), got ({ar},{ac})", er = .expected.0, ec = .expected.1, ar = .actual.0, ac = .actual.1)]
	ShapeMismatch {
		expected: (usize, usize),
		actual: (usize, usize),
	},
	#[error("inner dimension mismatch: {lhs} vs {rhs}")]
	InnerMismatch { lhs: usize, rhs: usize },
	#[error("data length {len} does not match shape ({rows}, {cols})")]
	DataLength {
		len: usize,
		rows: usize,
		cols: usize,
	},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tensor {
	pub data: Vec<f64>,
	pub rows: usize,
	pub cols: usize,
}

impl Tensor {
	pub fn new(rows: usize, cols: usize, data: Vec<f64>) -> Result<Self, TensorError> {
		if data.len() != rows * cols {
			return Err(TensorError::DataLength {
				len: data.len(),
				rows,
				cols,
			});
		}
		Ok(Self { data, rows, cols })
	}

	pub fn zeros(rows: usize, cols: usize) -> Self {
		Self {
			data: vec![0.0; rows * cols],
			rows,
			cols,
		}
	}

	pub fn ones(rows: usize, cols: usize) -> Self {
		Self {
			data: vec![1.0; rows * cols],
			rows,
			cols,
		}
	}

	pub fn rand(rows: usize, cols: usize, scale: f64) -> Self {
		let mut rng = rand::rng();
		Self::rand_with(rows, cols, scale, &mut rng)
	}

	/// Box-Muller normal init using a caller-supplied RNG.
	///
	/// Use this for reproducible weight init in tests. Pass a seeded
	/// `rand::rngs::StdRng` (or any `RngCore`) to make initialization
	/// deterministic. Production callers should use [`Tensor::rand`],
	/// which draws from system entropy via `rand::rng()`.
	pub fn rand_with<R: rand::Rng>(rows: usize, cols: usize, scale: f64, rng: &mut R) -> Self {
		use rand::RngExt;
		let data: Vec<f64> = (0..rows * cols)
			.map(|_| {
				let u1: f64 = rng.random_range(1e-10..1.0);
				let u2: f64 = rng.random_range(0.0..std::f64::consts::TAU);
				(-2.0 * u1.ln()).sqrt() * u2.cos() * scale
			})
			.collect();
		Self { data, rows, cols }
	}

	#[inline]
	pub fn at(&self, row: usize, col: usize) -> f64 {
		self.data[row * self.cols + col]
	}

	#[inline]
	pub fn set(&mut self, row: usize, col: usize, val: f64) {
		self.data[row * self.cols + col] = val;
	}

	#[inline]
	pub fn shape(&self) -> (usize, usize) {
		(self.rows, self.cols)
	}

	pub fn add(&self, other: &Tensor) -> Result<Tensor, TensorError> {
		self.check_shape(other)?;
		let data = self
			.data
			.iter()
			.zip(&other.data)
			.map(|(a, b)| a + b)
			.collect();
		Ok(Tensor {
			data,
			rows: self.rows,
			cols: self.cols,
		})
	}

	pub fn sub(&self, other: &Tensor) -> Result<Tensor, TensorError> {
		self.check_shape(other)?;
		let data = self
			.data
			.iter()
			.zip(&other.data)
			.map(|(a, b)| a - b)
			.collect();
		Ok(Tensor {
			data,
			rows: self.rows,
			cols: self.cols,
		})
	}

	pub fn mul(&self, other: &Tensor) -> Result<Tensor, TensorError> {
		self.check_shape(other)?;
		let data = self
			.data
			.iter()
			.zip(&other.data)
			.map(|(a, b)| a * b)
			.collect();
		Ok(Tensor {
			data,
			rows: self.rows,
			cols: self.cols,
		})
	}

	pub fn scale(&self, s: f64) -> Tensor {
		Tensor {
			data: self.data.iter().map(|v| v * s).collect(),
			rows: self.rows,
			cols: self.cols,
		}
	}

	const MATMUL_PAR_THRESHOLD: usize = 64;

	pub fn matmul(&self, other: &Tensor) -> Result<Tensor, TensorError> {
		if self.cols != other.rows {
			return Err(TensorError::InnerMismatch {
				lhs: self.cols,
				rhs: other.rows,
			});
		}
		let (m, k, n) = (self.rows, self.cols, other.cols);
		let mut out = vec![0.0; m * n];
		let a = &self.data;
		let b = &other.data;

		if m >= Self::MATMUL_PAR_THRESHOLD {
			out.par_chunks_mut(n).enumerate().for_each(|(i, row)| {
				for p in 0..k {
					let a_ip = a[i * k + p];
					let b_row = p * n;
					for j in 0..n {
						row[j] += a_ip * b[b_row + j];
					}
				}
			});
		} else {
			for i in 0..m {
				for p in 0..k {
					let a_ip = a[i * k + p];
					let out_row = i * n;
					let b_row = p * n;
					for j in 0..n {
						out[out_row + j] += a_ip * b[b_row + j];
					}
				}
			}
		}

		Ok(Tensor {
			data: out,
			rows: m,
			cols: n,
		})
	}

	pub fn transpose(&self) -> Tensor {
		let mut out = Tensor::zeros(self.cols, self.rows);
		for i in 0..self.rows {
			for j in 0..self.cols {
				out.data[j * self.rows + i] = self.data[i * self.cols + j];
			}
		}
		out
	}

	pub fn apply(&self, f: impl Fn(f64) -> f64) -> Tensor {
		Tensor {
			data: self.data.iter().map(|v| f(*v)).collect(),
			rows: self.rows,
			cols: self.cols,
		}
	}

	pub fn add_row_vec(&self, vec: &Tensor) -> Result<Tensor, TensorError> {
		if vec.rows != 1 || vec.cols != self.cols {
			return Err(TensorError::ShapeMismatch {
				expected: (1, self.cols),
				actual: (vec.rows, vec.cols),
			});
		}
		let mut out = self.clone();
		for i in 0..self.rows {
			for j in 0..self.cols {
				out.data[i * self.cols + j] += vec.data[j];
			}
		}
		Ok(out)
	}

	pub fn row(&self, i: usize) -> Tensor {
		let start = i * self.cols;
		Tensor {
			data: self.data[start..start + self.cols].to_vec(),
			rows: 1,
			cols: self.cols,
		}
	}

	pub fn set_row(&mut self, i: usize, row: &Tensor) {
		let start = i * self.cols;
		self.data[start..start + self.cols].copy_from_slice(&row.data);
	}

	pub fn sum_all(&self) -> f64 {
		self.data.iter().sum()
	}

	pub fn max_in_row(&self, row: usize) -> usize {
		let start = row * self.cols;
		let slice = &self.data[start..start + self.cols];
		slice
			.iter()
			.enumerate()
			.max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
			.map(|(i, _)| i)
			.unwrap_or(0)
	}

	pub fn add_inplace(&mut self, other: &Tensor) -> Result<(), TensorError> {
		self.check_shape(other)?;
		for (a, b) in self.data.iter_mut().zip(&other.data) {
			*a += *b;
		}
		Ok(())
	}

	pub fn scale_inplace(&mut self, s: f64) {
		for v in &mut self.data {
			*v *= s;
		}
	}

	fn check_shape(&self, other: &Tensor) -> Result<(), TensorError> {
		if self.rows != other.rows || self.cols != other.cols {
			return Err(TensorError::ShapeMismatch {
				expected: (self.rows, self.cols),
				actual: (other.rows, other.cols),
			});
		}
		Ok(())
	}
}
