use std::sync::Arc;

pub type LlmFunc = Arc<dyn Fn(&str) -> String + Send + Sync>;
pub type EmbedFunc = Arc<dyn Fn(&str) -> Result<Vec<f64>, String> + Send + Sync>;
