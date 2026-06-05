use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Composite ignore matcher: per-root `.gitignore` + `.kernignore`.
///
/// Reuses the `ignore` crate (already a dep of `shared/search`) so we don't
/// duplicate gitignore semantics. Matchers are evaluated per-root: an event
/// is ignored if the path falls inside a root and the matcher for that root
/// reports a match.
pub struct IgnoreRules {
	per_root: Vec<RootRules>,
}

struct RootRules {
	root: PathBuf,
	gitignore: Option<Gitignore>,
	kernignore: Option<Gitignore>,
}

impl IgnoreRules {
	/// Build matchers by reading `<root>/.gitignore` and `<root>/.kernignore`
	/// for every root. Missing files are silently skipped (no rules).
	pub fn from_roots(roots: &[PathBuf]) -> Self {
		let per_root = roots
			.iter()
			.map(|r| {
				let root = r.clone();
				let gitignore = build(&root, ".gitignore");
				let kernignore = build(&root, ".kernignore");
				RootRules { root, gitignore, kernignore }
			})
			.collect();
		Self { per_root }
	}

	/// Empty matcher — nothing is ignored. Useful for tests.
	pub fn empty() -> Self {
		Self { per_root: Vec::new() }
	}

	/// Returns true if `path` should be skipped.
	pub fn is_ignored(&self, path: &Path) -> bool {
		// `.git/**` is always skipped — bursty internal churn we don't index.
		if path.components().any(|c| c.as_os_str() == ".git") {
			return true;
		}
		for rules in &self.per_root {
			let Ok(rel) = path.strip_prefix(&rules.root) else { continue };
			// `is_dir = false` is fine: notify gives us file-shaped events.
			if let Some(g) = &rules.gitignore {
				if g.matched(rel, false).is_ignore() {
					return true;
				}
			}
			if let Some(g) = &rules.kernignore {
				if g.matched(rel, false).is_ignore() {
					return true;
				}
			}
		}
		false
	}
}

fn build(root: &Path, file: &str) -> Option<Gitignore> {
	let path = root.join(file);
	if !path.is_file() {
		return None;
	}
	let mut b = GitignoreBuilder::new(root);
	if b.add(&path).is_some() {
		// `add` returns `Some(error)` on failure; treat as no rules.
		return None;
	}
	b.build().ok()
}
