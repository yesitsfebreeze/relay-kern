// Generic TOML load/save/layer for per-binary configs.
// User scope: <XDG_CONFIG>/relay/<bin>.toml
// Project scope: <cwd>/.relay/<bin>.toml
// Section-level merge: project TOML overrides whole sections; missing
// sections fall through to user, then defaults.

use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
	#[error("io: {0}")]
	Io(String),
	#[error("parse: {0}")]
	Parse(String),
	#[error("serialize: {0}")]
	Serialize(String),
	#[error("no config dir")]
	NoConfigDir,
}

pub fn user_dir() -> Result<PathBuf, Error> {
	dirs::config_dir().ok_or(Error::NoConfigDir).map(|p| p.join("relay"))
}

pub fn project_dir(cwd: &Path) -> PathBuf {
	cwd.join(".relay")
}

pub fn load<T: DeserializeOwned + Default>(path: &Path) -> Result<T, Error> {
	match std::fs::read_to_string(path) {
		Ok(text) => toml::from_str(&text).map_err(|e| Error::Parse(e.to_string())),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
		Err(e) => Err(Error::Io(e.to_string())),
	}
}

pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), Error> {
	let text = toml::to_string_pretty(value).map_err(|e| Error::Serialize(e.to_string()))?;
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent).map_err(|e| Error::Io(e.to_string()))?;
	}
	std::fs::write(path, text).map_err(|e| Error::Io(e.to_string()))
}

pub fn load_layered<T: DeserializeOwned + Default>(user: &Path, project: &Path) -> Result<T, Error> {
	let user_v = read_value(user)?;
	let project_v = read_value(project)?;
	let merged = merge_sections(user_v, project_v);
	merged.try_into().map_err(|e: toml::de::Error| Error::Parse(e.to_string()))
}

fn read_value(path: &Path) -> Result<toml::Value, Error> {
	match std::fs::read_to_string(path) {
		// Parse as a document TABLE, not a bare `toml::Value`. A bare-value
		// parse misreads a leading `[section]` header as an array literal
		// ("unexpected content"), so any real config file starting with a
		// section would fail to load. Parsing into `toml::Table` treats the
		// input as a document.
		Ok(text) => text
			.parse::<toml::Table>()
			.map(toml::Value::Table)
			.map_err(|e| Error::Parse(e.to_string())),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(toml::Value::Table(toml::value::Table::new())),
		Err(e) => Err(Error::Io(e.to_string())),
	}
}

fn merge_sections(base: toml::Value, over: toml::Value) -> toml::Value {
	match (base, over) {
		(toml::Value::Table(mut a), toml::Value::Table(b)) => {
			for (k, v) in b {
				a.insert(k, v);
			}
			toml::Value::Table(a)
		}
		(_, over) => over,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Regression: a document whose first line is a `[section]` header must
	/// parse as a table, not be misread as an array literal. Before the fix,
	/// `read_value` used `parse::<toml::Value>()` which failed on any real
	/// config file (every project `.relay/*.toml`), silently disabling
	/// project-scope config.
	#[test]
	fn read_value_parses_leading_section_header() {
		let dir = std::env::temp_dir().join(format!("cfgio_rv_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let p = dir.join("c.toml");
		std::fs::write(&p, "[section]\nenabled = true\n").unwrap();
		let v = read_value(&p).expect("read_value should parse a document");
		let enabled = v
			.get("section")
			.and_then(|s| s.get("enabled"))
			.and_then(|b| b.as_bool());
		assert_eq!(enabled, Some(true));
		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn load_layered_merges_project_section_over_missing_user() {
		let dir = std::env::temp_dir().join(format!("cfgio_ll_{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let user = dir.join("user.toml"); // intentionally absent
		let project = dir.join("project.toml");
		std::fs::write(&project, "[section]\nenabled = true\n").unwrap();
		let merged: toml::Table = load_layered(&user, &project).expect("load_layered");
		let enabled = merged
			.get("section")
			.and_then(|s| s.get("enabled"))
			.and_then(|b| b.as_bool());
		assert_eq!(enabled, Some(true));
		let _ = std::fs::remove_dir_all(&dir);
	}
}
