//! What the first-run wizard offers as models, so they are picked instead of typed.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

/// Aliases `claude --model` accepts for the latest model of each family.
pub const CLAUDE_ALIASES: [&str; 4] = ["haiku", "sonnet", "opus", "fable"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelOption {
    /// What goes to `--model`.
    pub value: String,
    pub label: String,
}

/// The models Codex lists in its picker, from its own cache (`$CODEX_HOME/models_cache.json`),
/// by priority. Empty when the cache is missing or unreadable: the user keeps Codex's default.
pub fn codex_models(codex_home: &Path) -> Vec<ModelOption> {
    let Ok(text) = std::fs::read_to_string(codex_home.join("models_cache.json")) else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(models) = cache.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut listed: Vec<(i64, ModelOption)> = models
        .iter()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let slug = m.get("slug").and_then(Value::as_str)?;
            let label = m
                .get("display_name")
                .and_then(Value::as_str)
                .unwrap_or(slug);
            let priority = m
                .get("priority")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            Some((
                priority,
                ModelOption {
                    value: slug.into(),
                    label: label.into(),
                },
            ))
        })
        .collect();
    listed.sort_by_key(|(p, _)| *p);
    listed.into_iter().map(|(_, m)| m).collect()
}
