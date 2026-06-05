use super::buffer::Pos;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditKind {
	InsertRun,
	DeleteRun,
	WordDelete,
	Other,
}

#[derive(Clone, Debug)]
pub struct Edit {
	pub start: Pos,
	pub end: Pos,
	pub text: String,
	pub is_insert: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Group {
	pub kind: Option<EditKind>,
	pub edits: Vec<Edit>,
	pub cursor_before: Pos,
	pub cursor_after: Pos,
}

pub struct History {
	pub(crate) undo: Vec<Group>,
	redo: Vec<Group>,
	pending: Option<Group>,
	capacity: usize,
}

impl Default for History {
	fn default() -> Self {
		Self::new(256)
	}
}

impl History {
	pub fn new(capacity: usize) -> Self {
		History {
			undo: Vec::new(),
			redo: Vec::new(),
			pending: None,
			capacity: capacity.max(1),
		}
	}

	pub fn record(&mut self, kind: EditKind, cursor_before: Pos, edit: Edit, cursor_after: Pos) {
		self.redo.clear();
		if let Some(g) = &self.pending {
			if g.kind.as_ref() != Some(&kind) {
				self.commit();
			}
		}
		let pending = self.pending.get_or_insert_with(|| Group {
			kind: Some(kind.clone()),
			edits: Vec::new(),
			cursor_before,
			cursor_after,
		});
		pending.edits.push(edit);
		pending.cursor_after = cursor_after;
	}

	pub fn commit(&mut self) {
		if let Some(g) = self.pending.take() {
			if !g.edits.is_empty() {
				self.undo.push(g);
				if self.undo.len() > self.capacity {
					self.undo.remove(0);
				}
			}
		}
	}

	pub fn can_undo(&self) -> bool {
		self.pending.is_some() || !self.undo.is_empty()
	}

	pub fn can_redo(&self) -> bool {
		!self.redo.is_empty()
	}

	pub fn pop_undo(&mut self) -> Option<Group> {
		self.commit();
		self.undo.pop()
	}

	pub fn push_redo(&mut self, g: Group) {
		self.redo.push(g);
	}

	pub fn pop_redo(&mut self) -> Option<Group> {
		self.redo.pop()
	}

	pub fn push_undo(&mut self, g: Group) {
		self.undo.push(g);
	}
}
