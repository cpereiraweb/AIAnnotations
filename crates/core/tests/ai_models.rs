//! Model lists for the first-run selects: Claude aliases and Codex's own models cache.

use aiannotations_core::ai::models::{CLAUDE_ALIASES, ModelOption, codex_models};
use serde_json::json;

fn option(value: &str, label: &str) -> ModelOption {
    ModelOption {
        value: value.into(),
        label: label.into(),
    }
}

#[test]
fn claude_aliases_include_the_defaults() {
    for m in ["haiku", "sonnet", "opus"] {
        assert!(CLAUDE_ALIASES.contains(&m), "{m}");
    }
}

#[test]
fn codex_models_come_from_the_cache_listed_only_by_priority() {
    let dir = tempfile::tempdir().unwrap();
    let cache = json!({"fetched_at": "x", "models": [
        {"slug": "b-model", "display_name": "B Model", "visibility": "list", "priority": 3},
        {"slug": "hidden", "display_name": "Hidden", "visibility": "hide", "priority": 1},
        {"slug": "a-model", "display_name": "A Model", "visibility": "list", "priority": 2},
        {"slug": "no-name", "visibility": "list", "priority": 4},
        {"display_name": "No slug", "visibility": "list", "priority": 0},
    ]});
    std::fs::write(dir.path().join("models_cache.json"), cache.to_string()).unwrap();
    assert_eq!(
        codex_models(dir.path()),
        vec![
            option("a-model", "A Model"),
            option("b-model", "B Model"),
            option("no-name", "no-name"),
        ]
    );
}

#[test]
fn codex_models_missing_or_broken_cache_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    assert!(codex_models(dir.path()).is_empty());
    std::fs::write(dir.path().join("models_cache.json"), "{ nope").unwrap();
    assert!(codex_models(dir.path()).is_empty());
    std::fs::write(dir.path().join("models_cache.json"), r#"{"models": 3}"#).unwrap();
    assert!(codex_models(dir.path()).is_empty());
}
