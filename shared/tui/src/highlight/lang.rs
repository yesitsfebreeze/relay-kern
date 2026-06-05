//! Languages supported by the highlighter and how to recognise them.

/// Languages the highlighter understands. Adding a new variant requires
/// (a) a grammar crate dep, (b) a `.scm` query under `queries/`, and
/// (c) a branch in [`crate::highlight::query::query_for`] /
/// [`crate::highlight::Highlighter::language_fn`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Language {
	Rust,
	TypeScript,
	JavaScript,
	Python,
	Toml,
	Markdown,
}

impl Language {
	/// Resolve a file extension (without the leading dot, case-insensitive)
	/// to a supported language. Returns `None` for unknown extensions so
	/// the caller can fall back to plain rendering.
	pub fn from_extension(ext: &str) -> Option<Self> {
		// Lowercase compare without allocating for the common case.
		let lower: String = ext.chars().map(|c| c.to_ascii_lowercase()).collect();
		Some(match lower.as_str() {
			"rs" => Self::Rust,
			"ts" | "tsx" => Self::TypeScript,
			"js" | "jsx" | "mjs" | "cjs" => Self::JavaScript,
			"py" | "pyi" => Self::Python,
			"toml" => Self::Toml,
			"md" | "markdown" => Self::Markdown,
			_ => return None,
		})
	}
}
