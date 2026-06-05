//! `HighlightRole` and its mapping to the shared `StyleRole` palette.
//!
//! Roles are language-agnostic semantic categories produced by the
//! tree-sitter highlighter. Consumers (the repl preview pane, etc.)
//! map them onto concrete cell styles via `to_style_role()`.

use crate::render::theme::StyleRole;

/// Semantic syntax category emitted by the highlighter.
///
/// The set is fixed and small on purpose — we map onto the shared
/// `StyleRole` palette which is also small. Per-language grammars
/// produce captures whose names map to one of these via
/// [`HighlightRole::from_capture`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum HighlightRole {
	Keyword,
	Function,
	Type,
	String,
	Number,
	Comment,
	Operator,
	Punctuation,
	Variable,
	Constant,
	Attribute,
	PreProc,
	Default,
}

impl HighlightRole {
	/// Map a tree-sitter capture name (e.g. `keyword`, `function.call`)
	/// onto a role. Unknown captures return `None` so the caller can skip.
	pub fn from_capture(name: &str) -> Option<Self> {
		// Match on the prefix before `.` so `function.call`, `string.special`
		// etc. all collapse to the base role.
		let head = name.split('.').next().unwrap_or(name);
		Some(match head {
			"keyword" => Self::Keyword,
			"function" | "method" => Self::Function,
			"type" | "class" => Self::Type,
			"string" | "char" => Self::String,
			"number" | "float" | "integer" => Self::Number,
			"comment" => Self::Comment,
			"operator" => Self::Operator,
			"punctuation" => Self::Punctuation,
			"variable" | "property" | "parameter" => Self::Variable,
			"constant" | "boolean" => Self::Constant,
			"attribute" | "decorator" => Self::Attribute,
			"preproc" | "include" => Self::PreProc,
			_ => return None,
		})
	}

	/// Project to a paint-time `StyleRole`. The mapping is intentionally
	/// coarse — the shared palette is only nine slots wide, so several
	/// roles reuse the same slot. Tweak the table here to retheme syntax
	/// uniformly across all languages.
	pub fn to_style_role(self) -> StyleRole {
		match self {
			HighlightRole::Keyword     => StyleRole::Focus,
			HighlightRole::Function    => StyleRole::Accent,
			HighlightRole::Type        => StyleRole::Warn,
			HighlightRole::String      => StyleRole::Ok,
			HighlightRole::Number      => StyleRole::Warn,
			HighlightRole::Constant    => StyleRole::Warn,
			HighlightRole::Comment     => StyleRole::Muted,
			HighlightRole::Attribute   => StyleRole::Accent,
			HighlightRole::PreProc     => StyleRole::Accent,
			HighlightRole::Operator    => StyleRole::Text,
			HighlightRole::Punctuation => StyleRole::Muted,
			HighlightRole::Variable    => StyleRole::Text,
			HighlightRole::Default     => StyleRole::Text,
		}
	}
}
