use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Default)]
pub struct State {
	pub project_root: PathBuf,
	pub active_recipe: Option<String>,
	pub current_model: Option<String>,
	pub current_provider: Option<String>,
	pub journal_day: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StateHandle(Arc<RwLock<State>>);

impl StateHandle {
	pub fn new(initial: State) -> Self {
		Self(Arc::new(RwLock::new(initial)))
}

	pub fn snapshot(&self) -> State {
		self.0.read().expect("state lock poisoned").clone()
}

	pub fn update(&self, f: impl FnOnce(&mut State)) {
		let mut g = self.0.write().expect("state lock poisoned");
		f(&mut g);
}

	pub fn render(&self) -> String {
		let s = self.snapshot();
		let mut out = String::from("state:\n");
		out.push_str(&format!("  project        {}\n", s.project_root.display()));
		out.push_str(&format!(
			"  active recipe  {}\n",
			s.active_recipe.as_deref().unwrap_or("(none)")
		));
		out.push_str(&format!(
			"  model          {}\n",
			match (s.current_provider.as_deref(), s.current_model.as_deref()) {
				(Some(p), Some(m)) => format!("{p} / {m}"),
				_ => "(unbound)".into(),
			}
		));
		out.push_str(&format!(
			"  journal day    {}\n",
			s.journal_day.as_deref().unwrap_or("(not opened)")
		));
		while out.ends_with('\n') {
			out.pop();
		}
		out
}
}
