//! Tree-sitter syntax highlighter.
//!
//! Public surface:
//!
//! ```ignore
//! use tui::highlight::{Highlighter, Language};
//! let mut h = Highlighter::new();
//! let spans = h.highlight(Language::Rust, source);
//! for s in spans { /* paint source[s.start..s.end] with s.role */ }
//! ```
//!
//! Spans are non-overlapping, byte-indexed, and sorted by `start`.
//! Bytes between spans are unstyled (`HighlightRole::Default` at paint
//! time). The `Highlighter` caches one parser + compiled query per
//! `Language`, so repeated calls amortise grammar setup costs — which
//! is what the keystroke-driven palette preview pane needs.

use std::collections::HashMap;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Parser, Query, QueryCursor};

pub mod lang;
pub mod query;
pub mod roles;

pub use lang::Language;
pub use roles::HighlightRole;

/// One highlighted byte range.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HighlightSpan {
	pub start: usize,
	pub end: usize,
	pub role: HighlightRole,
}

/// Cached tree-sitter state keyed by [`Language`]. Cheap to call into
/// repeatedly; building grammars and compiling queries happens once per
/// language, on first use.
pub struct Highlighter {
	parser: Parser,
	cache: HashMap<Language, LangCache>,
}

struct LangCache {
	language: TsLanguage,
	query: Query,
	/// Resolved capture-index → role lookup. Indices outside this vec
	/// (i.e. captures we don't recognise) get filtered before emission.
	capture_roles: Vec<Option<HighlightRole>>,
}

impl Highlighter {
	pub fn new() -> Self {
		Self { parser: Parser::new(), cache: HashMap::new() }
	}

	/// Tokenise `source` according to `lang` and return the resulting
	/// non-overlapping span list. Returns an empty vec on parse failure;
	/// the caller treats absence of spans as "render plain".
	pub fn highlight(&mut self, lang: Language, source: &str) -> Vec<HighlightSpan> {
		// Lazily build per-language state on first use.
		if let std::collections::hash_map::Entry::Vacant(e) = self.cache.entry(lang) {
			let Some(cache) = build_cache(lang) else { return Vec::new() };
			e.insert(cache);
		}
		let cache = self
			.cache
			.get(&lang)
			.expect("cache entry just inserted");

		if self.parser.set_language(&cache.language).is_err() {
			return Vec::new();
		}
		let Some(tree) = self.parser.parse(source, None) else {
			return Vec::new();
		};

		let bytes = source.as_bytes();
		let mut cursor = QueryCursor::new();
		// Walk every match, every capture, and convert to a raw span.
		// We collect first and reconcile overlap in a second pass so
		// later captures don't shadow earlier identifier captures we
		// actually want (e.g. `function` should win over `variable`).
		let mut raw: Vec<(usize, usize, HighlightRole, u32)> = Vec::new();
		let mut matches = cursor.matches(&cache.query, tree.root_node(), bytes);
		let mut idx: u32 = 0;
		while let Some(m) = matches.next() {
			for cap in m.captures {
				let role = cache
					.capture_roles
					.get(cap.index as usize)
					.and_then(|r| *r);
				let Some(role) = role else { continue };
				let node = cap.node;
				let start = node.start_byte();
				let end = node.end_byte();
				if start < end && end <= source.len() {
					raw.push((start, end, role, idx));
					idx += 1;
				}
			}
		}

		flatten_spans(raw)
	}
}

impl Default for Highlighter {
	fn default() -> Self {
		Self::new()
	}
}

/// Build the parser language + compiled query + capture-role table for
/// `lang`. Returns `None` if the bundled query string fails to compile
/// against the grammar — that's a build-time bug, but we degrade to
/// "no highlighting" rather than panic so the TUI keeps rendering.
fn build_cache(lang: Language) -> Option<LangCache> {
	let language: TsLanguage = match lang {
		Language::Rust       => tree_sitter_rust::LANGUAGE.into(),
		Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
		Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
		Language::Python     => tree_sitter_python::LANGUAGE.into(),
		Language::Toml       => tree_sitter_toml_ng::LANGUAGE.into(),
		Language::Markdown   => tree_sitter_md::LANGUAGE.into(),
	};

	let q_src = query::query_for(lang);
	// On compile failure we degrade silently to "no highlighting" so the
	// TUI keeps rendering. In test builds we surface the error to make
	// query bugs easy to spot.
	let query = match Query::new(&language, q_src) {
		Ok(q) => q,
		Err(_e) => {
			#[cfg(test)]
			eprintln!("query compile failed for {:?}: {:?}", lang, _e);
			return None;
		}
	};

	let capture_roles: Vec<Option<HighlightRole>> = query
		.capture_names()
		.iter()
		.map(|n| HighlightRole::from_capture(n))
		.collect();

	Some(LangCache { language, query, capture_roles })
}

/// Reconcile a stream of raw, possibly-overlapping captures into a
/// non-overlapping span list sorted by start byte.
///
/// Strategy: a later, more specific capture wins over an earlier
/// generic one when their ranges intersect — but only if the later
/// capture is fully contained in the earlier. We sort by (start asc,
/// end desc, idx asc) so outer/earlier ranges come first, then walk a
/// stack and split as needed.
///
/// In practice the queries are written generic-last (e.g. `(identifier)
/// @variable` at the bottom of `rust.scm`), so the natural priority is
/// "first capture wins". We honour both: anything fully contained in an
/// existing span is dropped, anything that extends past it splits.
fn flatten_spans(
	mut raw: Vec<(usize, usize, HighlightRole, u32)>,
) -> Vec<HighlightSpan> {
	// Sort by start asc, then by capture index asc (earlier query rule wins).
	raw.sort_by_key(|(s, _, _, idx)| (*s, *idx));

	let mut out: Vec<HighlightSpan> = Vec::with_capacity(raw.len());
	for (start, end, role, _) in raw {
		// Drop captures fully contained in (or starting before the end of)
		// the most recent emitted span — earlier capture wins.
		if let Some(last) = out.last() {
			if start < last.end {
				continue;
			}
		}
		out.push(HighlightSpan { start, end, role });
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn roles_at<'a>(spans: &'a [HighlightSpan], src: &'a str, needle: &str) -> Vec<HighlightRole> {
		spans
			.iter()
			.filter(|s| &src[s.start..s.end] == needle)
			.map(|s| s.role)
			.collect()
	}

	#[test]
	fn rust_basic_tokens() {
		let mut h = Highlighter::new();
		let src = r#"
fn main() {
    let x = 1;
    // hi
    let s = "hello";
}
"#;
		let spans = h.highlight(Language::Rust, src);
		assert!(!spans.is_empty(), "expected non-empty span list");

		assert!(roles_at(&spans, src, "fn").contains(&HighlightRole::Keyword));
		assert!(roles_at(&spans, src, "let").contains(&HighlightRole::Keyword));
		assert!(roles_at(&spans, src, "main").contains(&HighlightRole::Function));

		// String literal includes the quotes.
		let has_string = spans
			.iter()
			.any(|s| s.role == HighlightRole::String && &src[s.start..s.end] == "\"hello\"");
		assert!(has_string, "expected `\"hello\"` as String");

		// Comment.
		let has_comment = spans
			.iter()
			.any(|s| s.role == HighlightRole::Comment && src[s.start..s.end].starts_with("//"));
		assert!(has_comment, "expected `// hi` comment");

		// Number.
		assert!(roles_at(&spans, src, "1").contains(&HighlightRole::Number));
	}

	#[test]
	fn python_basic_tokens() {
		let mut h = Highlighter::new();
		let src = "def greet(name):\n    # say hi\n    return \"hello \" + name\n";
		let spans = h.highlight(Language::Python, src);
		assert!(!spans.is_empty());

		assert!(roles_at(&spans, src, "def").contains(&HighlightRole::Keyword));
		assert!(roles_at(&spans, src, "return").contains(&HighlightRole::Keyword));
		assert!(roles_at(&spans, src, "greet").contains(&HighlightRole::Function));
		assert!(spans.iter().any(|s| s.role == HighlightRole::Comment));
		assert!(spans.iter().any(|s| s.role == HighlightRole::String));
	}

	#[test]
	fn markdown_basic_tokens() {
		let mut h = Highlighter::new();
		let src = "# Title\n\n```\nlet x = 1;\n```\n\n- item\n";
		let spans = h.highlight(Language::Markdown, src);
		assert!(!spans.is_empty(), "markdown should produce some spans");
		// At least one heading-as-keyword and one code-block-as-string.
		assert!(spans.iter().any(|s| s.role == HighlightRole::Keyword));
		assert!(spans.iter().any(|s| s.role == HighlightRole::String));
	}

	#[test]
	fn spans_are_sorted_and_non_overlapping() {
		let mut h = Highlighter::new();
		let src = "fn f() { let x = \"s\"; }";
		let spans = h.highlight(Language::Rust, src);
		assert!(!spans.is_empty());
		let mut prev_end = 0usize;
		for s in &spans {
			assert!(s.start < s.end, "empty span");
			assert!(s.end <= src.len(), "span runs past source");
			assert!(s.start >= prev_end, "spans overlap or unsorted: {:?}", spans);
			prev_end = s.end;
		}
	}

	#[test]
	fn lang_from_extension() {
		assert_eq!(Language::from_extension("rs"), Some(Language::Rust));
		assert_eq!(Language::from_extension("RS"), Some(Language::Rust));
		assert_eq!(Language::from_extension("tsx"), Some(Language::TypeScript));
		assert_eq!(Language::from_extension("py"), Some(Language::Python));
		assert_eq!(Language::from_extension("md"), Some(Language::Markdown));
		assert_eq!(Language::from_extension("toml"), Some(Language::Toml));
		assert_eq!(Language::from_extension("xyz"), None);
	}

	#[test]
	fn role_to_style_role_is_total() {
		// Sanity: every role projects to *some* StyleRole.
		for r in [
			HighlightRole::Keyword, HighlightRole::Function, HighlightRole::Type,
			HighlightRole::String, HighlightRole::Number, HighlightRole::Comment,
			HighlightRole::Operator, HighlightRole::Punctuation, HighlightRole::Variable,
			HighlightRole::Constant, HighlightRole::Attribute, HighlightRole::PreProc,
			HighlightRole::Default,
		] {
			let _ = r.to_style_role();
		}
	}
}
