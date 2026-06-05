use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualRow {
	pub line: usize,
	pub start_byte: usize,
	pub end_byte: usize,
	pub trailing_hyphen: bool,
}

const MIN_WORD_PART: usize = 3;

fn is_break_ws(g: &str) -> bool {
	g.chars().all(|c| c == ' ' || c == '\t')
}

fn is_break_hyphen(g: &str) -> bool {
	matches!(g, "-" | "\u{2010}" | "\u{2013}" | "\u{2014}")
}

fn letter_count(s: &str) -> usize {
	s.chars().filter(|c| c.is_alphanumeric()).count()
}

fn word_end_from(text: &str, start: usize) -> usize {
	for (bi, g) in text[start..].grapheme_indices(true) {
		if is_break_ws(g) || is_break_hyphen(g) {
			return start + bi;
		}
	}
	text.len()
}

pub fn wrap_line(line_idx: usize, text: &str, cols: u16) -> Vec<VisualRow> {
	let cols = cols.max(1) as usize;
	let mut rows = Vec::new();
	if text.is_empty() {
		rows.push(VisualRow {
			line: line_idx,
			start_byte: 0,
			end_byte: 0,
			trailing_hyphen: false,
		});
		return rows;
	}
	let graphemes: Vec<(usize, &str)> = text.grapheme_indices(true).collect();
	let widths: Vec<usize> = graphemes
		.iter()
		.map(|(_, g)| UnicodeWidthStr::width(*g))
		.collect();

	let mut row_start_byte = 0usize;
	let mut width_acc = 0usize;
	let mut last_ws_end: Option<usize> = None;
	let mut last_hyphen_end: Option<usize> = None;
	let mut word_start: usize = 0;
	let mut i = 0usize;

	while i < graphemes.len() {
		let (gi, g) = graphemes[i];
		let w = widths[i];
		if width_acc + w > cols && width_acc > 0 {
			let (split, hyphen) = choose_break(
				text,
				row_start_byte,
				word_start,
				gi,
				i,
				&graphemes,
				&widths,
				width_acc,
				cols,
				last_ws_end,
				last_hyphen_end,
			);
			rows.push(VisualRow {
				line: line_idx,
				start_byte: row_start_byte,
				end_byte: split,
				trailing_hyphen: hyphen,
			});
			let mut skipped = 0usize;
			for (j, g2) in text[split..].grapheme_indices(true) {
				if is_break_ws(g2) {
					skipped = j + g2.len();
				} else {
					break;
				}
			}
			row_start_byte = split + skipped;
			let row_start_idx = graphemes
				.iter()
				.position(|(b, _)| *b >= row_start_byte)
				.unwrap_or(graphemes.len());
			width_acc = 0;
			last_ws_end = None;
			last_hyphen_end = None;
			word_start = row_start_byte;
			i = row_start_idx;
			continue;
		}
		width_acc += w;
		if is_break_ws(g) {
			last_ws_end = Some(gi + g.len());
			word_start = gi + g.len();
		} else if is_break_hyphen(g) {
			if letter_count(&text[word_start..gi]) >= MIN_WORD_PART {
				last_hyphen_end = Some(gi + g.len());
			}
			word_start = gi + g.len();
		}
		i += 1;
	}
	rows.push(VisualRow {
		line: line_idx,
		start_byte: row_start_byte,
		end_byte: text.len(),
		trailing_hyphen: false,
	});
	rows
}

#[allow(clippy::too_many_arguments)]
fn choose_break(
	text: &str,
	row_start: usize,
	word_start: usize,
	cur_byte: usize,
	cur_idx: usize,
	graphemes: &[(usize, &str)],
	widths: &[usize],
	width_acc: usize,
	cols: usize,
	last_ws_end: Option<usize>,
	last_hyphen_end: Option<usize>,
) -> (usize, bool) {
	if let Some(ws) = last_ws_end.filter(|&b| b > row_start) {
		return (ws, false);
	}
	if let Some(hy) = last_hyphen_end.filter(|&b| b > row_start) {
		let end = word_end_from(text, hy);
		if letter_count(&text[hy..end]) >= MIN_WORD_PART {
			return (hy, false);
		}
	}
	let next_end = word_end_from(text, cur_byte);
	let left_letters = letter_count(&text[word_start..cur_byte]);
	let right_letters = letter_count(&text[cur_byte..next_end]);
	if cols >= 4 && left_letters >= MIN_WORD_PART && right_letters >= MIN_WORD_PART {
		let mut back_i = cur_idx;
		let mut back_acc = width_acc;
		while back_acc > cols - 1 && back_i > 0 {
			back_i -= 1;
			back_acc -= widths[back_i];
		}
		let back_byte = graphemes[back_i].0;
		if letter_count(&text[word_start..back_byte]) >= MIN_WORD_PART && back_byte > row_start {
			return (back_byte, true);
		}
	}
	(cur_byte, false)
}

pub fn hard_wrap(text: &str, cols: u16) -> String {
	let cols = cols.max(1) as usize;
	let mut out = String::with_capacity(text.len() + text.len() / cols.max(1));
	for (i, line) in text.split('\n').enumerate() {
		if i > 0 {
			out.push('\n');
		}
		let rows = wrap_line(0, line, cols as u16);
		for (j, r) in rows.iter().enumerate() {
			if j > 0 {
				out.push('\n');
			}
			out.push_str(&line[r.start_byte..r.end_byte]);
			if r.trailing_hyphen {
				out.push('-');
			}
		}
	}
	out
}

pub fn wrap_display(text: &str, cols: u16) -> Vec<String> {
	let mut out = Vec::new();
	for line in text.split('\n') {
		let rows = wrap_line(0, line, cols);
		if rows.is_empty() {
			out.push(String::new());
			continue;
		}
		for r in rows {
			let mut s = line[r.start_byte..r.end_byte].to_string();
			if r.trailing_hyphen {
				s.push('-');
			}
			out.push(s);
		}
	}
	out
}
