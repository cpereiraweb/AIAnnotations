//! Sessions found on disk without the hooks (crash, kill, Codex desktop app, history from before
//! the install). Port of `app/Sessions/Discovery/{Claude,Codex}SessionDiscovery.php` (Inv §3.5).

use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::types::Value;

use crate::Result;
use crate::model::Tool;
use crate::transcripts::codex_history::{self, CODEX_STATE_DB};
use crate::transcripts::jsonl;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSession {
    pub tool: Tool,
    pub session_id: String,
    pub transcript_path: Option<PathBuf>,
    /// Always set by the discoveries (v1 skips sessions without one); required to create a session.
    pub cwd: Option<String>,
    /// Claude: the transcript's mtime in whole seconds (PHP `getMTime`); Codex: the thread's
    /// `updated_at_ms`.
    pub last_activity_ms: i64,
}

/// Entries of `dir` in name order; an unreadable or missing directory has none.
fn entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut all: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    all.sort();
    all
}

fn mtime_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(secs).ok()?.checked_mul(1000)
}

/// First non-empty `cwd` declared in the transcript.
fn first_cwd(path: &Path) -> Option<String> {
    let mut cwd = None;
    jsonl::for_each_value_until(path, |v| match v.get("cwd").and_then(|c| c.as_str()) {
        Some(c) if !c.is_empty() => {
            cwd = Some(c.to_string());
            ControlFlow::Break(())
        }
        _ => ControlFlow::Continue(()),
    })
    .ok()?;
    cwd
}

/// `{claude_dir}/projects/*/*.jsonl`: only the first level of each project, so the subagent
/// transcripts under `<session>/subagents/` are never visited. Transcripts without a `cwd` are
/// skipped.
pub fn claude(claude_dir: &Path, since_ms: Option<i64>) -> Result<Vec<DiscoveredSession>> {
    let mut found = Vec::new();
    for project in entries(&claude_dir.join("projects")) {
        if !project.is_dir() {
            continue;
        }
        for file in entries(&project) {
            if !file.is_file() || file.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            let Some(last_activity_ms) = mtime_ms(&file) else {
                continue;
            };
            if since_ms.is_some_and(|since| last_activity_ms < since) {
                continue;
            }
            let (Some(cwd), Some(stem)) = (first_cwd(&file), file.file_stem()) else {
                continue;
            };
            found.push(DiscoveredSession {
                tool: Tool::Claude,
                session_id: stem.to_string_lossy().into_owned(),
                transcript_path: Some(file.clone()),
                cwd: Some(cwd),
                last_activity_ms,
            });
        }
    }
    Ok(found)
}

/// PHP `is_numeric` + `(int)` over a SQLite value.
fn as_int(v: &Value) -> Option<i64> {
    match v {
        Value::Integer(i) => Some(*i),
        Value::Real(f) => Some(*f as i64),
        Value::Text(t) => t
            .trim()
            .parse::<i64>()
            .ok()
            .or_else(|| t.trim().parse::<f64>().ok().map(|f| f as i64)),
        _ => None,
    }
}

fn as_text(v: &Value) -> Option<&str> {
    match v {
        Value::Text(t) if !t.is_empty() => Some(t),
        _ => None,
    }
}

/// Threads of `{codex_home}/state_5.sqlite`, opened read-only. Includes subagent and exec threads:
/// the transcript reader classifies them. Absent database: none; a database that cannot be
/// queried (unexpected schema, corrupt) is an error, isolated by the collector.
pub fn codex(codex_home: &Path, since_ms: Option<i64>) -> Result<Vec<DiscoveredSession>> {
    let path = codex_home.join(CODEX_STATE_DB);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let conn = codex_history::connect(&path)?;
    let mut stmt = conn.prepare(
        "SELECT id, rollout_path, cwd, updated_at, updated_at_ms FROM threads \
         WHERE COALESCE(updated_at_ms, updated_at * 1000) >= ?1 \
         ORDER BY COALESCE(updated_at_ms, updated_at * 1000)",
    )?;
    // Bound as an integer: in SQLite text always sorts above numbers.
    let rows = stmt
        .query_map([since_ms.unwrap_or(0)], |r| {
            Ok([r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?])
        })?
        .collect::<rusqlite::Result<Vec<[Value; 5]>>>()?;

    Ok(rows
        .iter()
        .filter_map(|[id, rollout, cwd, updated_at, updated_at_ms]| {
            let ms = as_int(updated_at_ms).or_else(|| as_int(updated_at)?.checked_mul(1000))?;
            let (id, cwd) = (as_text(id)?, as_text(cwd)?);
            let transcript_path = as_text(rollout).map(PathBuf::from).filter(|p| p.is_file());
            Some(DiscoveredSession {
                tool: Tool::Codex,
                session_id: id.to_string(),
                transcript_path,
                cwd: Some(cwd.to_string()),
                last_activity_ms: ms,
            })
        })
        .collect())
}
