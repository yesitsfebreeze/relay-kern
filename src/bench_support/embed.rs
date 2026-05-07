use crate::base::util::content_hash;

pub const DIM: usize = 64;

pub fn embed(text: &str) -> Vec<f64> {
	let mut v = vec![0.0f64; DIM];
	for tok in tokenize(text) {
		let h = content_hash(&tok);
		let bytes = h.as_bytes();
		for chunk in 0..4 {
			let base = chunk * 4;
			let slot = (hex_u32(&bytes[base..base + 4]) as usize) % DIM;
			let sign = if (bytes[base + 4] & 1) == 0 { 1.0 } else { -1.0 };
			v[slot] += sign;
		}
	}
	normalize(&mut v);
	v
}

fn tokenize(text: &str) -> Vec<String> {
	text
		.split(|c: char| !c.is_alphanumeric())
		.filter(|s| !s.is_empty())
		.map(|s| s.to_lowercase())
		.collect()
}

fn hex_u32(bytes: &[u8]) -> u32 {
	let mut n = 0u32;
	for &b in bytes {
		let v = match b {
			b'0'..=b'9' => b - b'0',
			b'a'..=b'f' => b - b'a' + 10,
			_ => 0,
		};
		n = (n << 4) | v as u32;
	}
	n
}

fn normalize(v: &mut [f64]) {
	let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
	if norm > 0.0 {
		for x in v.iter_mut() {
			*x /= norm;
		}
	}
}
