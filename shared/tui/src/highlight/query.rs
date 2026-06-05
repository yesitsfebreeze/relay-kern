//! Embedded tree-sitter query strings, one per supported language.
//!
//! The `.scm` files live alongside this module in `queries/`. Bundling
//! them with `include_str!` keeps the highlighter self-contained — no
//! runtime filesystem lookups, no risk of drift between binary and
//! shipped queries.

use crate::highlight::lang::Language;

const RUST: &str = include_str!("queries/rust.scm");
const TYPESCRIPT: &str = include_str!("queries/typescript.scm");
const JAVASCRIPT: &str = include_str!("queries/javascript.scm");
const PYTHON: &str = include_str!("queries/python.scm");
const TOML: &str = include_str!("queries/toml.scm");
const MARKDOWN: &str = include_str!("queries/markdown.scm");

/// Return the embedded highlight query source for `lang`.
pub fn query_for(lang: Language) -> &'static str {
	match lang {
		Language::Rust       => RUST,
		Language::TypeScript => TYPESCRIPT,
		Language::JavaScript => JAVASCRIPT,
		Language::Python     => PYTHON,
		Language::Toml       => TOML,
		Language::Markdown   => MARKDOWN,
	}
}
