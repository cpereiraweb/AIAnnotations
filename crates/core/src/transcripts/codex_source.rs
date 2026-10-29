//! Classifies the Codex `source` field (Inv §3.7 "Source do Codex"). Port of `CodexSource.php`.
//! It never logs: an unknown value is returned so the caller can warn (see `origin`).

use serde_json::Value;

use crate::model::SessionOrigin;

/// String sources treated as interactive without a warning.
const INTERACTIVE_SOURCES: [&str; 3] = ["cli", "vscode", "mcp"];

/// `source` is a string ("cli", "vscode", "exec"), an object such as
/// `{"subagent":{"thread_spawn":{"parent_thread_id":…}}}`, or a string holding that JSON.
/// Returns the origin, the parent session id and the unknown value, if any.
pub fn classify(source: &Value) -> (SessionOrigin, Option<String>, Option<String>) {
    let decoded;
    let mut source = source;
    if let Value::String(s) = source
        && s.trim_start_matches(super::php::PHP_WS).starts_with('{')
        && let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(s)
    {
        decoded = value;
        source = &decoded;
    }

    match source {
        Value::Object(map) => {
            if map.contains_key("subagent") {
                let parent = map
                    .get("subagent")
                    .and_then(|v| v.get("thread_spawn"))
                    .and_then(|v| v.get("parent_thread_id"))
                    .and_then(Value::as_str)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string);
                return (SessionOrigin::Subagent, parent, None);
            }
            // PHP encodes an empty array as "[]".
            let raw = if map.is_empty() {
                "[]".to_string()
            } else {
                source.to_string()
            };
            (SessionOrigin::Interactive, None, Some(raw))
        }
        Value::Array(_) => (SessionOrigin::Interactive, None, Some(source.to_string())),
        Value::String(s) if s == "exec" => (SessionOrigin::Automation, None, None),
        Value::String(s) if s.contains("subagent") => (SessionOrigin::Subagent, None, None),
        Value::String(s) if !s.is_empty() && !INTERACTIVE_SOURCES.contains(&s.as_str()) => {
            (SessionOrigin::Interactive, None, Some(s.clone()))
        }
        _ => (SessionOrigin::Interactive, None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::Tool;
    use crate::transcripts::origin::warn_unknown_once;
    use serde_json::json;

    fn unknown(source: Value) -> Option<String> {
        classify(&source).2
    }

    /// source desconhecido continua interactive e registra um warning só uma vez por valor
    #[test]
    fn unknown_source_stays_interactive_and_warns_once_per_value() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = db::open(&dir.path().join("t.db")).unwrap();
        let tx = db::write_tx(&mut conn).unwrap();
        let now = 1_800_000_000_000;

        let atlas = classify(&json!("atlas"));
        assert_eq!(atlas.0, SessionOrigin::Interactive);
        assert_eq!(atlas.2.as_deref(), Some("atlas"));
        let plugin = classify(&json!(r#"{"plugin":{"x":1}}"#));
        assert_eq!(plugin.0, SessionOrigin::Interactive);
        assert_eq!(plugin.2.as_deref(), Some(r#"{"plugin":{"x":1}}"#));

        assert!(warn_unknown_once(&tx, Tool::Codex, "atlas", now).unwrap());
        assert!(!warn_unknown_once(&tx, Tool::Codex, "atlas", now).unwrap());
        assert!(warn_unknown_once(&tx, Tool::Codex, r#"{"plugin":{"x":1}}"#, now).unwrap());
    }

    /// não registra warning para os sources conhecidos
    #[test]
    fn known_sources_report_no_unknown_value() {
        let known = [
            json!("cli"),
            json!("vscode"),
            json!("exec"),
            json!("mcp"),
            Value::Null,
            json!(r#"{"subagent":{"other":"guardian"}}"#),
            json!({"subagent": {"thread_spawn": {"parent_thread_id": "p"}}}),
        ];
        for source in known {
            assert_eq!(unknown(source.clone()), None, "{source}");
        }
    }

    /// o cache evita repetir o aviso na rodada seguinte do agendador (novo processo)
    #[test]
    fn warning_cache_survives_a_new_process() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let now = 1_800_000_000_000;

        let value = unknown(json!("atlas")).unwrap();
        let mut first = db::open(&path).unwrap();
        let tx = db::write_tx(&mut first).unwrap();
        assert!(warn_unknown_once(&tx, Tool::Codex, &value, now).unwrap());
        tx.commit().unwrap();
        drop(first);
        // Reopening simulates another process: only the persisted state remembers.
        let mut second = db::open(&path).unwrap();
        let tx = db::write_tx(&mut second).unwrap();
        assert!(!warn_unknown_once(&tx, Tool::Codex, &value, now).unwrap());
    }

    #[test]
    fn classifies_exec_subagent_and_parent() {
        assert_eq!(classify(&json!("exec")).0, SessionOrigin::Automation);
        assert_eq!(
            classify(&json!("thread_subagent_x")).0,
            SessionOrigin::Subagent
        );
        let (origin, parent, unknown) = classify(&json!(
            {"subagent": {"thread_spawn": {"parent_thread_id": "pai-1", "depth": 1}}}
        ));
        assert_eq!(origin, SessionOrigin::Subagent);
        assert_eq!(parent.as_deref(), Some("pai-1"));
        assert_eq!(unknown, None);
        // JSON in a string is decoded; a subagent object without a parent has none.
        let (origin, parent, _) = classify(&json!(r#" {"subagent":{"other":"guardian"}}"#));
        assert_eq!((origin, parent), (SessionOrigin::Subagent, None));
    }

    #[test]
    fn unknown_object_without_subagent_and_malformed_json() {
        assert_eq!(unknown(json!({})).as_deref(), Some("[]"));
        assert_eq!(unknown(json!({"a": 1})).as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(unknown(json!("{broken")).as_deref(), Some("{broken"));
        assert_eq!(unknown(json!(42)), None);
        assert_eq!(unknown(json!("")), None);
    }
}
