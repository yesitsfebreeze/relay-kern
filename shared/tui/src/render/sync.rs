use std::env;

pub fn detect_sync_update_support() -> bool {
	if env::var_os("WT_SESSION").is_some() {
		return true;
	}
	if let Ok(tp) = env::var("TERM_PROGRAM") {
		match tp.as_str() {
			"iTerm.app" | "WezTerm" | "vscode" => return true,
			_ => {}
		}
	}
	if let Ok(term) = env::var("TERM") {
		match term.as_str() {
			"xterm-kitty" | "foot" | "foot-extra" => return true,
			_ => {}
		}
	}
	false
}
