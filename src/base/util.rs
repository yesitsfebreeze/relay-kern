use sha2::{Digest, Sha256};

pub fn content_hash(s: &str) -> String {
	let hash = Sha256::digest(s.as_bytes());
	hex::encode(hash)
}

mod hex {
	const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

	pub fn encode(bytes: impl AsRef<[u8]>) -> String {
		let bytes = bytes.as_ref();
		let mut s = String::with_capacity(bytes.len() * 2);
		for &b in bytes {
			s.push(HEX_CHARS[(b >> 4) as usize] as char);
			s.push(HEX_CHARS[(b & 0x0f) as usize] as char);
		}
		s
	}
}

pub fn short_id(id: &str) -> &str {
	match id.char_indices().nth(12) {
		Some((byte_pos, _)) => &id[..byte_pos],
		None => id,
	}
}

pub fn truncate(s: &str, max: usize) -> String {
	match s.char_indices().nth(max) {
		Some((byte_pos, _)) => format!("{}...", &s[..byte_pos]),
		None => s.to_string(),
	}
}

pub fn uuid_v4() -> String {
	use rand::RngExt;
	let mut rng = rand::rng();
	let mut b = [0u8; 16];
	rng.fill(&mut b);
	b[6] = (b[6] & 0x0f) | 0x40;
	b[8] = (b[8] & 0x3f) | 0x80;
	format!(
		"{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
		u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
		u16::from_be_bytes([b[4], b[5]]),
		u16::from_be_bytes([b[6], b[7]]),
		u16::from_be_bytes([b[8], b[9]]),
		u64::from_be_bytes([0, 0, b[10], b[11], b[12], b[13], b[14], b[15]]),
	)
}
