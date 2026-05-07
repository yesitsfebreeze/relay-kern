

use crate::gnn::model::Model;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PersistError {
	#[error("parameter count mismatch: model {model}, file {file}")]
	CountMismatch { model: usize, file: usize },
	#[error("param {idx} shape mismatch: model ({mr},{mc}), file ({fr},{fc})", mr = .model.0, mc = .model.1, fr = .file.0, fc = .file.1)]
	ShapeMismatch {
		idx: usize,
		model: (usize, usize),
		file: (usize, usize),
	},
	#[error("bincode encode: {0}")]
	BincodeEncode(#[from] bincode::error::EncodeError),
	#[error("bincode decode: {0}")]
	BincodeDecode(#[from] bincode::error::DecodeError),
	#[error("io: {0}")]
	Io(#[from] std::io::Error),
}

fn bincode_cfg() -> bincode::config::Configuration {
	bincode::config::standard()
}

#[derive(Serialize, Deserialize)]
struct WeightFile {
	version: u32,
	params: Vec<TensorRecord>,
}

#[derive(Serialize, Deserialize)]
struct TensorRecord {
	rows: usize,
	cols: usize,
	data: Vec<f64>,
}

pub fn marshal_weights(model: &Model) -> Result<Vec<u8>, PersistError> {
	let params = model.parameters();
	let records: Vec<TensorRecord> = params
		.iter()
		.map(|p| TensorRecord {
			rows: p.rows,
			cols: p.cols,
			data: p.data.clone(),
		})
		.collect();
	let wf = WeightFile {
		version: 1,
		params: records,
	};
	Ok(bincode::serde::encode_to_vec(&wf, bincode_cfg())?)
}

pub fn unmarshal_weights(model: &mut Model, data: &[u8]) -> Result<(), PersistError> {
	let (wf, _): (WeightFile, _) = bincode::serde::decode_from_slice(data, bincode_cfg())?;
	let params = model.parameters_mut();
	if params.len() != wf.params.len() {
		return Err(PersistError::CountMismatch {
			model: params.len(),
			file: wf.params.len(),
		});
	}
	for (i, (param, rec)) in params.into_iter().zip(&wf.params).enumerate() {
		if param.rows != rec.rows || param.cols != rec.cols {
			return Err(PersistError::ShapeMismatch {
				idx: i,
				model: (param.rows, param.cols),
				file: (rec.rows, rec.cols),
			});
		}
		param.data.copy_from_slice(&rec.data);
	}
	Ok(())
}

pub fn save_weights(model: &Model, path: &str) -> Result<(), PersistError> {
	let data = marshal_weights(model)?;
	std::fs::write(path, data)?;
	Ok(())
}

pub fn load_weights(model: &mut Model, path: &str) -> Result<(), PersistError> {
	let data = std::fs::read(path)?;
	unmarshal_weights(model, &data)
}
