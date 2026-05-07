mod file_picker;

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;

use crossterm::{
	event::{self, Event, KeyEventKind},
	terminal::{self},
};
use tui::{
	input::{Key, KeyCode, Mods},
	render::{Attrs, Cell, Color, FrameView, Region, StdoutSurface, Renderer},
	textarea::{EditArea, EditOutcome, WrapMode},
};

use file_picker::{FilePicker, PickerOutcome};

fn main() -> anyhow::Result<()> {
	terminal::enable_raw_mode()?;
	write!(io::stdout(), "\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H")?;
	io::stdout().flush()?;
	let result = run();
	write!(io::stdout(), "\x1b[?25h\x1b[?1049l")?;
	io::stdout().flush()?;
	terminal::disable_raw_mode()?;
	result
}

fn run() -> anyhow::Result<()> {
	let (w, h) = terminal::size()?;
	let mut renderer = Renderer::new(w, h);
	let mut surface = StdoutSurface::new(w, h);
	let mut editor = EditArea::new();
	editor.set_wrap(WrapMode::Soft);
	let mut picker: Option<FilePicker> = None;
	let mut current_file: Option<PathBuf> = None;

	render_frame(&mut renderer, &mut surface, &mut editor, picker.as_mut(), current_file.as_deref())?;

	loop {
		let ev = event::read()?;
		let dirty = match ev {
			Event::Key(ke) if ke.kind != KeyEventKind::Press => continue,
			Event::Key(ke) => {
				let key = Key::from(&ke);

				if key.mods.contains(Mods::CTRL) {
					if let KeyCode::Char(c) = key.code {
						if c == 'q' || c == 'Q' { break; }
					}
				}

				if key.code == KeyCode::Char('o') && key.mods == Mods::CTRL {
					let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
					picker = Some(FilePicker::new(cwd));
					true
				} else if let Some(p) = picker.as_mut() {
					match p.handle_key(&key) {
						PickerOutcome::Cancel => { picker = None; true }
						PickerOutcome::Open(path) => {
							picker = None;
							if let Ok(contents) = std::fs::read_to_string(&path) {
								editor.set_text(&contents);
								current_file = Some(path);
							}
							true
						}
						PickerOutcome::OpenAt(path, line) => {
							picker = None;
							if let Ok(contents) = std::fs::read_to_string(&path) {
								editor.set_text(&contents);
								editor.goto_line(line);
								current_file = Some(path);
							}
							true
						}
						PickerOutcome::Handled => true,
					}
				} else {
					match editor.handle_key(&ke) {
						EditOutcome::Submit | EditOutcome::Cancel => break,
						_ => true,
					}
				}
			}
			Event::Resize(nw, nh) => { renderer.resize(nw, nh); true }
			_ => false,
		};

		if dirty {
			render_frame(&mut renderer, &mut surface, &mut editor, picker.as_mut(), current_file.as_deref())?;
		}
	}
	Ok(())
}

fn render_frame(
	renderer: &mut Renderer,
	surface: &mut StdoutSurface,
	editor: &mut EditArea,
	picker: Option<&mut FilePicker>,
	current_file: Option<&std::path::Path>,
) -> anyhow::Result<()> {
	let (w, h) = terminal::size()?;
	let (rw, rh) = renderer.size();
	if (w, h) != (rw, rh) { renderer.resize(w, h); }
	if h == 0 { return Ok(()); }

	let mut view = renderer.frame_view(Region::new(0, 0, w, h));
	view.fill(Cell::new(' ').style(Color::Default, Color::Default, Attrs::NONE));

	// bar is always the last row; everything above is either editor or picker overlay
	let bar_row = h - 1;
	let main_h = bar_row; // rows 0..bar_row-1

	if let Some(p) = picker {
		if main_h > 0 {
			let mut overlay = view.sub(Region::new(0, 0, w, main_h));
			p.render_overlay(&mut overlay);
		}
		let mut bar = view.sub(Region::new(0, bar_row, w, 1));
		p.render_bar(&mut bar);
	} else {
		if main_h > 0 {
			let mut ev = view.sub(Region::new(0, 0, w, main_h));
			editor.render(&mut ev);
		}
		let mut bar = view.sub(Region::new(0, bar_row, w, 1));
		render_status(&mut bar, current_file);
	}

	renderer.present(surface)?;
	Ok(())
}

fn render_status(view: &mut FrameView<'_>, current_file: Option<&std::path::Path>) {
	let w = view.width() as usize;
	let file_part = current_file
		.and_then(|p| p.file_name())
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_default();
	let hint = "Ctrl+O open  Ctrl+Q quit";
	let content = if file_part.is_empty() {
		format!(" {}", hint)
	} else {
		format!(" {}  {}", file_part, hint)
	};
	let content: String = content.chars().take(w).collect();
	view.put_str(0, 0, &content, Color::Default, Color::Default, Attrs::INVERSE);
}