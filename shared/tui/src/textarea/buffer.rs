use unicode_segmentation::UnicodeSegmentation;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
	pub line: usize,
	pub col: usize,
}

#[derive(Clone, Debug)]
pub struct Buffer {
	lines: Vec<String>,
}

impl Default for Buffer {
	fn default() -> Self {
		Buffer {
			lines: vec![String::new()],
		}
	}
}

impl Buffer {
	pub fn new() -> Self {
		Self::default()
	}

	#[allow(clippy::should_implement_trait)]
	pub fn from_str(s: &str) -> Self {
		let lines: Vec<String> = s.split('\n').map(String::from).collect();
		Buffer { lines }
	}

	#[allow(clippy::inherent_to_string_shadow_display, clippy::inherent_to_string)]
	pub fn to_string(&self) -> String {
		self.lines.join("\n")
	}

	pub fn line_count(&self) -> usize {
		self.lines.len()
	}

	pub fn line(&self, idx: usize) -> &str {
		self.lines.get(idx).map(String::as_str).unwrap_or("")
	}

	pub fn line_mut(&mut self, idx: usize) -> &mut String {
		&mut self.lines[idx]
	}

	pub fn lines(&self) -> &[String] {
		&self.lines
	}

	pub fn clamp(&self, p: Pos) -> Pos {
		let line = p.line.min(self.lines.len().saturating_sub(1));
		let lb = self.lines[line].len();
		let col = p.col.min(lb);
		Pos {
			line,
			col: snap_to_grapheme(&self.lines[line], col),
		}
	}

	pub fn end_of_line(&self, line: usize) -> Pos {
		let line = line.min(self.lines.len().saturating_sub(1));
		Pos {
			line,
			col: self.lines[line].len(),
		}
	}

	pub fn end(&self) -> Pos {
		let line = self.lines.len() - 1;
		Pos {
			line,
			col: self.lines[line].len(),
		}
	}

	pub fn insert(&mut self, at: Pos, text: &str) -> Pos {
		let at = self.clamp(at);
		if !text.contains('\n') {
			self.lines[at.line].insert_str(at.col, text);
			return Pos {
				line: at.line,
				col: at.col + text.len(),
			};
		}
		let rest = self.lines[at.line].split_off(at.col);
		let mut parts = text.split('\n');
		let first = parts.next().unwrap_or("");
		self.lines[at.line].push_str(first);
		let mut end_line = at.line;
		let mut end_col = self.lines[at.line].len();
		let mut new_lines: Vec<String> = parts.map(String::from).collect();
		if !new_lines.is_empty() {
			let n = new_lines.len();
			let last = new_lines.last_mut().unwrap();
			end_line = at.line + n;
			end_col = last.len();
			last.push_str(&rest);
		}
		for (i, nl) in new_lines.into_iter().enumerate() {
			self.lines.insert(at.line + 1 + i, nl);
		}
		Pos {
			line: end_line,
			col: end_col,
		}
	}

	pub fn delete(&mut self, start: Pos, end: Pos) -> String {
		let (start, end) = normalize_range(self, start, end);
		if start == end {
			return String::new();
		}
		if start.line == end.line {
			let removed = self.lines[start.line]
				.drain(start.col..end.col)
				.collect::<String>();
			return removed;
		}
		let mut out = String::new();
		let first_tail = self.lines[start.line].split_off(start.col);
		out.push_str(&first_tail);
		out.push('\n');
		for _ in 0..(end.line - start.line - 1) {
			let mid = self.lines.remove(start.line + 1);
			out.push_str(&mid);
			out.push('\n');
		}
		let end_line_str = self.lines.remove(start.line + 1);
		let (head, tail) = end_line_str.split_at(end.col);
		out.push_str(head);
		self.lines[start.line].push_str(tail);
		out
	}

	pub fn slice(&self, start: Pos, end: Pos) -> String {
		let (start, end) = normalize_range(self, start, end);
		if start == end {
			return String::new();
		}
		if start.line == end.line {
			return self.lines[start.line][start.col..end.col].to_string();
		}
		let mut out = String::new();
		out.push_str(&self.lines[start.line][start.col..]);
		out.push('\n');
		for l in &self.lines[start.line + 1..end.line] {
			out.push_str(l);
			out.push('\n');
		}
		out.push_str(&self.lines[end.line][..end.col]);
		out
	}

	pub fn next_grapheme(&self, p: Pos) -> Pos {
		let p = self.clamp(p);
		let line = &self.lines[p.line];
		if p.col < line.len() {
			if let Some((_, g)) = line[p.col..].grapheme_indices(true).next() {
				return Pos {
					line: p.line,
					col: p.col + g.len(),
				};
			}
			return Pos {
				line: p.line,
				col: line.len(),
			};
		}
		if p.line + 1 < self.lines.len() {
			Pos {
				line: p.line + 1,
				col: 0,
			}
		} else {
			p
		}
	}

	pub fn prev_grapheme(&self, p: Pos) -> Pos {
		let p = self.clamp(p);
		if p.col > 0 {
			let line = &self.lines[p.line];
			let mut last = 0usize;
			for (i, _) in line.grapheme_indices(true) {
				if i >= p.col {
					break;
				}
				last = i;
			}
			return Pos {
				line: p.line,
				col: last,
			};
		}
		if p.line > 0 {
			let above = p.line - 1;
			return Pos {
				line: above,
				col: self.lines[above].len(),
			};
		}
		p
	}

	pub fn display_col(&self, line: usize, col: usize) -> usize {
		use unicode_width::UnicodeWidthStr;
		let s = self.line(line);
		let col = col.min(s.len());
		let col = snap_to_grapheme(s, col);
		UnicodeWidthStr::width(&s[..col])
	}

	pub fn byte_col_for_display(&self, line: usize, target_display: usize) -> usize {
		use unicode_width::UnicodeWidthStr;
		let s = self.line(line);
		let mut acc = 0usize;
		for (i, g) in s.grapheme_indices(true) {
			let w = UnicodeWidthStr::width(g);
			if acc + w > target_display {
				return i;
			}
			acc += w;
		}
		s.len()
	}
}

fn snap_to_grapheme(s: &str, col: usize) -> usize {
	if col >= s.len() {
		return s.len();
	}
	let mut last = 0usize;
	for (i, _) in s.grapheme_indices(true) {
		if i > col {
			break;
		}
		last = i;
	}
	last
}

fn normalize_range(b: &Buffer, a: Pos, c: Pos) -> (Pos, Pos) {
	let a = b.clamp(a);
	let c = b.clamp(c);
	if a <= c {
		(a, c)
	} else {
		(c, a)
	}
}
