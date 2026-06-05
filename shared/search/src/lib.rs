use std::path::{Path, PathBuf};

use grep_regex::RegexMatcher;
use grep_searcher::{Searcher, Sink, SinkMatch};
use ignore::WalkBuilder;
use nucleo_matcher::{
	pattern::{CaseMatching, Normalization, Pattern},
	Config, Matcher as NucleoMatcher, Utf32Str,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
	pub path: PathBuf,
	pub line: Option<u64>,
	pub snippet: Option<String>,
	pub score: i64,
}

pub fn walk_dirs(scopes: &[PathBuf], limit: usize) -> Vec<SearchHit> {
	walk_kind(scopes, limit, Kind::Dir)
}

pub fn walk_files(scopes: &[PathBuf], limit: usize) -> Vec<SearchHit> {
	walk_kind(scopes, limit, Kind::File)
}

pub fn grep(scopes: &[PathBuf], regex: &str, limit: usize) -> Vec<SearchHit> {
	if limit == 0 || scopes.is_empty() {
		return Vec::new();
	}
	let matcher = match RegexMatcher::new(regex) {
		Ok(m) => m,
		Err(_) => return Vec::new(),
	};
	let mut hits = Vec::new();
	'outer: for root in scopes {
		for entry in WalkBuilder::new(root).build().flatten() {
			if !entry.file_type().is_some_and(|t| t.is_file()) {
				continue;
			}
			let path = entry.path().to_path_buf();
			let mut sink = GrepSink {
				path: &path,
				out: &mut hits,
				limit,
				stop: false,
			};
			let _ = Searcher::new().search_path(&matcher, &path, &mut sink);
			if sink.stop {
				break 'outer;
			}
		}
	}
	hits
}

pub fn fuzzy(mut hits: Vec<SearchHit>, pattern: &str, limit: usize) -> Vec<SearchHit> {
	if pattern.is_empty() {
		hits.truncate(limit);
		return hits;
	}
	let mut matcher = NucleoMatcher::new(Config::DEFAULT.match_paths());
	let parsed = Pattern::parse(pattern, CaseMatching::Smart, Normalization::Smart);
	let mut buf = Vec::new();
	let mut scored: Vec<SearchHit> = hits
		.into_iter()
		.filter_map(|mut h| {
			let name = h
				.path
				.file_name()
				.map(|s| s.to_string_lossy().into_owned())
				.unwrap_or_else(|| h.path.to_string_lossy().into_owned());
			buf.clear();
			let haystack = Utf32Str::new(&name, &mut buf);
			let score = parsed.score(haystack, &mut matcher)?;
			h.score = score as i64;
			Some(h)
		})
		.collect();
	scored.sort_by(|a, b| b.score.cmp(&a.score));
	scored.truncate(limit);
	scored
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Kind {
	Dir,
	File,
}

fn walk_kind(scopes: &[PathBuf], limit: usize, kind: Kind) -> Vec<SearchHit> {
	if limit == 0 || scopes.is_empty() {
		return Vec::new();
	}
	let mut hits = Vec::new();
	'outer: for root in scopes {
		for entry in WalkBuilder::new(root).build().flatten() {
			let matched = match kind {
				Kind::Dir => entry.file_type().is_some_and(|t| t.is_dir()),
				Kind::File => entry.file_type().is_some_and(|t| t.is_file()),
			};
			if !matched {
				continue;
			}
			hits.push(SearchHit {
				path: entry.path().to_path_buf(),
				line: None,
				snippet: None,
				score: 0,
			});
			if hits.len() >= limit {
				break 'outer;
			}
		}
	}
	hits
}

struct GrepSink<'a> {
	path: &'a Path,
	out: &'a mut Vec<SearchHit>,
	limit: usize,
	stop: bool,
}

impl<'a> Sink for GrepSink<'a> {
	type Error = std::io::Error;

	fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
		let snippet = std::str::from_utf8(mat.bytes())
			.unwrap_or("")
			.trim_end_matches(['\n', '\r'])
			.to_string();
		self.out.push(SearchHit {
			path: self.path.to_path_buf(),
			line: mat.line_number(),
			snippet: Some(snippet),
			score: 0,
		});
		if self.out.len() >= self.limit {
			self.stop = true;
			return Ok(false);
		}
		Ok(true)
	}
}
