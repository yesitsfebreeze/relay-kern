

use crate::gnn::activation::{log_softmax, sigmoid, softmax};
use crate::gnn::tensor::Tensor;

pub fn mse_loss(predicted: &Tensor, target: &Tensor) -> f64 {
	let diff = predicted.sub(target).expect("mse shape");
	diff.data.iter().map(|v| v * v).sum::<f64>() / diff.data.len() as f64
}

pub fn mse_grad(predicted: &Tensor, target: &Tensor) -> Tensor {
	let mut diff = predicted.sub(target).expect("mse_grad shape");
	let scale = 2.0 / diff.data.len() as f64;
	diff.scale_inplace(scale);
	diff
}

pub fn cross_entropy_loss(predicted: &Tensor, target: &Tensor) -> f64 {
	let log_probs = log_softmax(predicted);
	let n = predicted.rows;
	let mut loss = 0.0;
	for i in 0..n {
		let class_idx = target.at(i, 0) as usize;
		loss -= log_probs.at(i, class_idx);
	}
	loss / n as f64
}

pub fn cross_entropy_grad(predicted: &Tensor, target: &Tensor) -> Tensor {
	let mut probs = softmax(predicted);
	let n = predicted.rows;
	for i in 0..n {
		let class_idx = target.at(i, 0) as usize;
		probs.set(i, class_idx, probs.at(i, class_idx) - 1.0);
	}
	let scale = 1.0 / n as f64;
	probs.scale_inplace(scale);
	probs
}

pub fn nll_loss(predicted: &Tensor, target: &Tensor) -> f64 {
	let n = predicted.rows;
	let mut loss = 0.0;
	for i in 0..n {
		let class_idx = target.at(i, 0) as usize;
		loss -= predicted.at(i, class_idx);
	}
	loss / n as f64
}

pub fn accuracy(predicted: &Tensor, target: &Tensor) -> f64 {
	let mut correct = 0;
	for i in 0..predicted.rows {
		let pred = predicted.max_in_row(i);
		let true_class = target.at(i, 0).round() as usize;
		if pred == true_class {
			correct += 1;
		}
	}
	correct as f64 / predicted.rows as f64
}


fn row_dot(t: &Tensor, i: usize, j: usize) -> f64 {
	let d = t.cols;
	let mut sum = 0.0;
	for k in 0..d {
		sum += t.at(i, k) * t.at(j, k);
	}
	sum
}

pub fn link_prediction_loss(
	embeddings: &Tensor,
	pos_edges: &[[usize; 2]],
	neg_edges: &[[usize; 2]],
) -> f64 {
	let total = pos_edges.len() + neg_edges.len();
	if total == 0 {
		return 0.0;
	}
	let mut loss = 0.0;
	for e in pos_edges {
		let dot = row_dot(embeddings, e[0], e[1]);
		loss -= (sigmoid(dot) + 1e-10).ln();
	}
	for e in neg_edges {
		let dot = row_dot(embeddings, e[0], e[1]);
		loss -= (1.0 - sigmoid(dot) + 1e-10).ln();
	}
	loss / total as f64
}

pub fn link_prediction_grad(
	embeddings: &Tensor,
	pos_edges: &[[usize; 2]],
	neg_edges: &[[usize; 2]],
) -> Tensor {
	let (n, d) = (embeddings.rows, embeddings.cols);
	let total = pos_edges.len() + neg_edges.len();
	if total == 0 {
		return Tensor::zeros(n, d);
	}
	let scale = 1.0 / total as f64;
	let mut grad = Tensor::zeros(n, d);

	for e in pos_edges {
		let (u, v) = (e[0], e[1]);
		let dot = row_dot(embeddings, u, v);
		let s = sigmoid(dot) - 1.0;
		for j in 0..d {
			grad.data[u * d + j] += scale * s * embeddings.at(v, j);
			grad.data[v * d + j] += scale * s * embeddings.at(u, j);
		}
	}
	for e in neg_edges {
		let (u, v) = (e[0], e[1]);
		let dot = row_dot(embeddings, u, v);
		let s = sigmoid(dot);
		for j in 0..d {
			grad.data[u * d + j] += scale * s * embeddings.at(v, j);
			grad.data[v * d + j] += scale * s * embeddings.at(u, j);
		}
	}
	grad
}
