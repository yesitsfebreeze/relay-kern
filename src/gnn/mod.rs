

pub mod activation;
pub mod backward;

/// Errors raised by GNN layer operations.
///
/// Reused across gnn submodules (currently `gat`); extend this enum rather
/// than introducing per-site error types.
#[derive(Debug, thiserror::Error)]
pub enum GnnError {
	/// `backward_graph` / inference invoked before a successful `forward_graph`,
	/// or after state was reset. Cached forward state is missing.
	#[error("gnn: missing forward state ({0}); call forward_graph before backward/inference")]
	MissingForwardState(&'static str),

	/// Tensor-level shape error bubbled up from cached intermediates.
	#[error("gnn: tensor error: {0}")]
	Tensor(#[from] crate::gnn::tensor::TensorError),
}

pub mod dropout;
pub mod gat;
pub mod gcn;
pub mod graph;
pub mod layer;
pub mod loss;
pub mod message;
pub mod model;
pub mod norm;
pub mod optim;
pub mod persist;
pub mod pool;
pub mod propagate;
pub mod sage;
pub mod tensor;
pub mod train;
