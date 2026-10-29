use std::path::PathBuf;
use std::str::FromStr;

use serde_json::Value;

use crate::model::{EndReason, Tool};
use crate::time::parse_ts;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Start,
    End,
}

/// A SessionStart/SessionEnd hook event, already validated by the ingestor (Inv §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub tool: Tool,
    pub kind: EventKind,
    pub session_id: String,
    pub cwd: PathBuf,
    pub transcript_path: Option<PathBuf>,
    pub model: Option<String>,
    /// `source` of a start: startup, resume, clear, compact, fork.
    pub source: Option<String>,
    /// `reason` of an end: prompt_input_exit, clear, logout, other.
    pub reason: Option<String>,
    pub received_at_ms: i64,
}

fn non_empty_str<'a>(payload: &'a Value, key: &str) -> Option<&'a str> {
    payload.get(key)?.as_str().filter(|s| !s.is_empty())
}

impl HookEvent {
    /// Validates a spool envelope (Inv §3.2). `session_id` and `cwd` are required; an invalid
    /// `received_at` falls back to the file's mtime; an unknown or `idle` end reason becomes `other`.
    pub fn from_envelope(v: &Value, file_mtime_ms: i64) -> Result<HookEvent> {
        let invalid = |what: &str| Error::InvalidEvent(what.to_string());
        let tool = v
            .get("tool")
            .and_then(Value::as_str)
            .and_then(|t| Tool::from_str(t).ok())
            .ok_or_else(|| invalid("unknown tool"))?;
        let kind = match v.get("event").and_then(Value::as_str) {
            Some("start") => EventKind::Start,
            Some("end") => EventKind::End,
            _ => return Err(invalid("unknown event")),
        };
        let payload = v
            .get("payload")
            .filter(|p| p.is_object())
            .ok_or_else(|| invalid("payload is not an object"))?;
        let session_id =
            non_empty_str(payload, "session_id").ok_or_else(|| invalid("missing session_id"))?;
        let cwd = non_empty_str(payload, "cwd").ok_or_else(|| invalid("missing cwd"))?;
        let received_at_ms = v
            .get("received_at")
            .and_then(Value::as_str)
            .and_then(parse_ts)
            .unwrap_or(file_mtime_ms);
        let text = |key| non_empty_str(payload, key).map(str::to_string);
        let reason = (kind == EventKind::End).then(|| {
            match text("reason").and_then(|r| EndReason::from_str(&r).ok()) {
                Some(EndReason::Idle) | None => EndReason::Other,
                Some(r) => r,
            }
            .as_str()
            .to_string()
        });
        Ok(HookEvent {
            tool,
            kind,
            session_id: session_id.to_string(),
            cwd: PathBuf::from(cwd),
            transcript_path: text("transcript_path").map(PathBuf::from),
            model: text("model"),
            source: if kind == EventKind::Start {
                text("source")
            } else {
                None
            },
            reason,
            received_at_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn env(event: &str, payload: Value) -> Value {
        json!({"tool":"claude","event":event,"received_at":"2026-10-29T10:00:00-03:00","payload":payload})
    }

    #[test]
    fn parses_start_with_offset_timestamp() {
        let ev = HookEvent::from_envelope(
            &env(
                "start",
                json!({"session_id":"s","cwd":"/p","source":"resume","model":"m","transcript_path":"/t.jsonl"}),
            ),
            1,
        )
        .unwrap();
        assert_eq!(ev.kind, EventKind::Start);
        assert_eq!(ev.received_at_ms, parse_ts("2026-10-29T13:00:00Z").unwrap());
        assert_eq!(ev.source.as_deref(), Some("resume"));
        assert_eq!(ev.transcript_path, Some(PathBuf::from("/t.jsonl")));
        assert_eq!(ev.reason, None);
    }

    #[test]
    fn end_reason_unknown_or_idle_becomes_other() {
        for (raw, want) in [
            ("clear", "clear"),
            ("idle", "other"),
            ("bogus", "other"),
            ("logout", "logout"),
        ] {
            let ev = HookEvent::from_envelope(
                &env("end", json!({"session_id":"s","cwd":"/p","reason":raw})),
                1,
            )
            .unwrap();
            assert_eq!(ev.reason.as_deref(), Some(want), "{raw}");
        }
        let ev =
            HookEvent::from_envelope(&env("end", json!({"session_id":"s","cwd":"/p"})), 1).unwrap();
        assert_eq!(ev.reason.as_deref(), Some("other"));
    }

    #[test]
    fn invalid_received_at_uses_mtime_and_required_fields_are_enforced() {
        let mut v = env("start", json!({"session_id":"s","cwd":"/p"}));
        v["received_at"] = json!("lixo");
        assert_eq!(HookEvent::from_envelope(&v, 42).unwrap().received_at_ms, 42);
        for payload in [
            json!({"cwd":"/p"}),
            json!({"session_id":"","cwd":"/p"}),
            json!({"session_id":"s"}),
            json!({"session_id":"s","cwd":3}),
            Value::Null,
        ] {
            assert!(matches!(
                HookEvent::from_envelope(&env("start", payload), 1),
                Err(Error::InvalidEvent(_))
            ));
        }
        let mut bad_tool = env("start", json!({"session_id":"s","cwd":"/p"}));
        bad_tool["tool"] = json!("gemini");
        assert!(HookEvent::from_envelope(&bad_tool, 1).is_err());
        assert!(
            HookEvent::from_envelope(&env("pause", json!({"session_id":"s","cwd":"/p"})), 1)
                .is_err()
        );
    }
}
