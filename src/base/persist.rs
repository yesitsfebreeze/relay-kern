// SLICE A NOTE — Entity rename is a clean break. The on-disk bincode layout
// changed (`Thought` -> `Entity`, `ThoughtKind` -> `EntityKind` + `EntityStatus`,
// `SourceRef` -> typed `Source` enum, `Kern.thoughts` -> `Kern.entities`,
// `created_at` moved off `Source` onto `Entity`). Old saved DBs are NOT
// migrated — they must be regenerated. CLAUDE.md mandates "no compat".
use super::graph::{migrate_root_id, GraphGnn};
use super::types::Kern;
use super::util;
use crate::quant::QuantizationMode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
	#[error("io: {0}")]
	Io(#[from] std::io::Error),
	#[error("bincode encode: {0}")]
	BincodeEncode(#[from] bincode::error::EncodeError),
	#[error("bincode decode: {0}")]
	BincodeDecode(#[from] bincode::error::DecodeError),
	#[error("missing node: {0}")]
	MissingNode(String),
	#[error("atomic rename {tmp:?} -> {dst:?}: {source}")]
	TmpRename {
		tmp: PathBuf,
		dst: PathBuf,
		#[source]
		source: std::io::Error,
	},
}

/// Cap bincode-decoded allocations at 1 GiB. Without a limit, a corrupt
/// or fuzzed length prefix can trick `decode_from_slice` into requesting
/// petabytes (observed: a 5 EiB allocation on random bytes from
/// `tests/persist_fuzz.rs`). Real kern snapshots are far smaller — even
/// the largest deployments stay well under this cap — so the limit only
/// rejects pathological inputs while permitting all real-world data.
fn bincode_cfg() -> impl bincode::config::Config {
	bincode::config::standard().with_limit::<{ 1024 * 1024 * 1024 }>()
}

/// Append a literal suffix to a path (whole filename, not the extension).
/// Stays in the same directory so a subsequent `rename` is atomic.
fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
	let mut p = path.as_os_str().to_owned();
	p.push(suffix);
	PathBuf::from(p)
}

/// Tmp-file convention: append `.tmp` to the final path. Same directory
/// (same volume) so `rename` is atomic on both Windows and Unix.
fn tmp_path(path: &Path) -> PathBuf {
	append_suffix(path, ".tmp")
}

/// Crash-atomic write: write `data` to `{path}.tmp`, fsync, then rename.
/// Existing `.tmp` siblings are overwritten (treated as stale).
fn atomic_write(path: &Path, data: &[u8]) -> Result<(), PersistError> {
	let tmp = tmp_path(path);
	{
		let mut f = fs::File::create(&tmp)?;
		f.write_all(data)?;
		f.sync_all()?;
	}
	if let Err(source) = fs::rename(&tmp, path) {
		// best-effort cleanup; surface original rename error
		let _ = fs::remove_file(&tmp);
		return Err(PersistError::TmpRename {
			tmp,
			dst: path.to_path_buf(),
			source,
		});
	}
	Ok(())
}

/// Sweep stray `*.tmp` files in `dir` left by a crashed prior write.
/// Logs and deletes; never recovers from a tmp.
fn sweep_stale_tmp(dir: &Path) {
	let entries = match fs::read_dir(dir) {
		Ok(e) => e,
		Err(_) => return,
	};
	for entry in entries.flatten() {
		let path = entry.path();
		if path.extension() == Some(OsStr::new("tmp")) {
			tracing::warn!(
				target: "kern::persist",
				tmp = %path.display(),
				"removing stale .tmp file from incomplete prior write"
			);
			let _ = fs::remove_file(&path);
		}
	}
}

#[derive(Serialize, Deserialize)]
struct SavedState {
	root: Kern,
	network_id: String,
	kerns: HashMap<String, Kern>,
	unloaded: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct QuantMeta {
	mode: QuantizationMode,
}

fn quant_sidecar_path(path: &Path) -> PathBuf {
	append_suffix(path, ".quant")
}

fn quant_dir_sidecar(dir: &str) -> PathBuf {
	Path::new(dir).join("_quant.meta")
}

fn read_quant_mode(sidecar: &Path) -> QuantizationMode {
	let data = match fs::read(sidecar) {
		Ok(d) => d,
		Err(_) => return QuantizationMode::None,
	};
	bincode::serde::decode_from_slice::<QuantMeta, _>(&data, bincode_cfg())
		.map(|(m, _)| m.mode)
		.unwrap_or(QuantizationMode::None)
}

fn write_quant_mode(sidecar: &Path, mode: QuantizationMode) {
	let meta = QuantMeta { mode };
	if let Ok(data) = bincode::serde::encode_to_vec(&meta, bincode_cfg()) {
		let _ = atomic_write(sidecar, &data);
	}
}

pub fn save(g: &GraphGnn, path: &Path) -> Result<(), PersistError> {
	let state = SavedState {
		root: g.root.clone(),
		network_id: g.network_id.clone(),
		kerns: g.map().clone(),
		unloaded: g.unloaded_ids(),
	};
	let data = bincode::serde::encode_to_vec(&state, bincode_cfg())?;
	atomic_write(path, &data)?;
	write_quant_mode(&quant_sidecar_path(path), g.quant_mode);
	Ok(())
}

pub fn load(path: &Path) -> Result<GraphGnn, PersistError> {
	if let Some(parent) = path.parent() {
		if !parent.as_os_str().is_empty() {
			sweep_stale_tmp(parent);
		}
	}
	let data = fs::read(path)?;
	let (mut state, _): (SavedState, _) = bincode::serde::decode_from_slice(&data, bincode_cfg())?;

	let unloaded: std::collections::HashSet<String> = state.unloaded.into_iter().collect();

	let root = state
		.kerns
		.get(&state.root.id)
		.cloned()
		.unwrap_or(state.root);
	let mut network_id = state.network_id;
	if network_id.is_empty() {
		network_id = util::uuid_v4();
	}

	for k in state.kerns.values_mut() {
		migrate_root_id(k, &network_id);
		backfill_created_at(k);
	}

	let quant_mode = read_quant_mode(&quant_sidecar_path(path));
	let g = GraphGnn::from_saved_with_mode(
		root,
		network_id,
		String::new(),
		state.kerns,
		unloaded,
		quant_mode,
	);
	Ok(g)
}

pub fn save_kern(dir: &str, kern: &Kern) -> Result<(), PersistError> {
	let path = Path::new(dir).join(format!("{}.kern", kern.id));
	let data = bincode::serde::encode_to_vec(kern, bincode_cfg())?;
	atomic_write(&path, &data)?;
	Ok(())
}

pub fn load_kern(dir: &str, id: &str) -> Result<Kern, PersistError> {
	let path = Path::new(dir).join(format!("{id}.kern"));
	let data = fs::read(path)?;
	let (mut kern, _): (Kern, _) = bincode::serde::decode_from_slice(&data, bincode_cfg())?;
	backfill_created_at(&mut kern);
	Ok(kern)
}

/// Delete a kern's on-disk file. Called when a kern is permanently removed
/// (`GraphGnn::deregister`) so a reaped kern does not resurrect on the next
/// `load_dir` — `load_dir` reads every `*.kern` in the directory, so a leftover
/// file IS a live kern as far as the next start is concerned. A missing file is
/// success (idempotent). Never touches the root or `_meta`.
pub fn delete_kern(dir: &str, id: &str) {
	if dir.is_empty() || id == "_meta" {
		return;
	}
	let path = Path::new(dir).join(format!("{id}.kern"));
	match fs::remove_file(&path) {
		Ok(()) => {}
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
		Err(e) => tracing::warn!(target: "kern.persist", kern = %id, error = %e, "failed to delete kern file"),
	}
}

fn backfill_created_at(kern: &mut Kern) {
	let now = std::time::SystemTime::now();
	for t in kern.entities.values_mut() {
		if t.created_at.is_none() {
			t.created_at = Some(now);
		}
	}
}

pub fn load_dir(dir: &str) -> Result<GraphGnn, PersistError> {
	sweep_stale_tmp(Path::new(dir));
	let mut root = load_kern(dir, "root")?;
	let mut network_id = load_network_id(dir);
	if network_id.is_empty() {
		network_id = util::uuid_v4();
	}
	migrate_root_id(&mut root, &network_id);

	let mut kerns = HashMap::new();
	let root_id = root.id.clone();
	kerns.insert(root_id.clone(), root);

	let unloaded = std::collections::HashSet::new();
	let mut skipped = 0usize;
	for entry in fs::read_dir(dir)? {
		let entry = entry?;
		let name = entry.file_name().to_string_lossy().to_string();
		if !name.ends_with(".kern") {
			continue;
		}
		let id = &name[..name.len() - ".kern".len()];
		if id == root_id || id == "_meta" {
			continue;
		}
		match load_kern(dir, id) {
			Ok(mut k) => {
				migrate_root_id(&mut k, &network_id);
				kerns.insert(k.id.clone(), k);
			}
			// A corrupt/unreadable sibling must not vanish silently: warn so the
			// orphaned data is visible rather than producing a quietly truncated
			// graph. The root is loaded above and still hard-errors.
			Err(e) => {
				skipped += 1;
				tracing::warn!(target: "kern.persist", kern = %id, error = %e, "skipping corrupt/unreadable kern file");
			}
		}
	}
	if skipped > 0 {
		tracing::warn!(target: "kern.persist", skipped, dir = %dir, "load_dir skipped corrupt kern file(s)");
	}

	let root_kern = kerns
		.get(&root_id)
		.ok_or_else(|| PersistError::MissingNode(root_id.clone()))?
		.clone();
	let quant_mode = read_quant_mode(&quant_dir_sidecar(dir));
	let g = GraphGnn::from_saved_with_mode(
		root_kern,
		network_id,
		dir.to_string(),
		kerns,
		unloaded,
		quant_mode,
	);
	Ok(g)
}

/// The canonical on-disk root kern: the in-memory map entry overlaid with the
/// authoritative root-only fields from `g.root` (id, root_id, purpose, radii,
/// descriptors). Both `save_all` and the tick worker's per-kern persist write
/// the root through this, so a `Persist` task targeting the root can never
/// clobber those fields with a stale map entry.
pub fn merged_root(g: &GraphGnn) -> Kern {
	let root_id = g.root.id.clone();
	let mut merged = g
		.map()
		.get(&root_id)
		.cloned()
		.unwrap_or_else(|| g.root.clone());
	merged.id = g.root.id.clone();
	merged.root_id = g.root.root_id.clone();
	merged.anchor_text = g.root.anchor_text.clone();
	merged.anchor_vec = g.root.anchor_vec.clone();
	merged.inner_radius = g.root.inner_radius;
	merged.outer_radius = g.root.outer_radius;
	for (k, v) in &g.root.descriptors {
		merged.descriptors.insert(k.clone(), v.clone());
	}
	merged
}

pub fn save_all(g: &GraphGnn) -> Result<(), PersistError> {
	if g.data_dir.is_empty() {
		return Ok(());
	}
	fs::create_dir_all(&g.data_dir)?;
	let root_id = g.root.id.clone();
	for (id, kern) in g.map().iter() {
		if id == &root_id {
			continue;
		}
		save_kern(&g.data_dir, kern)?;
	}

	save_kern(&g.data_dir, &merged_root(g))?;
	save_network_id(&g.data_dir, &g.network_id);
	write_quant_mode(&quant_dir_sidecar(&g.data_dir), g.quant_mode);

	// Prune orphaned kern files: any `<id>.kern` on disk that the live graph no
	// longer knows (neither loaded nor in the unloaded tier) is a stale remnant
	// of a deregistered kern. `load_dir` treats every file as a live kern, so
	// without this a reaped kern resurrects on restart — the mechanism that let
	// the unnamed-child runaway persist tens of thousands of empty kerns. The
	// `delete_kern` in `deregister` handles the common path; this reconciles disk
	// to memory on every full save as a backstop.
	let keep: std::collections::HashSet<String> = g.all_ids().into_iter().collect();
	let root_id = g.root.id.clone();
	if let Ok(entries) = fs::read_dir(&g.data_dir) {
		for entry in entries.flatten() {
			let name = entry.file_name().to_string_lossy().to_string();
			let Some(id) = name.strip_suffix(".kern") else { continue };
			if id == "_meta" || id == root_id || keep.contains(id) {
				continue;
			}
			delete_kern(&g.data_dir, id);
		}
	}
	Ok(())
}

pub fn compress_dir(
	src: &str,
	out_dir: &str,
	target_mode: QuantizationMode,
) -> Result<(), PersistError> {
	let mut g = load_dir(src)?;
	g.quant_mode = target_mode;
	g.data_dir = out_dir.to_string();
	fs::create_dir_all(out_dir)?;
	g.rebuild_index();
	save_all(&g)?;
	Ok(())
}

#[derive(Serialize, Deserialize)]
struct GraphMeta {
	network_id: String,
}

fn save_network_id(dir: &str, network_id: &str) {
	let path = Path::new(dir).join("_meta.kern");
	let meta = GraphMeta {
		network_id: network_id.to_string(),
	};
	if let Ok(data) = bincode::serde::encode_to_vec(&meta, bincode_cfg()) {
		let _ = atomic_write(&path, &data);
	}
}

fn load_network_id(dir: &str) -> String {
	let path = Path::new(dir).join("_meta.kern");
	let data = match fs::read(&path) {
		Ok(d) => d,
		Err(_) => return String::new(),
	};
	match bincode::serde::decode_from_slice::<GraphMeta, _>(&data, bincode_cfg()) {
		Ok((m, _)) => m.network_id,
		Err(_) => String::new(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::base::graph::GraphGnn;
	use tempfile::tempdir;

	#[test]
	fn merged_root_overlays_authoritative_fields_over_stale_map_entry() {
		let mut g = GraphGnn::new();
		// Stale root entry in the map: empty purpose/descriptors.
		let mut stale = g.root.clone();
		stale.anchor_text = String::new();
		stale.descriptors.clear();
		g.register(stale);
		// Authoritative values live on g.root.
		g.root.anchor_text = "guiding purpose".to_string();
		g.root
			.descriptors
			.insert("chat".to_string(), "desc".to_string());

		let merged = merged_root(&g);
		assert_eq!(merged.id, g.root.id);
		assert_eq!(merged.anchor_text, "guiding purpose");
		assert_eq!(merged.descriptors.get("chat").map(String::as_str), Some("desc"));
	}

	#[test]
	fn root_persist_via_merged_root_survives_reload() {
		let dir = tempdir().unwrap();
		let mut g = GraphGnn::new();
		g.data_dir = dir.path().to_string_lossy().to_string();
		g.root.anchor_text = "P".to_string();
		g.root.descriptors.insert("k".to_string(), "v".to_string());

		fs::create_dir_all(&g.data_dir).unwrap();
		// This is what do_persist(root) and save_all now write for the root.
		save_kern(&g.data_dir, &merged_root(&g)).unwrap();

		let reloaded = load_kern(&g.data_dir, &g.root.id).unwrap();
		assert_eq!(reloaded.anchor_text, "P");
		assert_eq!(reloaded.descriptors.get("k").map(String::as_str), Some("v"));
	}

	#[test]
	fn named_kern_with_anchor_vec_round_trips() {
		// Guards the bincode-positional assumption behind the purpose->anchor
		// rename: anchor_text/anchor_vec must survive save/load unchanged. If a
		// future edit reorders Kern's fields, the decoded values shift and this
		// fails — catching live-graph corruption before it ships.
		let dir = tempdir().unwrap();
		let d = dir.path().to_string_lossy().to_string();
		let mut k = Kern::new("anchor-work", "root");
		k.anchor_text = "work".to_string();
		k.anchor_vec = vec![0.1, -0.2, 0.3, 0.4];
		k.inner_radius = 0.15;
		k.outer_radius = 0.55;
		save_kern(&d, &k).unwrap();

		let back = load_kern(&d, "anchor-work").unwrap();
		assert_eq!(back.anchor_text, "work");
		assert_eq!(back.anchor_vec, vec![0.1, -0.2, 0.3, 0.4]);
		assert_eq!(back.inner_radius, 0.15);
		assert_eq!(back.outer_radius, 0.55);
		assert!(back.is_named() && back.has_anchor());
	}

	#[test]
	fn load_dir_skips_corrupt_kern_files() {
		let dir = tempdir().unwrap();
		let d = dir.path().to_string_lossy().to_string();
		save_kern(&d, &Kern::new("root", "")).unwrap();
		save_kern(&d, &Kern::new("child1", "root")).unwrap();
		// A corrupt sibling that fails to decode.
		fs::write(format!("{d}/bad.kern"), b"not a valid bincode kern").unwrap();

		let g = load_dir(&d).expect("load_dir tolerates a corrupt sibling");
		assert!(g.loaded("child1").is_some(), "valid sibling still loads");
		assert!(
			g.map().keys().all(|k| k != "bad"),
			"corrupt kern is skipped, not inserted"
		);
	}
}
