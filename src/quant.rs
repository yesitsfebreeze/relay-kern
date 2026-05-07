use serde::{Deserialize, Serialize};

pub const INT8_MAX_ABS: f32 = 127.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum QuantizationMode {
	#[default]
	None = 0,
	Int8 = 1,
}

impl QuantizationMode {
	pub fn parse(s: &str) -> Option<Self> {
		match s.trim().to_ascii_lowercase().as_str() {
			"none" | "f64" | "off" => Some(Self::None),
			"int8" | "i8" => Some(Self::Int8),
			_ => None,
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::None => "none",
			Self::Int8 => "int8",
		}
	}

	pub fn bytes_per_dim(self) -> f64 {
		match self {
			Self::None => 8.0,
			Self::Int8 => 1.0,
		}
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantizedVec {
	pub mode: QuantizationMode,
	pub scale: f32,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub f: Vec<f64>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub q: Vec<i8>,
}

impl QuantizedVec {
	pub fn encode(v: &[f64], mode: QuantizationMode) -> Self {
		match mode {
			QuantizationMode::None => Self {
				mode,
				scale: 0.0,
				f: v.to_vec(),
				q: Vec::new(),
			},
			QuantizationMode::Int8 => encode_int8(v),
		}
	}

	pub fn decode(&self) -> Vec<f64> {
		match self.mode {
			QuantizationMode::None => self.f.clone(),
			QuantizationMode::Int8 => self
				.q
				.iter()
				.map(|&qi| (qi as f64) * (self.scale as f64))
				.collect(),
		}
	}

	pub fn dim(&self) -> usize {
		match self.mode {
			QuantizationMode::None => self.f.len(),
			QuantizationMode::Int8 => self.q.len(),
		}
	}
}

fn encode_int8(v: &[f64]) -> QuantizedVec {
	if v.is_empty() {
		return QuantizedVec {
			mode: QuantizationMode::Int8,
			scale: 0.0,
			f: Vec::new(),
			q: Vec::new(),
		};
	}
	let max_abs = v.iter().fold(0.0_f64, |m, &x| m.max(x.abs()));
	let scale = if max_abs == 0.0 {
		1.0_f32
	} else {
		(max_abs as f32) / INT8_MAX_ABS
	};
	let inv = 1.0_f32 / scale;
	let q: Vec<i8> = v
		.iter()
		.map(|&x| {
			let scaled = (x as f32) * inv;
			let rounded = scaled.round();
			rounded.clamp(-INT8_MAX_ABS, INT8_MAX_ABS) as i8
		})
		.collect();
	QuantizedVec {
		mode: QuantizationMode::Int8,
		scale,
		f: Vec::new(),
		q,
	}
}

pub fn quantized_cosine_distance(a: &QuantizedVec, b: &QuantizedVec) -> f64 {
	match (a.mode, b.mode) {
		(QuantizationMode::Int8, QuantizationMode::Int8) => {
			int8_cosine_distance(&a.q, &b.q) as f64
		}
		_ => {
			let av = a.decode();
			let bv = b.decode();
			f64_cosine_distance(&av, &bv)
		}
	}
}

pub fn f64_cosine_distance(a: &[f64], b: &[f64]) -> f64 {
	if a.is_empty() || b.is_empty() || a.len() != b.len() {
		return 1.0;
	}
	let mut dot = 0.0_f64;
	let mut na = 0.0_f64;
	let mut nb = 0.0_f64;
	for i in 0..a.len() {
		dot += a[i] * b[i];
		na += a[i] * a[i];
		nb += b[i] * b[i];
	}
	let denom = (na * nb).sqrt();
	if denom == 0.0 {
		return 1.0;
	}
	let cos = (dot / denom).clamp(-1.0, 1.0);
	1.0 - cos
}

fn int8_cosine_distance(a: &[i8], b: &[i8]) -> f32 {
	let n = a.len();
	if n == 0 || n != b.len() {
		return 1.0;
	}
	let mut dot: i32 = 0;
	let mut na: i32 = 0;
	let mut nb: i32 = 0;
	for i in 0..n {
		let ai = a[i] as i32;
		let bi = b[i] as i32;
		dot += ai * bi;
		na += ai * ai;
		nb += bi * bi;
	}
	if na == 0 || nb == 0 {
		return 1.0;
	}
	let denom = ((na as f32) * (nb as f32)).sqrt();
	let cos = ((dot as f32) / denom).clamp(-1.0, 1.0);
	1.0 - cos
}
