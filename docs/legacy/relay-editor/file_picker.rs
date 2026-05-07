use std::fs;
use std::path::{Path, PathBuf};

use tui::input::{Key, KeyCode, Mods};
use tui::render::{Attrs, Cell, Color, FrameView};

const MAX_FILES: usize = 2000;
const MAX_GREP: usize = 500;
const MIN_GREP_LEN: usize = 2;
const LIST_HEIGHT: usize = 7;

static SKIP: &[&str] = &[
    ".git", "target", "node_modules", ".claude", "__pycache__",
    ".venv", "venv", "dist", "build", "out", ".next", ".nuxt",
];

#[derive(Clone, Copy, PartialEq)]
enum DataType { None, FileList, DirList, GrepList }

#[derive(Clone, Copy, PartialEq)]
enum AdapterKind { Files, Dirs, Grep }

struct AdapterDef {
    key: char,
    name: &'static str,
    accepts: &'static [DataType],
    #[allow(dead_code)]
    produces: DataType,
    kind: AdapterKind,
}

static ADAPTERS: &[AdapterDef] = &[
    AdapterDef { key: 'f', name: "Files", accepts: &[DataType::None, DataType::FileList, DataType::DirList, DataType::GrepList], produces: DataType::FileList, kind: AdapterKind::Files },
    AdapterDef { key: 'd', name: "Dirs",  accepts: &[DataType::None, DataType::DirList], produces: DataType::DirList, kind: AdapterKind::Dirs },
    AdapterDef { key: 'g', name: "Grep",  accepts: &[DataType::None, DataType::FileList, DataType::DirList, DataType::GrepList], produces: DataType::GrepList, kind: AdapterKind::Grep },
];

fn adapters_for(t: DataType) -> impl Iterator<Item = &'static AdapterDef> {
    ADAPTERS.iter().filter(move |a| a.accepts.contains(&t))
}

pub enum PickerOutcome { Handled, Open(PathBuf), OpenAt(PathBuf, usize), Cancel }

#[derive(Clone)]
enum EntryData {
    File(PathBuf),
    Dir(PathBuf),
    GrepMatch { path: PathBuf, line: usize },
}

impl EntryData {
    fn path(&self) -> &PathBuf {
        match self { EntryData::File(p) | EntryData::Dir(p) => p, EntryData::GrepMatch { path, .. } => path }
    }
    fn data_type(&self) -> DataType {
        match self { EntryData::File(_) => DataType::FileList, EntryData::Dir(_) => DataType::DirList, EntryData::GrepMatch { .. } => DataType::GrepList }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum StageKind { SelectAdapter, Adapter(AdapterKind) }

impl StageKind {
    fn label(self) -> &'static str {
        match self {
            StageKind::SelectAdapter => "?",
            StageKind::Adapter(AdapterKind::Files) => "Files",
            StageKind::Adapter(AdapterKind::Dirs) => "Dirs",
            StageKind::Adapter(AdapterKind::Grep) => "Grep",
        }
    }
}

enum StageOut { Handled, Submit(EntryData), Chain(EntryData), SelectAdapter(AdapterKind), Back, Cancel }

struct Stage {
    kind: StageKind,
    scope: Option<PathBuf>,
    input_type: DataType,
    display: Vec<String>,
    data: Vec<EntryData>,
    visible: Vec<usize>,
    selected: usize,
    query: String,
    preview_scroll: isize,
}

impl Stage {
    fn select_adapter(scope: Option<PathBuf>, input_type: DataType) -> Self {
        Stage { kind: StageKind::SelectAdapter, scope, input_type, display: vec![], data: vec![], visible: vec![], selected: 0, query: String::new(), preview_scroll: 0 }
    }

    fn files(scope: Option<PathBuf>, cwd: &Path) -> Self {
        let root = scope.as_deref().unwrap_or(cwd);
        let mut display = vec![];
        let mut data = vec![];
        scan_files(root, root, &mut display, &mut data);
        let visible = (0..display.len()).collect();
        Stage { kind: StageKind::Adapter(AdapterKind::Files), scope: None, input_type: DataType::None, display, data, visible, selected: 0, query: String::new(), preview_scroll: 0 }
    }

    fn dirs(scope: Option<PathBuf>, cwd: &Path) -> Self {
        let root = scope.as_deref().unwrap_or(cwd);
        let mut display = vec![];
        let mut data = vec![];
        scan_dirs(root, root, &mut display, &mut data);
        let visible = (0..display.len()).collect();
        Stage { kind: StageKind::Adapter(AdapterKind::Dirs), scope: None, input_type: DataType::None, display, data, visible, selected: 0, query: String::new(), preview_scroll: 0 }
    }

    fn grep(scope: Option<PathBuf>, input_type: DataType) -> Self {
        Stage { kind: StageKind::Adapter(AdapterKind::Grep), scope, input_type, display: vec![], data: vec![], visible: vec![], selected: 0, query: String::new(), preview_scroll: 0 }
    }

    fn selected_entry(&self) -> Option<&EntryData> {
        self.visible.get(self.selected).and_then(|&i| self.data.get(i))
    }

    fn update_fuzzy(&mut self) {
        let prev = self.visible.get(self.selected).copied();
        if self.query.is_empty() {
            self.visible = (0..self.display.len()).collect();
        } else {
            let q = self.query.to_lowercase();
            let mut scored: Vec<(i32, usize)> = self.display.iter().enumerate()
                .filter_map(|(i, s)| fuzzy_score(s, &q).map(|sc| (sc, i))).collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            self.visible = scored.into_iter().map(|(_, i)| i).collect();
        }
        self.selected = prev.and_then(|o| self.visible.iter().position(|&i| i == o)).unwrap_or(0);
        self.preview_scroll = 0;
    }

    fn update(&mut self, cwd: &Path) {
        match self.kind {
            StageKind::SelectAdapter => {}
            StageKind::Adapter(AdapterKind::Files) | StageKind::Adapter(AdapterKind::Dirs) => self.update_fuzzy(),
            StageKind::Adapter(AdapterKind::Grep) => {
                if self.query.len() < MIN_GREP_LEN {
                    self.display.clear(); self.data.clear(); self.visible.clear();
                    self.selected = 0; self.preview_scroll = 0;
                } else {
                    let scope = self.scope.as_deref().unwrap_or(cwd);
                    let (d, dd) = run_grep(cwd, scope, &self.query);
                    self.display = d; self.data = dd;
                    self.visible = (0..self.display.len()).collect();
                    self.selected = 0; self.preview_scroll = 0;
                }
            }
        }
    }

    fn nav(&mut self, delta: isize) {
        let n = self.visible.len();
        if n == 0 { return; }
        self.selected = (self.selected as isize + delta).rem_euclid(n as isize) as usize;
        self.preview_scroll = 0;
    }

    fn key(&mut self, k: &Key, cwd: &Path) -> StageOut {
        match self.kind {
            StageKind::SelectAdapter => self.key_select(k),
            StageKind::Adapter(_) => self.key_prompt(k, cwd),
        }
    }

    fn key_select(&self, k: &Key) -> StageOut {
        match k.code {
            KeyCode::Esc => StageOut::Cancel,
            KeyCode::BackTab => StageOut::Back,
            KeyCode::Backspace if k.mods == Mods::NONE => StageOut::Back,
            KeyCode::Char(c) if k.mods == Mods::NONE || k.mods == Mods::SHIFT => {
                let c_low = c.to_ascii_lowercase();
                if let Some(a) = adapters_for(self.input_type).find(|a| a.key == c_low) {
                    StageOut::SelectAdapter(a.kind)
                } else { StageOut::Handled }
            }
            _ => StageOut::Handled,
        }
    }

    fn key_prompt(&mut self, k: &Key, cwd: &Path) -> StageOut {
        match k.code {
            KeyCode::Esc => StageOut::Cancel,
            KeyCode::Enter => match self.selected_entry().cloned() {
                None => StageOut::Cancel,
                Some(e @ EntryData::Dir(_)) => StageOut::Chain(e),
                Some(e) => StageOut::Submit(e),
            },
            KeyCode::Tab if k.mods == Mods::NONE => self.selected_entry().cloned().map_or(StageOut::Handled, StageOut::Chain),
            KeyCode::BackTab => StageOut::Back,
            KeyCode::Up => { self.nav(-1); StageOut::Handled }
            KeyCode::Down => { self.nav(1); StageOut::Handled }
            KeyCode::Char('u') if k.mods == Mods::CTRL => { self.preview_scroll -= 3; StageOut::Handled }
            KeyCode::Char('d') if k.mods == Mods::CTRL => { self.preview_scroll += 3; StageOut::Handled }
            KeyCode::Backspace if k.mods == Mods::NONE => {
                if self.query.is_empty() { StageOut::Back }
                else { self.query.pop(); self.update(cwd); StageOut::Handled }
            }
            KeyCode::Char(c) if k.mods == Mods::NONE || k.mods == Mods::SHIFT => {
                self.query.push(c); self.update(cwd); StageOut::Handled
            }
            _ => StageOut::Handled,
        }
    }
}

// ── FilePicker ────────────────────────────────────────────────────────────────

pub struct FilePicker {
    chain: Vec<Stage>,
    active: Stage,
    cwd: PathBuf,
}

impl FilePicker {
    pub fn new(cwd: PathBuf) -> Self {
        FilePicker { chain: vec![], active: Stage::select_adapter(None, DataType::None), cwd }
    }

    pub fn handle_key(&mut self, k: &Key) -> PickerOutcome {
        let cwd = self.cwd.clone();
        match self.active.key(k, &cwd) {
            StageOut::Cancel => PickerOutcome::Cancel,
            StageOut::Back => {
                if let Some(prev) = self.chain.pop() { self.active = prev; PickerOutcome::Handled }
                else { PickerOutcome::Cancel }
            }
            StageOut::Submit(entry) => match entry {
                EntryData::File(p) => PickerOutcome::Open(p),
                EntryData::GrepMatch { path, line } => PickerOutcome::OpenAt(path, line),
                EntryData::Dir(_) => unreachable!(),
            },
            StageOut::Chain(entry) => {
                let scope = Some(entry.path().clone());
                let input_type = entry.data_type();
                let new = Stage::select_adapter(scope, input_type);
                let old = std::mem::replace(&mut self.active, new);
                self.chain.push(old);
                PickerOutcome::Handled
            }
            StageOut::SelectAdapter(kind) => {
                let scope = self.active.scope.clone();
                let input_type = self.active.input_type;
                self.active = match kind {
                    AdapterKind::Files => Stage::files(scope, &cwd),
                    AdapterKind::Dirs => Stage::dirs(scope, &cwd),
                    AdapterKind::Grep => Stage::grep(scope, input_type),
                };
                PickerOutcome::Handled
            }
            StageOut::Handled => PickerOutcome::Handled,
        }
    }

    /// Renders the list+preview overlay (everything above the bar).
    pub fn render_overlay(&mut self, view: &mut FrameView<'_>) {
        view.fill(Cell::new(' ').style(Color::Default, Color::Default, Attrs::NONE));
        let cwd = self.cwd.clone();
        match self.active.kind {
            StageKind::SelectAdapter => {
                if let Some(stage) = self.chain.last_mut() {
                    render_stage_overlay(stage, view, &cwd);
                }
                // else: blank — no previous stage yet
            }
            StageKind::Adapter(_) => {
                render_stage_overlay(&mut self.active, view, &cwd);
            }
        }
    }

    /// Renders the single command bar row.
    pub fn render_bar(&self, view: &mut FrameView<'_>) {
        let w = view.width() as usize;
        if w == 0 { return; }

        // chain prefix: committed stage labels, dim
        let chain_prefix: String = if self.chain.is_empty() {
            String::new()
        } else {
            format!("{} > ", self.chain.iter().map(|s| s.kind.label()).collect::<Vec<_>>().join(" > "))
        };

        match self.active.kind {
            StageKind::SelectAdapter => {
                // right: count from previous stage
                let count_str = self.chain.last()
                    .filter(|s| !s.visible.is_empty())
                    .map(|s| format!("  {}/{}", s.selected + 1, s.visible.len()))
                    .unwrap_or_default();
                let count_w = count_str.chars().count();
                let content_w = w.saturating_sub(count_w);
                let mut x = 0usize;

                // chain prefix
                if !chain_prefix.is_empty() && x < content_w {
                    let s: String = chain_prefix.chars().take(content_w - x).collect();
                    view.put_str(x as u16, 0, &s, Color::Default, Color::Default, Attrs::DIM);
                    x += s.chars().count();
                }
                // adapters: first char BOLD, rest DIM, separated by two spaces
                for (i, a) in adapters_for(self.active.input_type).enumerate() {
                    if x >= content_w { break; }
                    if i > 0 {
                        let sp: String = "  ".chars().take(content_w - x).collect();
                        view.put_str(x as u16, 0, &sp, Color::Default, Color::Default, Attrs::NONE);
                        x += sp.chars().count();
                    }
                    if x >= content_w { break; }
                    let first: String = a.name.chars().take(1).collect();
                    view.put_str(x as u16, 0, &first, Color::Default, Color::Default, Attrs::BOLD);
                    x += 1;
                    if x < content_w {
                        let rest: String = a.name.chars().skip(1).take(content_w - x).collect();
                        view.put_str(x as u16, 0, &rest, Color::Default, Color::Default, Attrs::DIM);
                        x += rest.chars().count();
                    }
                }
                // count right-aligned
                if !count_str.is_empty() {
                    let pos = w.saturating_sub(count_w);
                    view.put_str(pos as u16, 0, &count_str, Color::Default, Color::Default, Attrs::DIM);
                }
            }

            StageKind::Adapter(adapter_kind) => {
                let n = self.active.visible.len();
                let count_str = if n > 0 {
                    format!("  {}/{}", self.active.selected + 1, n)
                } else {
                    String::new()
                };
                let count_w = count_str.chars().count();
                let content_w = w.saturating_sub(count_w);
                let mut x = 0usize;

                // chain prefix dim
                if !chain_prefix.is_empty() && x < content_w {
                    let s: String = chain_prefix.chars().take(content_w - x).collect();
                    view.put_str(x as u16, 0, &s, Color::Default, Color::Default, Attrs::DIM);
                    x += s.chars().count();
                }
                // query with cursor
                let query_str = format!("/ {}\u{258c}", self.active.query); // ▌ block cursor
                if x < content_w {
                    let q: String = query_str.chars().take(content_w - x).collect();
                    view.put_str(x as u16, 0, &q, Color::Default, Color::Default, Attrs::BOLD);
                    x += q.chars().count();
                }
                // grep hint
                let is_grep = adapter_kind == AdapterKind::Grep;
                if is_grep && self.active.query.len() < MIN_GREP_LEN && x < content_w {
                    let hint = format!(" ({}+)", MIN_GREP_LEN - self.active.query.len());
                    let h: String = hint.chars().take(content_w - x).collect();
                    view.put_str(x as u16, 0, &h, Color::Default, Color::Default, Attrs::DIM);
                }
                // count right-aligned
                if !count_str.is_empty() {
                    let pos = w.saturating_sub(count_w);
                    view.put_str(pos as u16, 0, &count_str, Color::Default, Color::Default, Attrs::DIM);
                }
            }
        }
    }
}

// ── overlay render ────────────────────────────────────────────────────────────

fn render_stage_overlay(stage: &mut Stage, view: &mut FrameView<'_>, _cwd: &Path) {
    let w = view.width() as usize;
    let h = view.height() as usize;
    if w == 0 || h == 0 { return; }

    let sep: String = "─".repeat(w);

    // always: bottom sep at row h-1
    view.put_str(0, (h - 1) as u16, &sep, Color::Default, Color::Default, Attrs::DIM);

    if stage.visible.is_empty() {
        let is_grep = stage.kind == StageKind::Adapter(AdapterKind::Grep);
        let needs_min = is_grep && stage.query.len() < MIN_GREP_LEN;
        if h >= 2 && !needs_min {
            view.put_str(0, (h - 2) as u16, "  (no results)", Color::Default, Color::Default, Attrs::DIM);
        }
        return;
    }

    if h < 2 { return; }

    // list: up to LIST_HEIGHT rows, forced odd, capped to h-1 (leave room for bottom sep)
    let n = stage.visible.len();
    let want = n.min(LIST_HEIGHT);
    let want = if want > 0 && want % 2 == 0 { want + 1 } else { want };
    let list_h = want.min(h - 1);

    // list rows: list_start .. list_start+list_h-1, directly above bottom sep
    let list_start = h - 1 - list_h;

    // center selection in list window
    let half = list_h / 2;
    let top_idx = (stage.selected as isize - half as isize).max(0) as usize;
    let top_idx = top_idx.min(n.saturating_sub(list_h));

    for row in 0..list_h {
        let vis_idx = top_idx + row;
        let Some(&data_idx) = stage.visible.get(vis_idx) else { break };
        let is_sel = vis_idx == stage.selected;
        let attrs = if is_sel { Attrs::INVERSE } else { Attrs::NONE };
        let label: String = format!("  {}", &stage.display[data_idx]).chars().take(w).collect();
        view.put_str(0, (list_start + row) as u16, &label, Color::Default, Color::Default, attrs);
    }

    // preview: everything above the list
    // need at least: mid_sep (1) + 1 preview row → list_start >= 2
    if list_start < 2 { return; }
    let mid_sep_row = list_start - 1;
    view.put_str(0, mid_sep_row as u16, &sep, Color::Default, Color::Default, Attrs::DIM);
    if mid_sep_row == 0 { return; }

    let preview_h = mid_sep_row; // rows 0 .. preview_h-1

    // resolve file for selected entry
    let (path, match_line): (Option<PathBuf>, Option<usize>) = match stage.selected_entry() {
        Some(EntryData::File(p)) => (Some(p.clone()), None),
        Some(EntryData::GrepMatch { path, line }) => (Some(path.clone()), Some(*line)),
        _ => (None, None),
    };
    let Some(path) = path else { return };

    let Ok(raw) = fs::read(&path) else { return };
    if raw.len() > 1_048_576 { return; }
    if raw[..raw.len().min(8192)].contains(&0u8) { return; }
    let Ok(content) = std::str::from_utf8(&raw) else { return };
    let file_lines: Vec<&str> = content.lines().collect();
    if file_lines.is_empty() { return; }

    // center match line in preview window
    let match_0 = match_line.map(|l| l.saturating_sub(1)).unwrap_or(0);
    let half_prev = preview_h / 2;
    let raw_start = match_0 as isize - half_prev as isize + stage.preview_scroll;
    let start = raw_start.max(0) as usize;
    let max_start = file_lines.len().saturating_sub(preview_h);
    let start = start.min(max_start);
    // update scroll to actual clamped position
    stage.preview_scroll = start as isize - (match_0 as isize - half_prev as isize);

    for row in 0..preview_h {
        let line_idx = start + row;
        let Some(line) = file_lines.get(line_idx) else { break };
        let is_match = match_line.map_or(false, |ml| line_idx + 1 == ml);
        let attrs = if is_match { Attrs::INVERSE } else { Attrs::DIM };
        let label: String = line.chars().take(w).collect();
        view.put_str(0, row as u16, &label, Color::Default, Color::Default, attrs);
    }
}

// ── file scan ─────────────────────────────────────────────────────────────────

fn scan_files(root: &Path, dir: &Path, display: &mut Vec<String>, data: &mut Vec<EntryData>) {
    if display.len() >= MAX_FILES { return; }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if display.len() >= MAX_FILES { return; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP.contains(&name.as_str()) { continue; }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() { scan_files(root, &path, display, data); }
        else if ft.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            display.push(rel.to_string_lossy().replace('\\', "/"));
            data.push(EntryData::File(path));
        }
    }
}

fn scan_dirs(root: &Path, dir: &Path, display: &mut Vec<String>, data: &mut Vec<EntryData>) {
    if display.len() >= MAX_FILES { return; }
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if display.len() >= MAX_FILES { return; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP.contains(&name.as_str()) { continue; }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            display.push(rel.to_string_lossy().replace('\\', "/"));
            data.push(EntryData::Dir(path.clone()));
            scan_dirs(root, &path, display, data);
        }
    }
}

// ── grep ──────────────────────────────────────────────────────────────────────

fn run_grep(root: &Path, scope: &Path, pattern: &str) -> (Vec<String>, Vec<EntryData>) {
    let mut display = vec![];
    let mut data = vec![];
    grep_walk(root, scope, &pattern.to_lowercase(), &mut display, &mut data);
    (display, data)
}

fn grep_walk(root: &Path, target: &Path, pat: &str, display: &mut Vec<String>, data: &mut Vec<EntryData>) {
    if data.len() >= MAX_GREP { return; }
    if target.is_file() { grep_file(root, target, pat, display, data); return; }
    let Ok(rd) = fs::read_dir(target) else { return };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if data.len() >= MAX_GREP { return; }
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP.contains(&name.as_str()) { continue; }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() { grep_walk(root, &path, pat, display, data); }
        else if ft.is_file() { grep_file(root, &path, pat, display, data); }
    }
}

fn grep_file(root: &Path, path: &Path, pat: &str, display: &mut Vec<String>, data: &mut Vec<EntryData>) {
    let Ok(raw) = fs::read(path) else { return };
    if raw[..raw.len().min(8192)].contains(&0u8) { return; }
    let Ok(content) = std::str::from_utf8(&raw) else { return };
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    for (i, line) in content.lines().enumerate() {
        if data.len() >= MAX_GREP { return; }
        if line.to_lowercase().contains(pat) {
            let snippet: String = line.trim().chars().take(80).collect();
            display.push(format!("{}:{}: {}", rel_str, i + 1, snippet));
            data.push(EntryData::GrepMatch { path: path.to_path_buf(), line: i + 1 });
        }
    }
}

// ── fuzzy ─────────────────────────────────────────────────────────────────────

fn fuzzy_score(item: &str, query_low: &str) -> Option<i32> {
    if query_low.is_empty() { return Some(0); }
    let mut q = query_low.chars().peekable();
    let mut score: i32 = 0;
    let mut prev_matched = false;
    let mut prev_char: Option<char> = None;
    for (i, c) in item.chars().enumerate() {
        let matches = match q.peek() {
            Some(&qc) => if c.is_ascii() { c.to_ascii_lowercase() == qc } else { c.to_lowercase().any(|lc| lc == qc) },
            None => false,
        };
        if matches {
            q.next();
            score += 1;
            if i == 0 { score += 8; }
            if prev_matched { score += 4; }
            if matches!(prev_char, Some('/') | Some('_') | Some('-') | Some('.')) { score += 2; }
            prev_matched = true;
        } else { prev_matched = false; }
        prev_char = Some(c);
    }
    if q.peek().is_none() { Some(score) } else { None }
}