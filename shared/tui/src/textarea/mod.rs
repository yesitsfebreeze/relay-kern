pub mod binding;
pub mod buffer;
pub mod edit_area;
pub mod form;
pub mod history;
pub mod list;
pub mod words;
pub mod wrap;

#[cfg(test)]
mod tests;

pub use binding::{FormAction, FormBindings, KeyChord, ListAction, ListBindings, OnPickAction};
pub use buffer::{Buffer, Pos};
pub use edit_area::{EditArea, EditOutcome, WrapMode};
pub use form::{FormField, FormOutcome, FormState};
pub use history::{Edit, EditKind, Group, History};
pub use list::{ListItem, ListState};
pub use wrap::{hard_wrap, wrap_display, wrap_line, VisualRow};
