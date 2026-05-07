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

/// Tmp-file convention: append `.tmp` to the final path. Same directory
/// (same volume) so `rename` is atomic on both Windows and Unix.
fn tmp_path(path: &Path) -> PathBuf {
	let mut p = path.as_os_str().to_owned();
	p.push(".tmp");
	PathBuf::from(p)
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
	let mut p = path.as_os_str().to_owned();
	p.push(".quant");
	PathBuf::from(p)
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
		if let Ok(mut k) = load_kern(dir, id) {
			migrate_root_id(&mut k, &network_id);
			kerns.insert(k.id.clone(), k);
		}
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

	let mut merged_root = g
		.map()
		.get(&root_id)
		.cloned()
		.unwrap_or_else(|| g.root.clone());
	merged_root.id = g.root.id.clone();
	merged_root.root_id = g.root.root_id.clone();
	merged_root.purpose_text = g.root.purpose_text.clone();
	merged_root.purpose_vec = g.root.purpose_vec.clone();
	merged_root.inner_radius = g.root.inner_radius;
	merged_root.outer_radius = g.root.outer_radius;
	for (k, v) in &g.root.descriptors {
		merged_root.descriptors.insert(k.clone(), v.clone());
	}
	save_kern(&g.data_dir, &merged_root)?;
	save_network_id(&g.data_dir, &g.network_id);
	write_quant_mode(&quant_dir_sidecar(&g.data_dir), g.quant_mode);
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
