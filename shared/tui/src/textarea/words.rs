use unicode_segmentation::UnicodeSegmentation;

pub fn next_word_boundary_bytes(line: &str, col: usize) -> usize {
	let mut iter = line.split_word_bound_indices();
	for (i, w) in iter.by_ref() {
		let end = i + w.len();
		if end <= col {
			continue;
		}
		if i <= col {
			if end < line.len() {
				return skip_to_word_start(line, end).unwrap_or(end);
			}
			return end;
		}
		if is_wordlike(w) {
			return i;
		}
	}
	line.len()
}

pub fn prev_word_boundary_bytes(line: &str, col: usize) -> usize {
	let bounds: Vec<(usize, &str)> = line.split_word_bound_indices().collect();
	for &(i, w) in bounds.iter().rev() {
		if i < col && is_wordlike(w) {
			return i;
		}
	}
	0
}

fn skip_to_word_start(line: &str, from: usize) -> Option<usize> {
	for (i, w) in line[from..].split_word_bound_indices() {
		if is_wordlike(w) {
			return Some(from + i);
		}
	}
	None
}

fn is_wordlike(s: &str) -> bool {
	s.chars().any(|c| !c.is_whitespace())
}
