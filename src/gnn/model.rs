

use crate::gnn::backward::{BackwardGraphLayer, GraphLayer};
use crate::gnn::graph::Graph;
use crate::gnn::layer::{Backward, Layer, LinearLayer};
use crate::gnn::tensor::Tensor;

pub struct Model {
	pub layers: Vec<Box<dyn BackwardGraphLayer>>,
	pub out_layer: Option<LinearLayer>,
	pub residual: bool,
}

impl Model {
	pub fn new(layers: Vec<Box<dyn BackwardGraphLayer>>, out_layer: Option<LinearLayer>) -> Self {
		Self {
			layers,
			out_layer,
			residual: false,
		}
	}

	pub fn new_residual(
		layers: Vec<Box<dyn BackwardGraphLayer>>,
		out_layer: Option<LinearLayer>,
	) -> Self {
		Self {
			layers,
			out_layer,
			residual: true,
		}
	}

	pub fn forward(&mut self, g: &Graph, features: &Tensor) -> Tensor {
		let mut h = features.clone();
		for layer in &mut self.layers {
			let mut out = layer.forward_graph(g, &h);
			if self.residual && h.rows == out.rows && h.cols == out.cols {
				out = out.add(&h).expect("residual add");
			}
			h = out;
		}
		if let Some(ref mut ol) = self.out_layer {
			h = ol.forward(&h);
		}
		h
	}

	pub fn backward(&mut self, g: &Graph, d_out: &Tensor) {
		let mut grad = d_out.clone();
		if let Some(ref mut ol) = self.out_layer {
			grad = ol.backward(&grad);
		}
		for layer in self.layers.iter_mut().rev() {
			let mut input_grad = layer.backward_graph(g, &grad);
			if self.residual && input_grad.rows == grad.rows && input_grad.cols == grad.cols {
				input_grad
					.add_inplace(&grad)
					.expect("residual backward add");
			}
			grad = input_grad;
		}
	}

	pub fn parameters(&self) -> Vec<&Tensor> {
		let mut p = Vec::new();
		for layer in &self.layers {
			p.extend(GraphLayer::parameters(layer.as_ref()));
		}
		if let Some(ref ol) = self.out_layer {
			p.extend(Layer::parameters(ol));
		}
		p
	}

	pub fn parameters_mut(&mut self) -> Vec<&mut Tensor> {
		let mut p = Vec::new();
		for layer in &mut self.layers {
			p.extend(GraphLayer::parameters_mut(layer.as_mut()));
		}
		if let Some(ref mut ol) = self.out_layer {
			p.extend(Layer::parameters_mut(ol));
		}
		p
	}

	pub fn param_grads(&self) -> Vec<&Tensor> {
		let mut g = Vec::new();
		for layer in &self.layers {
			g.extend(layer.param_grads());
		}
		if let Some(ref ol) = self.out_layer {
			g.extend(Backward::param_grads(ol));
		}
		g
	}

	pub fn param_grads_mut(&mut self) -> Vec<&mut Tensor> {
		let mut g = Vec::new();
		for layer in &mut self.layers {
			g.extend(layer.param_grads_mut());
		}
		if let Some(ref mut ol) = self.out_layer {
			g.extend(Backward::param_grads_mut(ol));
		}
		g
	}

	pub fn zero_grads(&mut self) {
		for layer in &mut self.layers {
			layer.zero_grads();
		}
		if let Some(ref mut ol) = self.out_layer {
			Backward::zero_grads(ol);
		}
	}

	pub fn set_training(&mut self, training: bool) {
		for layer in &mut self.layers {
			layer.set_training(training);
		}
	}
}
